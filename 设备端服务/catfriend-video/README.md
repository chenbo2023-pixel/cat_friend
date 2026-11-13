# Android 网页视频监控

独立的树莓派视频服务，使用 Rust/tokio/axum 提供网页和 MJPEG。Android 打开已有 WireGuard，再用浏览器访问树莓派。云主机只参与已有网络转发；本服务不连接云端 `catfriend-web`，不操作 UART、GPIO 或舵机。

## 实现与验收状态

- 已实现：变化的测试输入、rpicam 持续采集适配器、JPEG 增量分帧、最新帧共享、观看人数限制、采集故障退避恢复、网络写入超时、截图、采集状态和 Android 网页。
- 已本地验证：Rust 单元测试、真实 HTTP 流与截图、多个观众和名额回收、模拟 rpicam 输出与退出恢复、无帧超时、SIGTERM 停止，以及网页逻辑模拟测试。
- 尚未验收：Pi4 上实际连续采集、Android 浏览器实际显示和网络切换、树莓派手动部署与启停、至少一小时连续运行、真实带宽及端到端延迟。已只读确认Pi的rpicam支持所需参数，WireGuard运行且8090端口空闲；尚未部署。

用户已确定当前通过 SSH 前台手动启动，使用结束按 Ctrl-C 停止。程序运行期间持续采集，即使没有观众也不会暂停摄像头；退出后释放摄像头并关闭观看连接。视频自启动暂不安装或启用，保留模板供未来另行决定。

本地测试使用 640×480、5fps 配置，短时状态统计约 5fps；这是 Mac 测试源结果，不代表 OV5647 的推流性能或 Android 显示帧率。

## 文件与执行位置

| 文件 | 职责 | 执行位置 |
|---|---|---|
| `src/main.rs` | 配置加载、启动和退出协调 | Mac 或树莓派 |
| `src/config.rs` | TOML 解析、地址和资源范围检查 | Mac 或树莓派 |
| `src/frame.rs` | JPEG 数据、序号、服务收到帧的时间 | Mac 或树莓派 |
| `src/capture/mod.rs` | 选择采集器，失败后按 1/2/5/10 秒退避重试 | Mac 或树莓派 |
| `src/capture/test_pattern.rs` | 移动色条与二进制序号测试图，JPEG 编码 | Mac 或树莓派 |
| `src/capture/rpicam.rs` | 运行一个 `rpicam-vid`，读取 stdout，管理超时与子进程 | 树莓派；本地可用模拟进程验证 |
| `src/capture/jpeg.rs` | 处理任意读取块边界、JPEG 标记段和缓冲上限 | Mac 或树莓派 |
| `src/stream.rs` | `watch` 共享最新帧、帧龄、源帧率和观看名额 | Mac 或树莓派 |
| `src/http.rs` | 网页、MJPEG、截图和状态路由 | Mac 或树莓派 |
| `src/transport.rs` | TCP 写入无进展超时、HTTP 连接管理、有界退出 | Mac 或树莓派 |
| `static/index.html`、`style.css`、`player.js` | 页面、画面连接、状态轮询、重连和页面生命周期 | Android 浏览器；本地可模拟逻辑 |
| `config/*.example.toml` | Mac 测试和 Pi 相机配置示例 | 运行设备读取 |
| `deploy/catfriend-video.service` | 未来可选的 systemd 模板，目前不安装或启用 | 树莓派 |
| `tests/` | HTTP 与模拟相机测试、网页逻辑测试 | 开发设备 |

网页源文件保存在 `static/`，通过 `include_str!` 编译进可执行文件。部署时只需要程序与 TOML 配置，不依赖当前目录寻找静态文件；修改网页后也需要重新构建程序。

## Mac 本地运行

进入项目目录：

```bash
cd /Users/chenbo21/Desktop/cat_friend/设备端服务/catfriend-video
cargo +stable run --locked -- --config config/mac.example.toml
```

浏览器打开 `http://127.0.0.1:8090/`。应看到移动色条，底部色块随帧序号变化，页面标注“测试画面”。按 Ctrl-C 停止。

本次验证工具链为 Rust/Cargo 1.87.0 stable。使用可满足 `Cargo.lock` 依赖的 Rust 工具链；Bookworm 自带的旧 Rust 包不能直接视为满足要求。`Cargo.lock` 随源码管理，`target/` 不提交。

## 树莓派手动运行（待执行）

建议源码路径为 `/home/cat/cat_friend/设备端服务/catfriend-video`，通过 VS Code Remote SSH 打开该目录。仓库克隆、推送认证与工具链准备是独立步骤，尚未验收。

先检查摄像头和工具：

```bash
rpicam-hello --list-cameras
command -v rpicam-vid
rpicam-vid --help
```

确认摄像头未被官方应用或拍照程序占用。检查版本是否支持 MJPEG、quality 及示例配置中的参数。树莓派需保持供电和家庭联网，WireGuard 地址 `10.0.0.3` 应已就绪。

核对配置并手动构建、运行：

