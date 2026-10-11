# Claude Code 日志采集器

0.1.0 通过 Claude Code 原生 command hook 运行，传输、增量检查点、ACK 和重试由共享 agent-logs SDK 负责。每次回调完成即退出，不安装驻留进程，不自动导入历史。

```sh
claude-code-logs-collector configure --endpoint https://HOST/api/logs/v1/events --config /PRIVATE/claude-logs/config.json --source /HOME/.claude --key-stdin
claude-code-logs-collector install-hooks --config /PRIVATE/claude-logs/config.json --client-home /HOME/.claude
claude-code-logs-collector flush --config /PRIVATE/claude-logs/config.json
```

密钥从 stdin 输入；默认客户端目录为 `CLAUDE_CONFIG_DIR` 或 `~/.claude`。支持 configure/import/flush/install-hooks/uninstall-hooks/hook；hook 从 stdin 接收 Claude JSON，成功返回 `{}`。历史回填只能显式执行 import。指定 source 文件或目录后，仅允许对应转录被排队。

安装在 settings.json 合并本配置标识的 handler，并在私有配置目录保留原始备份；既有 hooks、matcher、模型、权限及其他设置均保留。不会更改 disableAllHooks，用户需要确保原生 hooks 启用并完成 Claude 要求的信任。卸载仅移除本配置的 handler。所有 handler 同步运行，Windows 使用编码 PowerShell 命令，Unix 参数使用 shell 引号。

SessionStart、UserPromptSubmit、PostToolUse、PostToolUseFailure、Stop、StopFailure、SubagentStop 和 SessionEnd 只触发当前转录的增量采集。SubagentStop 使用 agent_transcript_path，不会误采父会话路径。先持久排队再等待停止类事件的 250 ms 写入缓冲期或发送网络请求。停止类事件及 SessionEnd 发送后仍保留转录重试入口，因为 Claude 可能在 hooks 结束后才写最后一条记录；下次事件或手动 flush 会处理尾部。失败不推进未 ACK 的检查点，也不会伪造完成事实。

会话使用原生 sessionId，任务归属使用真实用户提示 uuid。工具结果的 user 角色、元数据用户消息、压缩摘要不会开启新任务。每个文本、工具调用、工具结果、thinking 和 redacted_thinking block 分别保留。思考映射 assistant/phase=reasoning；隐藏思考映射 redacted_reasoning，完整 raw 保留签名、隐藏数据和其他源字段。usage 使用响应 ID 范围内的累计快照，保留输入、输出、缓存读写计数，不将重复块当作独立增量，不推算 total_tokens 或 provider。仅转录实际存在 reasoningEffort 或 effort.level 时输出推理力度。

仅 assistant stop_reason=end_turn|stop_sequence 或 system subtype=turn_duration 才表示 task_end。Stop 是可被其他 hook 阻止的停止尝试，不能证明任务结束。客户端转录未提供终止或力度字段时保持缺失，协议验证另行核实实际请求中的 max，不凭最后一块文本推断 final_answer。

参考 [Claude 官方 hooks 文档](https://code.claude.com/docs/en/hooks)，其明确说明转录异步写入及 SubagentStop 的 agent_transcript_path。实现依据已安装 Claude Code 2.1.289；测试仅使用合成记录，真实协议与客户端回归由统一 QA 执行。
