# Codex 原生日志采集器

[English](README.en.md)

这是安装在 **Codex 所在用户电脑** 上的 Rust 原生采集器。它将已有及后续 Codex 日志转换为 `1flowbase.agent-logs/v1`，上传到指定应用，复用 1flowbase 的日志、对话和客户端轨迹。

客户端无需安装 Node.js、Rust 或检出任何仓库。此目录使用 `collector-manifest.json`，由 1flowbase 现有扩展目录保留签名分发包，服务器不会执行采集器。

## 安装

先在 Agent Logs 应用的 **采集 CLI** 页面将分发包安装到当前 1flowbase 节点；这只保留下载资产，不表示客户端在线。随后在 Agent Logs 应用的 **采集 CLI** 页面选择 Codex，复制对应系统的安装命令；应用的 API 页面可创建或管理 API Key。安装命令不包含密钥。执行后，安装器在本地终端提示输入 Key，保存在仅当前用户可访问的配置中；上传时仅向指定 1flowbase 端点发送 Bearer 凭据。

Linux/macOS 命令结构如下，替换 endpoint 和应用 ID：

```bash
installer="$(mktemp)"
curl -fsSL 'https://YOUR_HOST/api/public/client-collectors/taichuy/codex-logs-collector/0.1.0/assets/install.sh' -o "$installer"
bash "$installer" --endpoint 'https://YOUR_HOST/api/logs/v1/events' --installation-id 'YOUR_APPLICATION_ID' --version '0.1.0' --release-base 'https://YOUR_HOST/api/public/client-collectors/taichuy/codex-logs-collector/0.1.0/assets'
rm -f "$installer"
```

Windows 请复制页面的 PowerShell 命令。脚本参数名是 `-Endpoint`、`-InstallationId`、`-Version` 和必填 `-ReleaseBase`；使用当前用户的计划任务启动采集器。Linux 使用用户级 systemd，macOS 使用 LaunchAgent；安装后立即启动，并在此用户重启登录后恢复。

Linux 没有用户级 systemd 会话时，安装器会明确报告后台启动失败。可以在有用户会话的环境重新执行，或使用 `--no-start` 配置后，在自己的进程管理器中运行原生 `watch` 命令。无需 sudo，不修改 Codex 配置或源日志。

## 采集范围与恢复

默认读取安装时的 `CODEX_HOME`，未设置时为 `~/.codex`（Windows 为用户目录中的 `.codex`），仅采集其 `sessions` 和 `archived_sessions`，包括已有历史及后续增量。可用 `--source PATH` 选择 Codex 根目录、单独的日志目录或 rollout 文件。

采集器只上传已换行提交的 JSONL 记录。断网、HTTP 错误或不完整 ACK 不推进游标；恢复后使用相同事件 ID 重传。移动到归档目录不重新计量；修改或截断已确认源内容会拒绝继续该次采集，保留游标供诊断。没有明确轮次归属的记录等待源事实，不制造任务。

每个应用 ID 独立安装和持有 source identity。重新安装相同版本或升级会保留配置和 checkpoint；重复配置只更新 Key，不能静默修改既有 endpoint/source 绑定。要采集到另一部署或改变源绑定，请使用新的 `--installation-id`。

## 本地路径与手动操作

Linux/macOS 默认目录：`${XDG_DATA_HOME:-$HOME/.local/share}/1flowbase/collectors/codex/应用ID/`。Windows：`%LOCALAPPDATA%\1flowbase\collectors\codex\应用ID\`。目录内包含 `bin`、私有 `config.json` 和 `state.json`。不要公开配置内容。

原生可执行程序支持：

```text
codex-logs-collector --help
codex-logs-collector --version
codex-logs-collector import --config /path/to/config.json
codex-logs-collector watch --config /path/to/config.json
```

已有后台采集器占用 checkpoint 时，手动 import/watch 会拒绝并发占用。先停止所属服务后再运行；进程异常退出会由操作系统释放文件锁，无需删除 checkpoint。

卸载时重新执行同一安装脚本，Linux/macOS 使用 `--uninstall --installation-id 应用ID`，Windows 使用 `-Uninstall -InstallationId 应用ID`。卸载停止并移除本应用采集服务及 binary，保留配置、游标和 Codex 源日志。自定义过 `--install-dir/--config` 的安装需传入相同路径。

## 发布与开发

`collector-release` 工作流测试源代码，并构建 Linux musl、macOS 和 Windows 的 amd64/arm64 原生包。每次发行发布一个与服务器平台无关的签名 tar.gz，包含六个不解包的原生平台档案、公开安装器、双语 README、SHA-256 checksums、Ed25519 checksum 签名、公钥和 source SHA 元数据；根 manifest 记录每项资产的名称、摘要与大小。现有扩展目录提供外层档案的 SHA-256 和 Ed25519 签名。安装器必须使用页面提供的版本固定 1flowbase 资产基址，并从同一节点取得 binary 和 checksums，无 GitHub 回退；删除保留包后不能发起新下载。安装器验证档案 SHA-256；签名用于独立验证和发行审计，不将 checksum 校验宣称为签名验证。

共享 `sdk/agent-logs-collector` 负责读取、上传、ACK 和恢复；本插件只负责 Codex 映射与 CLI。SDK 直接依赖主仓锁定 Git revision 的 canonical Rust DTO，不复制协议定义。源测试见 SDK 与插件 `src/_tests`；安装/发行测试见 `scripts/collectors/_tests`。
