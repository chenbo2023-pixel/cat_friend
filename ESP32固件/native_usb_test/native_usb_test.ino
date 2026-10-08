// 猫岗哨 —— Native USB口（板上标"USB"的口）独立验证
// 目的：只验证USB口本身能不能发东西出来、电脑能不能收到，跟协议测试互不相关。
// 用法：本sketch仍从COM口烧录（不影响），烧录后拔掉/忽略COM口，
//       只关注USB口对应的Mac设备文件（如 /dev/cu.usbmodem1201）能否读到内容。

#include <Arduino.h>

void setup() {
  USBSerial.begin(115200);
}

void loop() {
  USBSerial.println("hello from native USB");
  delay(1000);
}
