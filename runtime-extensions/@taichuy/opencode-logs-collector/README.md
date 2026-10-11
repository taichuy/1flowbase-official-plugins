# OpenCode 日志采集器

针对 OpenCode 1.18.4 的原生 `session.next.*` 事件，同步写入每会话私有日志，再由共享 agent-logs SDK 上传。安装不扫描数据库或历史记录，不启动常驻进程。

```sh
opencode-logs-collector configure --endpoint http://localhost:7800/api/public/applications/APP/agent-logs --source /absolute/collector/sessions --config /absolute/collector/config.json --key-stdin
opencode-logs-collector install-hooks --config /absolute/collector/config.json --client-home /absolute/opencode-config
opencode-logs-collector flush --config /absolute/collector/config.json
opencode-logs-collector uninstall-hooks --config /absolute/collector/config.json --client-home /absolute/opencode-config
```

采集密钥通过 stdin 输入。`--client-home` 是 OpenCode 配置目录，也可对应隔离的 `OPENCODE_CONFIG_DIR`。安装器只管理按本安装配置哈希命名的一个 `plugins/*.js` 文件，保留原有配置及其他插件。各安装使用独立配置目录，避免共享日志和检查点。

内嵌插件采用参数数组调用 `spawnSync`，原生事件回调返回前完成持久化。采集提示词、请求步骤、结束的文本/推理、工具调用/结果以及请求 usage；忽略流式 delta 和旧版投影重复事件。idle/dispose 只触发上传，不生成完成或 usage。上传失败保留待处理记录，后续 flush 重试。重放通过 SDK 检查点去重，不读取客户端历史。

原生提示词 message ID 标识任务。`step.ended` 的 `finish=stop` 提供完成事实，`tool-calls` 表示继续当前任务。其他 finish 和失败保留为事实 context，不伪造终态。原始事件、时间戳、模型、provider 和模型 variant 保留；variant 是客户端标签，真实推理强度另以网关请求验证。

当前源码可定义 `max` variant 的 request body：Chat 使用 `{"reasoning_effort":"max"}`，Responses 使用 `{"reasoning":{"effort":"max"}}`，Anthropic 使用 `{"output_config":{"effort":"max"}}`。OpenAI 原生 effort schema 排除 max，但 HTTP body overlay 可发送该值。必须选正确 provider/API、使用 `--variant max` 并验证网关协议。旧安装二进制可能采用不同接口，需单独验证。

定向测试：`cargo test --manifest-path runtime-extensions/@taichuy/opencode-logs-collector/Cargo.toml`。原生集成还需当前 OpenCode 源码真实运行和上传回执；测试数据均为合成数据。
