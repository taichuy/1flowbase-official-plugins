# Codex 原生日志采集器

[English](README.en.md)

这是安装在 **Codex 所在用户电脑** 上的 Rust 原生采集器。它将已有及后续 Codex 日志转换为 `1flowbase.agent-logs/v1`，上传到指定应用，复用 1flowbase 的日志、对话和客户端轨迹。

客户端无需安装 Node.js、Rust 或检出任何仓库。此目录使用 `collector-manifest.json`，由 1flowbase 现有扩展目录保留签名分发包，服务器不会执行采集器。

## 安装

先在 Agent Logs 应用的 **采集 CLI** 页面将分发包安装到当前 1flowbase 节点；这只保留下载资产，不表示客户端在线。随后选择 Codex，可以粘贴应用 Key 或快速生成 Key，再使用“复制命令”；真实 Key 通过 `FLOWBASE_AGENT_LOGS_API_KEY` 环境变量交给安装器，页面预览会遮住 Key。未填写时，安装器在本地终端提示输入。Key 保存在仅当前用户可访问的配置中；上传时仅向指定 1flowbase 端点发送 Bearer 凭据。应用 API 页面可管理或撤销 Key。

Linux/macOS 命令结构如下，替换 endpoint 和应用 ID：

```bash
installer="$(mktemp)"
curl -fsSL 'https://YOUR_HOST/api/public/client-collectors/taichuy/codex-logs-collector/0.2.0/assets/install.sh' -o "$installer"
bash "$installer" --endpoint 'https://YOUR_HOST/api/logs/v1/events' --installation-id 'YOUR_APPLICATION_ID' --version '0.2.0' --release-base 'https://YOUR_HOST/api/public/client-collectors/taichuy/codex-logs-collector/0.2.0/assets'
rm -f "$installer"
```

Windows 请复制页面的 PowerShell 命令。脚本参数名是 `-Endpoint`、`-InstallationId`、`-Version` 和必填 `-ReleaseBase`；使用当前用户的计划任务启动采集器。Linux 使用用户级 systemd，macOS 使用 LaunchAgent；安装后立即启动，并在此用户重启登录后恢复。

Linux 没有用户级 systemd 会话时，安装器会明确报告后台启动失败。可以在有用户会话的环境重新执行，或使用 `--no-start` 配置后，在自己的进程管理器中运行原生 `watch` 命令。无需 sudo，不修改 Codex 配置或源日志。

## 采集范围与恢复

默认读取安装时的 `CODEX_HOME`，未设置时为 `~/.codex`（Windows 为用户目录中的 `.codex`），仅采集其 `sessions` 和 `archived_sessions`，包括已有历史及后续增量。可用 `--source PATH` 选择 Codex 根目录、单独的日志目录或 rollout 文件。

采集器只上传已换行提交的 JSONL 记录。断网、HTTP 错误或不完整 ACK 不推进游标；恢复后使用相同事件 ID 重传。移动到归档目录不重新计量；修改或截断已确认源内容会拒绝继续该次采集，保留游标供诊断。没有明确轮次归属的记录等待源事实，不制造任务。

每个应用 ID 独立安装和持有 source identity。重新安装相同版本或升级会保留配置和 checkpoint；重复配置只更新 Key，不能静默修改既有 endpoint/source 绑定。要采集到另一部署或改变源绑定，请使用新的 `--installation-id`。

## 历史导入、组批与升级

0.2.0 使用共享 SDK 的有状态文件追踪。断点将已确认的来源字节位置、该位置的最小解析上下文及必要的来源归属声明一起原子保存。每次收集仍验证已确认历史前缀，但已保存上下文的历史不再重复做 JSON 解析或格式转换；随后转换新增完整记录。严格前缀验证仍需读取历史字节，不能将其 I/O 成本宣称为只与新增数据量有关。

上传按完整序列化请求的字节数组批，默认目标为 **1 MiB（1048576 字节）**，包含 `raw`、正文、JSON 转义、元数据与分隔符。这是可配置的传输目标，不限制记录总量；单条事件超过目标时独立发送，保留完整正文。多个文件按批轮询，保持各自来源顺序；任何时刻只有一批等待服务端持久确认，确认前不推进断点。

手动运行时可以调整组批目标，例如：

```text
codex-logs-collector import --config /path/to/config.json --batch-bytes 262144
codex-logs-collector watch --config /path/to/config.json --batch-bytes 262144
```

`--batch-bytes` 必须为正整数，不改变配置、来源身份或事件身份；后台服务未指定时使用默认目标。等待明确轮次归属的事实通过来源字节区间与最小上下文重放，不在内存中长期积累完整事件；其扫描游标单独保存，不推进服务端已确认游标。只有来源明确提供的轮次能声明归属；工具、模型、供应商和原始事实仍取自各自记录当时的上下文。重放依赖原始日志保持可读，采集器不会删除它们。

从 0.1.0 升级时，在平台安装 0.2.0 分发包，再执行该版本的本地安装命令。保留原来的安装 ID、配置和 `state.json`；首次读取旧文件断点时先按旧摘要规则验证历史，再恢复解析上下文并迁移，保留 `source_id`、已确认位置和稳定事件 ID。暂时缺失的旧文件保留断点，返回后再验证迁移，不阻塞其他文件采集。旧版可执行程序不能读取新版断点；不要混用版本或删除断点来降级。既有 0.1.0 发行保持不可变，平台已保留的旧包仍按其原版本分发。

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

`collector-release` 工作流测试源代码，并构建 Linux musl、macOS 和 Windows 的 amd64/arm64 原生包。每次发行发布一个与服务器平台无关的签名 tar.gz，包含六个不解包的原生平台档案、公开安装器、双语 README、SHA-256 checksums、Ed25519 checksum 签名、公钥和 source SHA 元数据；根 manifest 记录每项资产的名称、摘要与大小。现有扩展目录提供外层档案的 SHA-256 和 Ed25519 签名。安装器必须使用页面提供的版本固定 1flowbase 资产基址，并从同一节点取得 binary 和 checksums，无 GitHub 回退；删除保留包后不能发起新下载。安装器验证档案 SHA-256；下载地址必须直接返回资产，安装器拒绝 HTTP 重定向。签名用于独立验证和发行审计，不将 checksum 校验宣称为签名验证。

共享 `sdk/agent-logs-collector` 负责文件身份索引、前缀验证、上下文断点、来源区间重放、字节组批、轮询、上传、ACK 和恢复；本插件只负责 Codex 映射、最小可序列化上下文与 CLI。SDK 直接依赖主仓锁定 Git revision 的 canonical Rust DTO，不复制协议定义。架构参考 [Vector file source](https://github.com/vectordotdev/vector/tree/master/lib/file-source) 和 [Fluent Bit tail](https://github.com/fluent/fluent-bit/tree/master/plugins/in_tail)，使用现有 Rust 标准库和依赖实现，不嵌入这两个完整采集器。源测试见 SDK 与插件 `src/_tests`；安装/发行测试见 `scripts/collectors/_tests`。
