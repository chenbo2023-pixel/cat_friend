#!/usr/bin/env python3
"""只依赖 Python 标准库，在回环地址验证 HTTP 与采集进程恢复。"""

import argparse
import contextlib
import hashlib
import http.client
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


PROJECT = Path(__file__).resolve().parents[1]


def unused_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def eventually(check, timeout=8):
    deadline = time.monotonic() + timeout
    last_error = None
    while time.monotonic() < deadline:
        try:
            result = check()
            if result:
                return result
        except (OSError, urllib.error.URLError, http.client.HTTPException) as error:
            last_error = error
        time.sleep(0.05)
    raise AssertionError(f"等待验证结果超时：{last_error}")


def get(base, path):
    with urllib.request.urlopen(base + path, timeout=3) as response:
        return response.status, response.headers, response.read()


def status(base):
    return json.loads(get(base, "/api/status")[2])


@contextlib.contextmanager
def server(binary, source="test_pattern", extra="", env=None):
    port = unused_port()
    base = f"http://127.0.0.1:{port}"
    with tempfile.TemporaryDirectory(prefix="catfriend-smoke-") as directory:
        config = Path(directory) / "test.toml"
        config.write_text(
            f'[server]\nlisten = "127.0.0.1:{port}"\nmax_viewers = 2\n'
            f'[capture]\nsource = "{source}"\nwidth = 640\nheight = 480\n'
            f'fps = 5\nstale_after_ms = 1000\nframe_timeout_ms = 1200\n{extra}',
            encoding="utf-8",
        )
        with (Path(directory) / "service.log").open("wb") as log:
            child = subprocess.Popen(
                [str(binary), "--config", str(config)],
                cwd=directory, stdout=log, stderr=subprocess.STDOUT,
                env={**os.environ, **(env or {})},
            )
            try:
                eventually(lambda: status(base))
                yield base, child
            finally:
                if child.poll() is None:
                    child.terminate()
                    try:
                        child.wait(timeout=8)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=3)
                        raise AssertionError("SIGTERM 后服务未在 8 秒内退出")
                if child.returncode != 0:
                    print((Path(directory) / "service.log").read_text(), flush=True)
                    raise AssertionError(f"视频服务异常退出：{child.returncode}")


def open_stream(base):
    port = int(base.rsplit(":", 1)[1])
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=4)
    connection.request("GET", "/stream.mjpg?connection=smoke")
    response = connection.getresponse()
    assert response.status == 200
    assert "boundary=catfriend-frame" in response.getheader("Content-Type")
    return connection, response


def frame(response):
    line = response.readline()
    while line == b"\r\n":
        line = response.readline()
    assert line == b"--catfriend-frame\r\n", line
    headers = {}
    while True:
        line = response.readline()
        if line == b"\r\n":
            break
        assert line, "multipart 帧头意外结束"
        key, value = line.decode("ascii").split(":", 1)
        headers[key.lower()] = value.strip()
    jpeg = response.read(int(headers["content-length"]))
    assert jpeg[:2] == b"\xff\xd8" and jpeg[-2:] == b"\xff\xd9"
    assert response.read(2) == b"\r\n"
    return int(headers["x-frame-sequence"]), hashlib.sha256(jpeg).hexdigest(), len(jpeg)


def run(binary):
    with server(binary) as (base, child):
        eventually(lambda: status(base)["healthy"])
        for path, content_type in [("/", "text/html"), ("/style.css", "text/css"), ("/player.js", "text/javascript")]:
            code, headers, data = get(base, path)
            assert code == 200 and headers["Content-Type"].startswith(content_type)
            assert "no-store" in headers["Cache-Control"] and data
        first_snapshot = get(base, "/snapshot.jpg")
        assert first_snapshot[1]["Content-Type"] == "image/jpeg"
        sequence = int(first_snapshot[1]["X-Frame-Sequence"])
        eventually(lambda: status(base)["frame_sequence"] > sequence)
        assert first_snapshot[2] != get(base, "/snapshot.jpg")[2]

        connections = []
        try:
            for _ in range(2):
                connection, response = open_stream(base)
                connections.append((connection, response))
            one = frame(connections[0][1])
            two = frame(connections[0][1])
            assert two[0] > one[0] and two[1] != one[1]
            assert frame(connections[1][1])[2] > 0
            assert status(base)["viewers"] == 2
            try:
                get(base, "/stream.mjpg")
                raise AssertionError("超额观看连接未被拒绝")
            except urllib.error.HTTPError as error:
                assert error.code == 429
            connections[0][1].close()
            connections[0][0].close()
            eventually(lambda: status(base)["viewers"] == 1)
            connection, response = open_stream(base)
            connections.append((connection, response))
            assert frame(response)[0] >= two[0]
        finally:
            for connection, response in connections:
                response.close()
                connection.close()
        eventually(lambda: status(base)["viewers"] == 0)
        measured = eventually(lambda: status(base) if status(base)["measured_fps"] > 0 else None)
        print(f"HTTP/截图/多观众/名额回收通过；实测源帧率 {measured['measured_fps']:.2f} fps")

        # 源仍健康时，验证 rpicam 分块输出、进程退出、重试和旧图清理。
        fixture = PROJECT / "tests/fake_rpicam.py"
        assert os.access(fixture, os.X_OK), "请先 chmod +x tests/fake_rpicam.py"
        extra = f"binary = {json.dumps(str(fixture), ensure_ascii=False)}\n"
        with server(binary, "rpicam", extra, {"CATFRIEND_FAKE_JPEG_URL": base + "/snapshot.jpg"}) as (camera_base, _):
            first = eventually(lambda: status(camera_base) if status(camera_base)["healthy"] else None)
            connection, response = open_stream(camera_base)
            try:
                assert frame(response)[2] > 0
            finally:
                response.close()
                connection.close()
            recovering = eventually(lambda: status(camera_base) if status(camera_base)["state"] == "reconnecting" else None)
            assert not recovering["healthy"] and recovering["frame_sequence"] is None
            assert "fake camera diagnostic" in recovering["last_error"]
            try:
                get(camera_base, "/snapshot.jpg")
                raise AssertionError("采集故障仍提供旧截图")
            except urllib.error.HTTPError as error:
                assert error.code == 503
            eventually(lambda: status(camera_base)["healthy"] and status(camera_base)["frame_sequence"] > first["frame_sequence"])
        print("模拟 rpicam 分块输出、退出重连、故障清除旧图通过")

        with server(binary, "rpicam", extra, {"CATFRIEND_FAKE_MODE": "stall"}) as (camera_base, _):
            stalled = eventually(lambda: status(camera_base) if status(camera_base)["state"] == "reconnecting" else None)
            assert "采集超时" in stalled["last_error"] and not stalled["healthy"]
        print("模拟相机无帧超时和 SIGTERM 清理通过")

        # 带一个仍打开的流停止主服务，验证长响应不会阻塞正常退出。
        connection, response = open_stream(base)
        try:
            frame(response)
            child.terminate()
            child.wait(timeout=8)
            assert child.returncode == 0
        finally:
            response.close()
            connection.close()
        print("带活动观看连接的服务停止通过")

    with server(binary, "rpicam", extra, {"CATFRIEND_FAKE_MODE": "stall"}) as (_, child):
        child.send_signal(signal.SIGHUP)
        child.wait(timeout=8)
        assert child.returncode == 0
    print("SSH 终端挂断信号 SIGHUP 清理通过")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, default=PROJECT / "target/debug/catfriend-video")
    args = parser.parse_args()
    run(args.binary.resolve())
