#!/usr/bin/env python3
"""
猫岗哨 —— 串口协议测试脚本

模拟《技术方案设计.md》里树莓派 esp32_bridge_node 的角色，用电脑USB串口
直连ESP32（刷 protocol_test.ino），验证：
1. 帧收发格式（帧头/长度/校验）是否双端一致
2. 云台指令帧 → 云台确认帧 往返
3. 执行器触发帧 → 执行器确认帧 往返
4. 心跳双向互发 + 停发心跳后ESP32是否在阈值时间内触发安全模式

协议帧（COM口）与调试文本（USB口/USBSerial）物理上走两条独立串口，互不干扰，
不需要在接收端靠猜字节模式区分。

用法：
    pip install pyserial
    python3 test_protocol.py /dev/tty.usbserial-xxxx /dev/tty.usbmodem-xxxx
（不传参数时默认尝试 /dev/tty.usbserial-0001 和 /dev/tty.usbmodem1201，
 找不到设备时脚本会报错并提示用 `ls /dev/tty.*` 自行确认端口）
"""

import sys
import time
import threading

import serial

FRAME_HEARTBEAT = 0x01
FRAME_GIMBAL_CMD = 0x02
FRAME_ACTUATOR_TRIGGER = 0x03
FRAME_GIMBAL_ACK = 0x04
FRAME_ACTUATOR_ACK = 0x05

HEADER = bytes([0xAA, 0x55])


def build_frame(frame_type: int, body: bytes = b"") -> bytes:
    length = 1 + len(body)
    checksum = length ^ frame_type
    for b in body:
        checksum ^= b
    return HEADER + bytes([length, frame_type]) + body + bytes([checksum])


class FrameReader:
    """
    从COM口（纯二进制协议帧）字节流里切出完整帧。
    调试文本已经改到USB口（USBSerial）独立输出，这里不再需要"猜文本"的兼容逻辑。
    """

    def __init__(self):
        self.state = "H1"
        self.length = 0
        self.buf = bytearray()

    def feed(self, b: int):
        if self.state == "H1":
            if b == 0xAA:
                self.state = "H2"
            return None
        elif self.state == "H2":
            self.state = "LEN" if b == 0x55 else "H1"
            return None
        elif self.state == "LEN":
            self.length = b
            self.buf = bytearray()
            self.state = "PAYLOAD"
        elif self.state == "PAYLOAD":
            self.buf.append(b)
            if len(self.buf) >= self.length:
                self.state = "CHECKSUM"
        elif self.state == "CHECKSUM":
            expected = self.length
            for x in self.buf:
                expected ^= x
            self.state = "H1"
            if expected == b:
                return bytes(self.buf)  # payload[0]=frame_type, payload[1:]=body
            else:
                print(f"[WARN] checksum mismatch, payload={self.buf.hex()}")
        return None


def reader_thread(ser: serial.Serial, stop_event: threading.Event, send_heartbeat_event: threading.Event):
    reader = FrameReader()
    last_heartbeat_print = 0
    while not stop_event.is_set():
        data = ser.read(1)
        if not data:
            continue
        frame = reader.feed(data[0])
        if frame is None:
            continue
        frame_type, body = frame[0], frame[1:]
        if frame_type == FRAME_HEARTBEAT:
            now = time.time()
            if now - last_heartbeat_print > 1:  # 心跳太频繁，1秒打印一次即可
                print("[RECV] heartbeat from ESP32")
                last_heartbeat_print = now
            if send_heartbeat_event.is_set():
                ser.write(build_frame(FRAME_HEARTBEAT))
        elif frame_type == FRAME_GIMBAL_ACK:
            status, pan, tilt = body[0], int.from_bytes(body[1:3], "little", signed=True), int.from_bytes(body[3:5], "little", signed=True)
            print(f"[RECV] gimbal_ack status={status} pan={pan} tilt={tilt}")
        elif frame_type == FRAME_ACTUATOR_ACK:
            action_type, status = body[0], body[1]
            print(f"[RECV] actuator_ack action_type=0x{action_type:02x} status={status}")
        else:
            print(f"[RECV] unknown frame_type=0x{frame_type:02x} body={body.hex()}")


def usb_text_reader_thread(usb_ser: serial.Serial, stop_event: threading.Event):
    """从USB口（USBSerial）读取调试文本并打印，如 SAFETY_MODE_TRIGGERED"""
    while not stop_event.is_set():
        line = usb_ser.readline()
        if line:
            text = line.decode("ascii", errors="replace").strip()
            if text:
                print(f"[TEXT] {text}")


def main():
    port = sys.argv[1] if len(sys.argv) > 1 else "/dev/tty.usbserial-0001"
    usb_port = sys.argv[2] if len(sys.argv) > 2 else "/dev/tty.usbmodem1201"
    print(f"Opening {port} (协议帧) 和 {usb_port} (调试文本) ...")
    ser = serial.Serial(port, 115200, timeout=1)
    usb_ser = serial.Serial(usb_port, 115200, timeout=1)
    time.sleep(2)  # 等ESP32复位完成

    stop_event = threading.Event()
    send_heartbeat_event = threading.Event()
    send_heartbeat_event.set()  # 默认互发心跳，维持ESP32不进入安全模式

    t = threading.Thread(target=reader_thread, args=(ser, stop_event, send_heartbeat_event), daemon=True)
    t.start()
    t_text = threading.Thread(target=usb_text_reader_thread, args=(usb_ser, stop_event), daemon=True)
    t_text.start()

    try:
        print("\n=== 阶段1：心跳维持 3秒，观察ESP32是否保持正常（不应打印 SAFETY_MODE_TRIGGERED） ===")
        time.sleep(3)

        print("\n=== 阶段2：发送云台指令帧 pan=30 tilt=-15 ===")
        body = (30).to_bytes(2, "little", signed=True) + (-15).to_bytes(2, "little", signed=True)
        ser.write(build_frame(FRAME_GIMBAL_CMD, body))
        time.sleep(0.5)

        print("\n=== 阶段3：发送执行器触发帧 action_type=WAND(0x01) ===")
        ser.write(build_frame(FRAME_ACTUATOR_TRIGGER, bytes([0x01])))
        time.sleep(0.5)

        print("\n=== 阶段4：停止发心跳，观察约1秒后ESP32是否打印 SAFETY_MODE_TRIGGERED ===")
        send_heartbeat_event.clear()
        time.sleep(3)

        print("\n=== 阶段5：恢复发心跳，观察ESP32是否打印 SAFETY_MODE_RECOVERED ===")
        send_heartbeat_event.set()
        time.sleep(3)

        print("\n测试结束。请对照上方[RECV]（协议帧）与[TEXT]（调试文本）日志，核对是否符合预期。")
    finally:
        stop_event.set()
        ser.close()
        usb_ser.close()


if __name__ == "__main__":
    main()
