# Antigravity 原生日志采集器

[English](README.en.md)

这是安装在 **Antigravity 所在用户电脑** 上的 Rust 原生采集器。它读取 Antigravity App / IDE / CLI 落盘的追加式 JSONL 会话日志，转换为 `1flowbase.agent-logs/v1` 并上传到指定应用，复用 1flowbase 的日志、对话和客户端轨迹。

客户端无需安装 Node.js、Rust 或 `agy` CLI。传输、增量字节检查点、ACK 和断网重试均由共享 `agent-logs-collector` SDK 负责，不安装常驻守护进程，也不自动导入历史。

## 安装与卸载

先在 1flowbase Agent Logs 应用的 **采集 CLI** 页面将分发包安装到当前节点，随后选择 **Antigravity**，准备应用 Key 并复制对应平台的 Shell / PowerShell 安装命令执行即可。

```sh
antigravity-logs-collector configure --endpoint https://HOST/api/logs/v1/events --config /PRIVATE/antigravity-logs/config.json --source /HOME/.gemini --key-stdin
antigravity-logs-collector install-hooks --config /PRIVATE/antigravity-logs/config.json --client-home /HOME/.gemini
antigravity-logs-collector flush --config /PRIVATE/antigravity-logs/config.json
```

默认客户端目录为 `GEMINI_HOME` 或 `~/.gemini`。`install-hooks` 会在 `~/.gemini/config/plugins/1flowbase-agent-logs/` 创建独立的 Antigravity 插件目录（含 `plugin.json` 与 `hooks.json`），**不会修改 `~/.gemini/config/config.json` 或用户全局 `~/.gemini/config/hooks.json`**。重启或下一轮对话后可在 Antigravity 的 **Customizations > Installed** 中看到并随时开关该插件。多应用安装共享同一插件目录，按配置哈希隔离各自的 Hook 条目，卸载仅移除当前配置的条目并在无残留时清理该插件目录。

## 采集与防阻塞机制

Antigravity 生命周期 Hook 为同步执行。本采集器注册 `SessionStart`、`PostInvocation` 和 `Stop` 钩子并直接调用本机原生可执行文件（Windows 无需 PowerShell 包装）：

1. 收到 Hook JSON 后，先从 `transcriptPath`（或 `artifactDirectoryPath`）定位当前会话日志，优先选择完整未截断的 `transcript_full.jsonl`（缺失时回退 `transcript.jsonl`，单会话严格单选防重复）；
2. 将目标路径原子持久化到私有 `.pending` 队列；
3. 派生无窗口分离式短生命周期 `flush` 子进程在后台完成上传并退出（`Stop` 默认等待 250 ms 落盘缓冲），Hook 进程立即向 stdout 输出 `{}` 返回，不阻塞 Antigravity 主循环；
4. 网络失败不会推进未 ACK 检查点，下次 Hook 或手动执行 `flush` 自动补传。历史回填需显式执行 `import`。

## 协议映射

- 默认覆盖 `~/.gemini/antigravity/brain`、`~/.gemini/antigravity-ide/brain` 与 `~/.gemini/antigravity-cli/brain` 三端会话，不扫描 `powertools-telemetry.jsonl` 或 `scratch/*.jsonl`。
- `USER_INPUT` 开启新轮次并提取 `<USER_REQUEST>` 正文；`PLANNER_RESPONSE` 分别映射思考（`reasoning`）、回复/最终回答（`final_answer` + `task_end`）、工具调用（`tool_call`）和用量（`usage`）。
- Antigravity 原始 `input_tokens` 不含缓存命中，采集器按 `1flowbase.agent-logs/v1` 规范换算 `input_tokens = raw_input + cache_read_tokens`、`input_cache_hit_tokens = cache_read_tokens`、`cache_write_tokens = 0`。
- 子 Agent 会话通过父会话 `invoke_subagent` 工具结果中的 `conversationId` 自动关联 `parent_source_task_id`，并在无 `USER_INPUT` 时分配确定性根任务 ID。
