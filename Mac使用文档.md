# Mac 通过 WireGuard 访问云主机和树莓派

## 当前状态

- Mac 已通过 Homebrew 安装 `wireguard-tools` 和 `wireguard-go`，使用命令行版本。
- Mac 到云主机的 WireGuard 连接已验证：`ping -c 3 10.0.0.1` 收到全部回复，测试时平均往返约 7.9ms。
- Mac 尚未设置 WireGuard 开机自动启动。
- 树莓派已接入 WireGuard，已启用开机启动，Mac 通过 `10.0.0.3` 的 SSH 登录已验证。
- VS Code Remote SSH 已配置，局域网和 WireGuard 两种入口均已连接并读取 `/home/cat`。

| 设备 | WireGuard 地址 | 状态 |
|---|---|---|
| 云主机 | `10.0.0.1` | 已配置，Mac 可通过隧道到达 |
| Android 手机 | `10.0.0.2` | 已配置并验证 |
| 树莓派 | `10.0.0.3` | 已配置并验证 |
| Mac | `10.0.0.4` | 已配置并验证 |

## Mac 本机配置

以下路径适用于当前用户 `chenbo21`：

| 文件 | 用途 |
|---|---|
| `/Users/chenbo21/.config/wireguard/catfriend.conf` | 隧道配置 |
| `/Users/chenbo21/.config/wireguard/catfriend-private.key` | Mac 私钥，留在本机 |
| `/Users/chenbo21/.config/wireguard/catfriend-public.key` | Mac 公钥，云主机已登记 |

配置目录权限为 `700`，上述文件权限为 `600`。配置通过 `PostUp` 从私钥文件加载密钥；本文不记录私钥内容。

云主机入口为 `39.96.25.199:51820`，使用 UDP。只有发往 `10.0.0.0/24` 的流量进入隧道，其他流量使用原来的网络路由。

## 1. 启动隧道

在 Mac 本地终端执行：

```bash
sudo /opt/homebrew/bin/bash /opt/homebrew/bin/wg-quick up /Users/chenbo21/.config/wireguard/catfriend.conf
```

在终端输入 Mac 管理员密码；输入时不会显示字符。

同一次使用期间只需启动一次。已启动时不要重复执行 `up`。退出 SSH 或关闭终端不会停止隧道；Mac 重启后，需要重新执行启动命令。

日志中的 `utun5` 等名称由 macOS 动态分配，下次可能不同。日常启动和关闭都使用配置文件路径，不需要手动填写 `utun` 编号。

## 2. 检查连接

```bash
ping -c 3 10.0.0.1
```

```bash
sudo /opt/homebrew/bin/wg show
```

收到 ping 回复说明当前 Mac 到云主机的隧道通信正常。`wg show` 可查看 Peer、最近握手时间和流量；它不会显示私钥。

## 3. 访问云主机

### 通过公网 SSH

```bash
ssh cat_friend_server
```

本机已有 SSH 别名 `cat_friend_server`，指向公网地址 `39.96.25.199`，使用 `root` 和已有 SSH 密钥。这种访问不需要 WireGuard，已验证可用。

### 通过 WireGuard SSH

先启动隧道，然后执行：

```bash
ssh root@10.0.0.1
```

当前已验证隧道 IP 连通；此 SSH 登录命令尚未单独验收。SSH 仍需要有效密钥，WireGuard 不替代 SSH 登录认证。

如果首次连接这个地址出现 SSH 主机密钥确认提示，应与已信任的云主机主机密钥核对。

### 猫岗哨网页

当前项目记录中，云端应用监听 `127.0.0.1:8080`，经 Nginx 的 HTTPS/mTLS 入口访问。

`http://10.0.0.1:8080` 需要服务增加 WireGuard 地址监听后才能使用；不能因浏览器打不开该地址就判断隧道故障。

## 4. 访问树莓派

树莓派已接入 WireGuard，当前账号为 `cat`。远程访问需要树莓派保持供电、连接家庭网络，并运行 WireGuard 和 SSH。

### SSH

```bash
ssh cat-pi
```

本机已配置以下 SSH 入口：

| 入口 | 实际连接 | 适用场景 |
|---|---|---|
| `ssh cat-pi` | `cat@10.0.0.3` | 通过 WireGuard，在家或外出使用 |
| `ssh cat@10.0.0.3` | 同上 | 直接输入隧道 IP |
| `ssh cat-pi-lan` | `cat@192.168.31.179` | Mac 在家庭局域网中时使用 |

这两个别名使用密码认证，避免 SSH agent 自动提交多个密钥导致 `Too many authentication failures`。密码在 SSH 或 VS Code 的认证提示处输入，不保存在配置中。

配置还启用了 SSH 连接复用，空闲保留 20 分钟。复用已有连接时可能无需再次输入密码；连接退出后会重新认证。

### VS Code Remote SSH

1. 在 VS Code 按 `Command+Shift+P`。
2. 选择 `Remote-SSH: Connect to Host…`。
3. 在家选择 `cat-pi-lan`；外出选择 `cat-pi`，并确保 Mac WireGuard 已启动。
4. 在认证提示处输入树莓派 `cat` 账号密码。
5. 连接后选择“打开文件夹”，打开 `/home/cat` 或其中的实际项目目录。

编辑器界面在 Mac，文件、远程终端和程序运行在树莓派。

设备端使用现有 `cat_friend` 单仓库，代码放在 `设备端服务/`，不使用子仓库或子模块。视频代码及Mac本地验证已完成；树莓派计划克隆同一仓库至 `/home/cat/cat_friend`，视频项目目录为 `/home/cat/cat_friend/设备端服务/catfriend-video`，仓库克隆、推送认证和Pi实机验收仍待完成。目录职责和Git同步步骤见[设备端目录说明](设备端服务/README.md)，本地及Pi运行步骤见[视频服务说明](设备端服务/catfriend-video/README.md)。

