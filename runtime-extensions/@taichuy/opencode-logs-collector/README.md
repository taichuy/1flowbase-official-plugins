# OpenCode 日志采集器

OpenCode 默认 V1 运行时通过 `message.updated` 与 `message.part.updated` 发布原生事实。内嵌插件同步记录这些事件，由共享 SDK 私有会话日志上传，同时兼容 V2 的 `session.next.*` 完整事件。安装不启动常驻进程，不读取数据库或历史对话。

```sh
opencode-logs-collector configure --endpoint http://localhost:7800/api/public/applications/APP/agent-logs --source /absolute/collector/sessions --config /absolute/collector/config.json --key-stdin
opencode-logs-collector install-hooks --config /absolute/collector/config.json --client-home /absolute/opencode-config
opencode-logs-collector flush --config /absolute/collector/config.json
opencode-logs-collector uninstall-hooks --config /absolute/collector/config.json --client-home /absolute/opencode-config
```

密钥通过 stdin 输入。`--client-home` 是 OpenCode 配置目录，也可对应隔离的 `OPENCODE_CONFIG_DIR`。仅管理一个按配置路径哈希命名的 `plugins/*.js` 文件，保留原有配置和其他插件。各安装使用独立日志与检查点目录。

原生回调以参数数组调用 `spawnSync`，返回前完成持久化。idle/dispose 只上传已记录事实，不生成成功终态。上传失败保留待处理记录，后续 flush 重试；忽略流式 delta。

V1 用户文本采用原生 part 通知时间，缺失时回退到消息创建时间；assistant 文本/推理仅在 `part.time.end` 存在时采集。running 工具状态提供调用及参数；completed/error 状态提供结果，缺少 running 通知时同时补采该状态中的真实调用和参数。完成的 assistant 元数据提供 usage 和 finish；仅 `finish=stop` 将已记录真实文本投影为 final answer。其他 finish 继续任务或保留错误事实。原始事件保留。

当前默认 CLI 使用 V1 配置：provider 包名放在 `npm`，端点/密钥放在 `options.baseURL/apiKey`，模型 `variants` 为对象。Chat 使用 `@ai-sdk/openai-compatible`，Responses 使用 `@ai-sdk/openai`，两者配置 `variants.max.reasoningEffort="max"`；Anthropic 使用 `@ai-sdk/anthropic` 并配置 `variants.max.effort="max"`。调用 `opencode run --format json -m gateway/gpt-6-luna --variant max`，以网关请求验证协议和强度；完整配置示例见 [English README](README.en.md)。

`OPENCODE_EXPERIMENTAL_NATIVE_LLM` 只切换模型 transport，不改变 Session/config 版本；默认 AI SDK 路径支持上述控制。variant 标签本身不证明 wire effort。

定向测试：`cargo test --manifest-path runtime-extensions/@taichuy/opencode-logs-collector/Cargo.toml`。集成还需默认客户端真实续聊、工具交互和上传回执。fixtures 为合成数据；旧安装二进制需单独验证兼容性。

0.1.1 原生来源分类：synthetic part 作为 Context 保留；user metadata 本身不建立真人轮次，非 synthetic 且未 ignored 的真人 text/file/agent/subtask part 才建立轮次。混合消息保留真人文本，assistant.parentID 通过已知 user 别名关联到真实轮次；新真人输入清理已完成 assistant 正文，保留用户别名与尚未完成的 assistant 归属。晚到旧 part 不夺取当前轮次。启动中途未见亲代时保留其原生 parentID 归属，不补造用户消息；尚未见 message metadata 的 part 保持原有不虚构归属边界，原始日志不删除。仅 text/reasoning 与工具生命周期输出表面消息，其他 part 的正文不被猜测。

正式 experimental.chat.system.transform hook 按原生 system 数组记录 request_system_snapshot，保留 System/raw，不修改数组或其他用户配置。无 sessionID 的 small-model 调用跳过。该值是采集插件运行位置的请求前快照，后续插件仍可能修改；不声称最终 wire 请求完全一致。
