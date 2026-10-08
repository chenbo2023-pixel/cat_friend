#include <ESP32Servo.h>

Servo myServo;
int pin = 13;

void setup() {
  Serial.begin(115200);
  Serial.println("setup start");

  ESP32PWM::allocateTimer(0);
  myServo.setPeriodHertz(50);       // SG90 标准PWM频率50Hz
  myServo.attach(pin, 500, 2400);   // 显式指定脉宽范围(微秒)，SG90常见范围500~2400us

  Serial.println("setup done");
}

void loop() {
  Serial.println("angle 0");
  myServo.write(0);
  delay(1000);
  Serial.println("angle 90");
  myServo.write(90);
  delay(1000);
  Serial.println("angle 180");
  myServo.write(180);
  delay(1000);
}
