# 树莓派设备端服务

设备端代码统一保存在现有 `cat_friend` Git 仓库的本目录中，与云端服务、ESP32 固件及项目文档分文件夹管理。使用同一套提交历史，不创建子仓库或 Git 子模块。

## 当前范围与状态

已确定首版只做 Android 浏览器监控，通过已有 WireGuard 访问树莓派。网页和视频服务部署在树莓派；云主机负责已有隧道路由转发，无需新增视频代码，但视频流仍经过云主机并消耗带宽。Android 到树莓派视频端口的访问待验收。

已实现 `catfriend-video` 的 Rust/MJPEG 服务、rpicam 持续采集适配器、变化的测试输入和 Android 网页。本地代码与模拟链路已验证，树莓派真实采集、手动部署、Android 显示及持续运行仍待验收。用户已确定当前用SSH前台手动启停，暂不安装或启用视频自启动；程序运行期间持续采集，关闭网页不会停止采集。起步配置为 640×480、5fps，Mac 测试源约 5fps，不代表实机推流性能。iPhone 视频接入、ROS2、UART 和执行器不属于首版监控范围。运行步骤见[视频服务说明](catfriend-video/README.md)。

## 代码位置与职责

当前结构如下；自启动文件是未来可选模板，当前不安装：

```text
设备端服务/
├── README.md
└── catfriend-video/
    ├── Cargo.toml
    ├── Cargo.lock
    ├── src/
    │   ├── main.rs
    │   ├── config.rs
    │   ├── frame.rs
    │   ├── capture/
    │   │   ├── mod.rs
    │   │   ├── jpeg.rs
    │   │   ├── rpicam.rs
    │   │   └── test_pattern.rs
    │   ├── stream.rs
    │   ├── http.rs
    │   └── transport.rs
    ├── static/
    │   ├── index.html
    │   ├── style.css
    │   └── player.js
    ├── config/
    │   ├── mac.example.toml
    │   └── pi.example.toml
    ├── deploy/
    │   └── catfriend-video.service
    ├── tests/
    │   ├── fake_rpicam.py
    │   ├── smoke.py
    │   └── player.test.cjs
    └── README.md
```

| 位置 | 职责 | 执行位置 |
|---|---|---|
| `src/main.rs` | 加载配置、启动任务和协调退出 | Mac 或树莓派 |
| `src/config.rs` | 读取和检查输入类型、画面参数及监听地址 | Mac 或树莓派 |
| `src/frame.rs` | 定义 JPEG 帧序号、采集时间、宽高和数据 | Mac 或树莓派 |
| `src/capture/` | 定义采集接口，适配 rpicam 和变化的测试画面 | rpicam 在树莓派；测试输入可在 Mac 验证 |
| `src/stream.rs` | 共享一次采集、保留最新帧、有界分发 | Mac 或树莓派 |
| `src/http.rs` | 提供网页、MJPEG 流、截图和采集状态接口 | Mac 或树莓派 |
| `src/transport.rs` | HTTP 连接、TCP 写入超时和有界退出 | Mac 或树莓派 |
| `static/` | 页面、样式和播放重连逻辑，使用同源相对接口地址 | 服务提供文件，Android 浏览器执行网页代码 |
| `config/` | 可复制的配置示例，IP、端口和采集参数不硬编码 | 运行程序的设备 |
| `deploy/` | 未来可选的后台运行和开机启动模板，当前不安装 | 树莓派 |

网页源文件在 `static/`，编译时嵌入程序，修改后需重新构建；部署只需可执行文件及 TOML 配置。

未来 UART 桥接和 ROS2 节点按功能在 `设备端服务/` 下建立同级目录，具体名称和实现待对应阶段确定。

## Mac、树莓派和 Git 同步

Mac 当前仓库为 `/Users/chenbo21/Desktop/cat_friend`。树莓派建议克隆同一个仓库至 `/home/cat/cat_friend`；克隆和远端推送认证尚未配置或验收。

通过 VS Code Remote SSH 连接 `cat-pi` 后，打开 `/home/cat/cat_friend/设备端服务/catfriend-video` 即可直接编辑、编译和运行设备端代码。编辑器界面在 Mac，文件和远程终端在树莓派。

Git 不会自动同步保存的文件。推荐单次改动在一个工作副本中完成：

1. 在开发设备上检查修改、验证并提交，只提交本次相关文件。
2. 推送到现有项目远端仓库。
3. 另一台设备确认工作区状态后执行 `git pull --ff-only`。

有未提交修改或分支分叉时，先保存和处理改动，不强制覆盖工作区。树莓派运行环境可能包含相机权限或其他设备配置，需要按实际情况单独准备；Git 拉取也不等于程序已构建、部署或重启。

编译产物、运行日志、实际设备配置和私钥等不进入源码提交；已加入 `target/`、实际 `config/mac.toml`/`pi.toml` 及项目日志的忽略规则。部署模板从源码目录的 `target/release/` 启动，实际路径待部署核对。

## 微雪环境复用

采集适配器与帧分发、HTTP 服务和网页分开。未来可以继续使用 rpicam，或在核实官方视频接口后增加官方输出适配器。同一颗 CSI 摄像头由一个采集程序持有，再向其他功能分发画面。

自研目录与 `/home/cat/ugv_rpi` 分开；官方依赖安装可能改变共享系统和相机栈，目录隔离不能代替安装前后复验。JPEG 帧接口不直接用于 H.264/WebRTC，后续媒体格式需另行扩展。

完整方案见[技术方案设计第19节](../技术方案设计.md)，实施顺序见[周计划](../周计划.md)。