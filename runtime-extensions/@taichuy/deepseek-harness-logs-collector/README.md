# DeepSeek Harness 日志采集器 0.1.0

原生 Cordis 插件，参考 DeepSeek Harness 0.2.1-alpha.2 / d743267388 的实际接口。插件源码嵌入采集器二进制，无常驻进程。配置和安装不会导入历史，不修改原客户端持久化或压缩设置。

```sh
deepseek-harness-logs-collector configure --endpoint https://HOST/api/logs/v1/events --config /绝对路径/collector.json --source /绝对路径/private-journal --key-stdin
deepseek-harness-logs-collector install-hooks --config /绝对路径/collector.json --client-home /绝对路径/dsh-home
deepseek-harness-logs-collector flush --config /绝对路径/collector.json
deepseek-harness-logs-collector uninstall-hooks --config /绝对路径/collector.json --client-home /绝对路径/dsh-home
```

摄取密钥从 stdin 输入。配置、原生事件 journal 和断点由共享 SDK 私密存储。省略 --source 使用 DSH_HOME/1flowbase-logs/journal 或 ~/.dsh/1flowbase-logs/journal；请使用客户端实际 home。import 只显式读取采集器 journal，不扫描原客户端压缩存储。

安装在 cordis.patch.yml 追加带标记的 YAML patch，部署 1flowbase-logs/plugin.mjs。其他配置文本（含 !!js 表达式）原样保留。重装仅替换自己的区块；卸载只删除该区块，并仅在 SHA256 收据匹配时删除生成插件。配置、journal 和断点保留用于重试。标记歧义、非本插件文件、JSON 数组 patch 均拒绝自动改写；JSON 数组用户可以手动使用独立 --patch overlay。

session/event 回调是 fire-and-forget，因此同步 spawnSync 调用 record，把事件完整写入并 sync 后才能返回。普通事件只入队；turn/end 带 --flush，因为客户端轮次结束并不会自动 flush。被等待的 session/flush 也触发投递。网络失败保留 pending 队列，后续 flush 重试；错误信息不会输出原始事件、配置或子进程 stderr。

适配器逐块保留用户、助手、推理；独立 tool/call 负责工具生命周期，助手内嵌 tool-call 留在 raw，避免重复调用事实。工具结果、嵌套 PTC 调用、中断输出、真实结束原因、seq/time/surface 元数据、父会话和紧凑助手流均保留 raw。失败尝试按块聚合文字和推理增量，phase=attempt，不伪造结束。仅报告原生 usage；原生 inputTokens 为不含缓存的输入；规范化 input_tokens 为它与 cacheReadTokens、cacheWriteTokens 之和，缓存项按原生契约独立保留，totalTokens 不推测，reasoningTokens 留在 raw（现有通用合约没有对应字段）。

任务 ID 优先使用明确 turn_id，否则使用原生 turn 数字；明确 parent_turn_id/parentTaskId 在该轮保持。parentSession 是会话血缘，仅留 raw，不能当作父任务 ID。请求 header 与助手 source 提供 provider/model/reasoningEffort。进程退出或缺失 turn/end 不产生虚构终止。

原生接口依据、fixture 覆盖范围和执行说明见 [English README](README.en.md)。构建、验收及真实协议验证由本轮统一 QA 执行。

通过 Anthropic 协议连接网关时，DSH provider 的 baseURL 使用服务根地址（不带 /v1）。自定义 gpt-6-luna 模型设置 reasoningEfforts: {max: max} 和 compat: {forceAdaptiveThinking: true}，才能发送 adaptive thinking 与 output_config.effort=max。Chat/Responses 的 baseURL 使用 /v1。