```bash
cd /home/cat/cat_friend/设备端服务/catfriend-video
cp config/pi.example.toml config/pi.toml
cargo +stable build --release --locked -j 1
./target/release/catfriend-video --config config/pi.toml
```

`pi.toml` 是忽略提交的实际设备配置，可调整相机编号、工具路径和画面参数。`-j 1` 限制并行编译，降低 Pi4 构建期的内存压力。

部署完成后，日常使用只需通过 SSH 前台运行已编译程序：

```bash
ssh cat-pi
cd /home/cat/cat_friend/设备端服务/catfriend-video
./target/release/catfriend-video --config config/pi.toml
```

使用期间保留这个 SSH 终端。结束时按 Ctrl-C，等待程序退出、终端回到提示符，再执行 `exit` 退出 SSH。程序处理 SIGINT、SIGTERM 和 SIGHUP，退出时停止采集子进程；本地模拟测试覆盖终端挂断信号。网络断开何时触发远端终端挂断取决于 SSH 对断连的检测，日常关闭使用 Ctrl-C 明确停止。

Android 启用已有 WireGuard 后，浏览器打开 `http://10.0.0.3:8090/`。HTTP 在 WireGuard 隧道内传输；本服务没有账号或证书鉴权，默认 Pi 配置只绑定 WireGuard IP，不绑定所有网卡。另行绑定家庭私网 IP 会把访问范围扩大到该网络，应按实际用途配置。

## 配置与接口

| 配置 | 含义 |
|---|---|
| `server.listen` | 本机或私网监听地址；拒绝公网及 `0.0.0.0`/`::` |
| `server.max_viewers` | 最大 MJPEG 连接数，默认 4，允许 1～16 |
| `server.write_timeout_ms` | TCP 写入没有进展的超时，默认 10 秒 |
| `capture.source` | `test_pattern` 或 `rpicam` |
| `capture.binary`、`camera` | rpicam 可执行文件路径和相机编号 |
| `capture.width`、`height`、`fps`、`quality` | 画面参数，起步 640×480、5fps、JPEG 质量 75 |
| `capture.max_frame_bytes` | 单帧解析缓冲上限，默认 4MiB |
| `capture.stale_after_ms` | 超过此帧龄视为源不健康，默认 5 秒 |
| `capture.frame_timeout_ms` | 没有完整帧时重启采集，默认 15 秒 |

| HTTP 接口 | 返回 |
|---|---|
| `/` | 监控网页 |
| `/stream.mjpg` | `multipart/x-mixed-replace` 连续 JPEG；超额连接返回 429 |
| `/snapshot.jpg` | 最新的新鲜 JPEG；无画面或过期返回 503 |
| `/api/status` | 源状态、健康、帧序号/帧龄、尺寸、配置帧率、近期源帧率、观看人数和最近错误 |

源状态为 `starting`、`streaming`、`reconnecting`、`stopped`。故障时清空最新帧，防止新请求把故障前画面当作实时图像。手机断线后释放连接；半失联且停止读取时，TCP 写入超时最终回收名额。

网页每两秒轮询状态，画面错误按 1/2/5/10 秒重连。页面隐藏时停止流和轮询，返回时重新连接。`img.onload` 不用作逐帧时钟；页面“源正在出帧”表示设备采集健康，并不证明浏览器持续渲染。画面连接单独冻结而状态接口正常时，可以点击“重新连接”；该场景还需 Android 实测。

帧时间是服务接收到完整 JPEG 时记录的本机时间，不是传感器曝光时刻。源帧率按最近约五秒的收到帧时间统计。端到端延迟需用画面中的外部时钟等方法测量，不能从这些状态字段或网络 RTT 推导。

## 验证命令

在项目目录执行：

```bash
cargo +stable fmt -- --check
cargo +stable test --locked
cargo +stable clippy --locked --all-targets -- -D warnings
cargo +stable build --locked
chmod +x tests/fake_rpicam.py
python3 tests/smoke.py
node --check static/player.js
node --test tests/player.test.cjs
```

HTTP 测试创建临时配置、启动回环服务，结束后清理；模拟相机只读取测试截图，不打开真实相机。Python 与 Node 测试仅依赖标准库。没有安装 Node 的树莓派可在 Mac 完成网页逻辑测试。

## 手动运行与未来微雪环境

`deploy/catfriend-video.service` 只是未来可选模板。用户已决定当前不安装、启用或自动启动视频服务，日常使用 SSH 手动启停。当前验收包括重复手动启停、Ctrl-C 后释放相机和端口、Android观看及持续运行；视频开机启动不属于本轮验收，已有WireGuard/SSH开机恢复分别记录。

本服务与 `/home/cat/ugv_rpi` 分目录，采集器与视频分发独立。未来可继续使用 rpicam，或在核实官方输出后新增采集适配器。同一颗 CSI 摄像头只能由一个采集程序持有。微雪脚本可能改变共享系统、相机依赖和启动任务，需在安装前后复验。

JPEG 帧接口不直接兼容 H.264/WebRTC；后续升级应新增对应媒体接口，复用配置、生命周期和页面结构。