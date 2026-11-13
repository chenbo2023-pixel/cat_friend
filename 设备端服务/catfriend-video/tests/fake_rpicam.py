#!/usr/bin/env python3
"""集成测试输入：模拟 rpicam 的分块输出、退出及无帧挂起，不打开摄像头。"""

import os
import sys
import time
import urllib.request


if os.environ.get("CATFRIEND_FAKE_MODE") == "stall":
    # 模拟持续存活但没有完整图像的进程，父服务应超时终止它。
    while True:
        time.sleep(1)

with urllib.request.urlopen(os.environ["CATFRIEND_FAKE_JPEG_URL"], timeout=3) as response:
    jpeg = response.read()

print("fake camera diagnostic", file=sys.stderr, flush=True)
for _ in range(3):
    # APP/COM 内出现帧尾字节，分帧器必须跳过元数据。
    decorated = jpeg[:2] + bytes([255, 254, 0, 6, 255, 217, 255, 216]) + jpeg[2:]
    for start in range(0, len(decorated), 137):
        sys.stdout.buffer.write(decorated[start:start + 137])
    sys.stdout.buffer.flush()
    time.sleep(0.2)
sys.exit(1)
