<p align="center">
  <img src="assets/logo.svg" width="96" height="96" alt="BiliPick">
</p>

<h1 align="center">哔取 BiliPick</h1>

<p align="center">轻量的 Windows 哔哩哔哩视频下载工具，支持投稿视频、合集、番剧与影视。</p>

<p align="center">
  <a href="#features">功能</a> ·
  <a href="#usage">使用</a> ·
  <a href="#account">登录与设置</a> ·
  <a href="#build">构建</a>
</p>
<a id="features"></a>

## 功能

| 功能 | 说明 |
| --- | --- |
| 原生界面 | Rust + Win32，面向 Windows 10/11 x64，无需浏览器内核或脚本运行环境。 |
| 链接解析 | 支持 BV 号、分 P、合集，以及番剧和影视的单集、整季、详情页链接，可批量选择下载。 |
| 下载管理 | 选择画质，查看进度和速度，取消、断点续传、重试失败项。 |
| 媒体处理 | 使用 ffmpeg 合并音视频，并用 ffprobe 校验轨道和时长。 |
| 可选登录 | 支持主动扫码或按需提示登录，可加密保存登录记录。 |
| 文件与设置 | 同名文件避让、下载清单，保存目录和登录偏好自动保留。 |

<a id="usage"></a>

## 使用

运行 `bilipick.exe`，在界面中指定包含 `ffmpeg.exe` 和 `ffprobe.exe` 的工具目录。也可以设置环境变量 `BILIPICK_FFMPEG` 或将工具加入系统 `PATH`，查找顺序为：界面指定目录 → 环境变量 → `PATH`。

1. 粘贴 BV 号或链接，点击“解析链接”。
2. 选择下载项目、画质和保存目录。
3. 点击“开始下载”，完成后可直接打开保存目录。

番剧与影视支持 `ep` 单集、`ss` 整季和 `md` 详情页链接，也可直接输入对应编号，例如 `ep232465`、`ss24588`。整季解析后可多选剧集；尚未上线的条目会跳过，试看片段不会作为完整视频保存。可下载内容和画质取决于平台返回及账号权限。

<a id="account"></a>

## 登录与设置

登录不是使用前提。可以点击右上角“扫码登录”，也可以在下载提示需要登录时再扫码。登录成功会继续等待登录的任务，没有暂停任务时不会自动开始下载。

“记住登录”只在主界面设置，扫码窗口显示本次登录的保存方式：

- 默认关闭；开启后，成功登录的会话会加密保存，下次启动时验证恢复。
- 关闭后删除保存的会话，当前登录保留到软件退出。
- “退出登录”清除当前和已保存的会话，保留保存目录与登录偏好。
- 会话过期时需要重新扫码；网络错误时保留记录并提示恢复失败。

程序数据位于 `%LOCALAPPDATA%\BiliPick\`：

| 文件 | 内容 |
| --- | --- |
| `settings.json` | 保存目录和“记住登录”偏好，不含账号凭据。 |
| `session.dat` | 经 Windows DPAPI 加密的会话 Cookie，绑定当前 Windows 用户。 |

程序不保存账号密码，也不从浏览器提取登录信息。平台决定会话有效期，当前版本不自动刷新 Cookie。

<a id="build"></a>

## 从源码构建

需要 Rust stable、Visual Studio 2022 C++ x64 工具和 Windows 10/11 SDK。

```powershell
rustup target add x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc
```

生成文件：

```text
target\x86_64-pc-windows-msvc\release\bilipick.exe
```

## 反馈与贡献

欢迎通过 Issue 或 Pull Request 提出问题和改进。反馈解析或下载问题时，请提供链接类型、错误提示及系统版本，并去除个人凭据。

提交代码前可运行：

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## 使用范围与许可

仅下载自己有权访问并允许保存的内容，不绕过大会员、充电、付费或地区限制。暂不支持 DRM、直播和后台订阅。下载默认串行进行，同一输出目录请勿由多个进程同时处理；链接与画质的可用性以平台返回为准。

本项目使用 [MIT License](LICENSE)。ffmpeg、ffprobe 等外部工具遵循各自的许可证。

