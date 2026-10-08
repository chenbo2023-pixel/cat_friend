// 猫岗哨 —— 串口协议帧解析 + 心跳看门狗 测试固件
// 对应《技术方案设计.md》第5~11节的帧结构与超时阈值定义。
// 目的：在没有真实云台/树莓派的情况下，用电脑（跑 test_protocol.py）模拟树莓派一侧，
// 验证帧解析框架、心跳超时判定、指令确认帧回传是否正确。
// 之后云台驱动板到货，只需把 handleGimbalCmd/handleActuatorTrigger 里的
// "打印+回传固定值" 换成真实舵机控制/读值即可，帧解析和看门狗逻辑不用改。

#include <Arduino.h>

// ---------- 帧类型编号（技术方案设计.md 第7节） ----------
#define FRAME_HEARTBEAT        0x01
#define FRAME_GIMBAL_CMD       0x02
#define FRAME_ACTUATOR_TRIGGER 0x03
#define FRAME_GIMBAL_ACK       0x04
#define FRAME_ACTUATOR_ACK     0x05
// 0x06 紧急停止帧：P1占位，本固件不处理

#define FRAME_HEADER_1 0xAA
#define FRAME_HEADER_2 0x55

#define HEARTBEAT_SEND_INTERVAL_MS 200
#define HEARTBEAT_TIMEOUT_MS       1000

// ---------- 帧接收状态机 ----------
enum ParseState { WAIT_H1, WAIT_H2, WAIT_LEN, WAIT_PAYLOAD, WAIT_CHECKSUM };
static ParseState parseState = WAIT_H1;
static uint8_t payloadLen = 0;      // 类型字节+数据体 合计长度
static uint8_t payloadIdx = 0;
static uint8_t payloadBuf[32];      // payloadBuf[0]=帧类型，payloadBuf[1..]=数据体

static unsigned long lastRecvHeartbeatMs = 0;
static unsigned long lastSendHeartbeatMs = 0;
static bool inSafetyMode = false;

void sendFrame(uint8_t type, const uint8_t* body, uint8_t bodyLen) {
  uint8_t len = 1 + bodyLen; // 类型字节 + 数据体
  uint8_t checksum = len ^ type;
  for (uint8_t i = 0; i < bodyLen; i++) checksum ^= body[i];

  Serial.write(FRAME_HEADER_1);
  Serial.write(FRAME_HEADER_2);
  Serial.write(len);
  Serial.write(type);
  if (bodyLen > 0) Serial.write(body, bodyLen);
  Serial.write(checksum);
}

void handleHeartbeat() {
  bool wasInSafetyMode = inSafetyMode;
  lastRecvHeartbeatMs = millis();
  inSafetyMode = false;
  if (wasInSafetyMode) {
    USBSerial.println("SAFETY_MODE_RECOVERED");
  }
}

void handleGimbalCmd(const uint8_t* body) {
  int16_t pan  = (int16_t)(body[0] | (body[1] << 8));
  int16_t tilt = (int16_t)(body[2] | (body[3] << 8));
  USBSerial.print("GIMBAL_CMD pan=");
  USBSerial.print(pan);
  USBSerial.print(" tilt=");
  USBSerial.println(tilt);

  // 无真实云台，直接把指令角度当作"执行后实际角度"回传，仅验证协议往返
  uint8_t ackBody[5];
  ackBody[0] = 0x00; // 成功
  ackBody[1] = body[0];
  ackBody[2] = body[1];
  ackBody[3] = body[2];
  ackBody[4] = body[3];
  sendFrame(FRAME_GIMBAL_ACK, ackBody, sizeof(ackBody));
}

void handleActuatorTrigger(const uint8_t* body) {
  uint8_t actionType = body[0];
  USBSerial.print("ACTUATOR_TRIGGER action_type=0x");
  USBSerial.println(actionType, HEX);

  uint8_t ackBody[2];
  ackBody[0] = actionType;
  ackBody[1] = 0x00; // 成功
  sendFrame(FRAME_ACTUATOR_ACK, ackBody, sizeof(ackBody));
}

void dispatchFrame(uint8_t type, const uint8_t* body, uint8_t bodyLen) {
  switch (type) {
    case FRAME_HEARTBEAT:
      handleHeartbeat();
      break;
    case FRAME_GIMBAL_CMD:
      if (bodyLen == 4) handleGimbalCmd(body);
      break;
    case FRAME_ACTUATOR_TRIGGER:
      if (bodyLen == 1) handleActuatorTrigger(body);
      break;
    default:
      // 未知/暂不处理的帧类型，忽略
      break;
  }
}

void feedParser(uint8_t b) {
  switch (parseState) {
    case WAIT_H1:
      if (b == FRAME_HEADER_1) parseState = WAIT_H2;
      break;
    case WAIT_H2:
      parseState = (b == FRAME_HEADER_2) ? WAIT_LEN : WAIT_H1;
      break;
    case WAIT_LEN:
      payloadLen = b;
      payloadIdx = 0;
      parseState = (payloadLen > 0 && payloadLen <= sizeof(payloadBuf)) ? WAIT_PAYLOAD : WAIT_H1;
      break;
    case WAIT_PAYLOAD:
      payloadBuf[payloadIdx++] = b;
      if (payloadIdx >= payloadLen) parseState = WAIT_CHECKSUM;
      break;
    case WAIT_CHECKSUM: {
      uint8_t expected = payloadLen;
      for (uint8_t i = 0; i < payloadLen; i++) expected ^= payloadBuf[i];
      if (expected == b) {
        dispatchFrame(payloadBuf[0], payloadBuf + 1, payloadLen - 1);
      } else {
        USBSerial.println("CHECKSUM_MISMATCH");
      }
      parseState = WAIT_H1;
      break;
    }
  }
}

void setup() {
  Serial.begin(115200);       // 纯二进制协议帧，走COM口（外接UART0）
  USBSerial.begin(115200);    // 调试文本，走USB口（片内Native USB），与协议帧物理分离
  lastRecvHeartbeatMs = millis();
  USBSerial.println("protocol_test ready");
}

void loop() {
  while (Serial.available() > 0) {
    feedParser((uint8_t)Serial.read());
  }

  unsigned long now = millis();

  if (now - lastSendHeartbeatMs >= HEARTBEAT_SEND_INTERVAL_MS) {
    lastSendHeartbeatMs = now;
    sendFrame(FRAME_HEARTBEAT, nullptr, 0);
  }

  if (!inSafetyMode && (now - lastRecvHeartbeatMs > HEARTBEAT_TIMEOUT_MS)) {
    inSafetyMode = true;
    USBSerial.println("SAFETY_MODE_TRIGGERED");
  }
}