在树莓派编辑后，验证并提交、推送，再在Mac检查工作区状态并用 `git pull --ff-only` 拉取；在Mac开发时反向操作。Remote SSH保存的是树莓派文件，Git不会自动同步两边工作副本，也不会自动构建或重启服务。

视频采用SSH前台手动运行：部署后在远程终端进入视频项目目录，执行 `./target/release/catfriend-video --config config/pi.toml`；用完按Ctrl-C并等待退出，再关闭SSH。运行期间持续采集，关闭手机网页不停止相机；当前不安装或启用视频自启动。

当前 VS Code 为 `1.135.0`，Server commit 为 `08d4889f9ec4a1685d257b9b95de036c8e1ce1e5`。匹配的 ARM64 Server 已在 Mac 下载，并通过局域网传输、安装到树莓派。

初次安装时曾使用本机下载和兼容启动方式，并在该设置下验证两种入口连接成功。由于用户级设置也影响 `dev` 等其他主机，现已按用户要求移除添加的 `remote.SSH.localServerDownload`、`remote.SSH.useExecServer`、`remote.SSH.showLoginTerminal` 和 `remote.SSH.remotePlatform`，恢复扩展默认行为。

恢复默认后，新启动方式请求了额外的 ARM64 Server/Agent Host 组件，版本 `1.146.0`，commit `69047191fc427f2f92828279aae1d90d8eeda7ab`。已在 Mac 下载约237MB安装包，通过 WireGuard 上传到树莓派，比对 SHA-256 一致后安装到：

```text
/home/cat/.vscode-server/cli/servers/Stable-69047191fc427f2f92828279aae1d90d8eeda7ab/server
```

旧的慢速下载已停止。默认方式的 `SSH: cat-pi` 已验证能读取 `/home/cat`，不再等待该组件下载。操作前后本机 SSH 配置和 VS Code 用户设置的校验值一致，其他主机连接设置未改动。该版本是当次默认方式请求的组件版本，不代表本机 VS Code 已升级为1.146.0。

VS Code 更新后，可能需要安装对应的新版本 Server。

### 常见问题与输入位置

- `Too many authentication failures`：可能是 SSH agent 提交了多个密钥。本机只针对树莓派别名指定密码认证；`dev` 和云主机的密钥配置保留。
- 普通 SSH 已成功、VS Code 仍停在安装：检查 `Remote-SSH: Show Log` 和远端组件日志，区别认证、下载、解压和服务启动阶段。普通 SSH 不需要 VS Code Server。
- 避免为解决单一设备问题修改用户级 Remote SSH 启动或下载设置；这些设置可能影响所有主机。
- `sudo ... wg-quick up` 与配置路径必须属于同一条命令。直接执行 `.conf` 会报权限错误，不应给它增加执行权限。
- Mac 管理员密码在 Terminal 的 `sudo Password:` 提示处输入，树莓派密码在 SSH/VS Code 认证提示处输入。Siada 对话输入框不会把消息转交给这些提示；不要发送密码。
- `cat-pi` 和 `cat-pi-lan` 是同一树莓派的两条入口。`Host` 列表已移除额外的 `10.0.0.3` 名称，直接执行 `ssh cat@10.0.0.3` 仍可使用。

### 供电与掉线排查

WireGuard 可以长期保持开启，空闲时 `PersistentKeepalive=25` 帮助维持 NAT 映射。树莓派需先主动连接云主机，云主机才能学习其公网出口地址和端口；保活不是从外部定位一台从未连接的设备。

曾出现 Mac/手机到云主机握手正常、树莓派握手停留约8小时且不可达。用户推测笔记本休眠停止 USB 供电；此原因未现场确认，树莓派随后恢复在线。长期远程使用建议独立 USB-C 电源，Pi4 使用官方5.1V/3A或可靠5V/至少3A，不能直接接12V。尚未确认实际更换电源。

掉线时先确认 Mac 到云主机、再检查云主机上的树莓派 Peer；若树莓派隧道不可达，需要家中检查供电、联网和服务。不能把网络超时当作 SSH 密码失败。

### 微雪上位机网页

微雪程序安装并运行后，在浏览器打开：

```text
http://10.0.0.3:5000
```

`5000` 是教程中的默认端口，最终以实际程序配置和监听状态为准。

远程访问不依赖 AccessPopup 热点。当前开发阶段保持树莓派连接家庭路由器，暂缓安装 AccessPopup。

## 5. 用完关闭隧道

在 Mac 本地终端执行：

```bash
sudo /opt/homebrew/bin/bash /opt/homebrew/bin/wg-quick down /Users/chenbo21/.config/wireguard/catfriend.conf
```

关闭后，通过 `10.0.0.x` 的连接会断开；公网 SSH 别名 `cat_friend_server` 仍可使用。

关闭仅清理运行中的接口及相关资源，保留配置文件和密钥。下次执行 `up` 即可重新连接，无需重新生成密钥或添加云主机 Peer。

## 待完成事项

1. 重启树莓派，验证家庭网络、WireGuard 和 SSH 自动恢复。
2. 独立供电方式确认，并验证断网恢复和长时间运行；外部网络下的 SSH 和 VS Code 已实际连接成功，手机热点场景可另行复验。
3. 摄像头识别、拍照及设备端监控代码本地验证已通过；下一步完成树莓派仓库克隆、推送认证、视频手动部署和Android显示验收；微雪安装和执行器控制暂缓。

相关资料：[技术方案设计](技术方案设计.md)、[硬件组装与树莓派配置操作指南](硬件组装与树莓派配置操作指南.md)。