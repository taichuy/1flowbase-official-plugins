# OpenAI 供应商插件

[English](README_en_US.md)

`openai` 是 1flowbase 的 OpenAI Responses API 官方运行时扩展，通过宿主的 `stdio_json_multiplex_v1` 契约调用。

1flowbase 底座负责安装、分配、供应商实例、密钥存储和子进程生命周期；插件负责 OpenAI 协议转换、模型发现、用量归一和供应商错误事实。支持 `GET /models` 与 `POST /responses`。

## 配置与传输

支持 `base_url`、`api_key`、`organization`、`project`、`validate_model`，以及可选的 `transport_mode`：`auto`、`responses_websocket` 或 `http_sse`。默认地址为 `https://api.openai.com/v1`。

流式默认使用 HTTP SSE；`auto` 先尝试 WebSocket，再在允许的失败条件下回退 SSE。节点的 `responses_transport_policy` 默认 `inherit`，跟随客户端传输；`force_http_sse` 和 `force_websocket` 可显式选择。未知客户端传输时沿用供应商实例配置。活动流保持流动，静默上游沿用 5 分钟 idle timeout。

## 0.2.72 原生回调恢复

0.2.72 要求宿主 0.5.4 或更高版本，该版本能够校验完整原生上下文的 HTTP 恢复回执。官方安装和更新沿用最低宿主版本准入；旧 0.5.3 不兼容这条恢复路径。兼容性强制覆盖和直接加载绕过默认准入，不属于此版本支持范围。

合法字符串 `input` 只在完成后的恢复历史中规范为 user/input_text item，已有原生请求转换保持原样。完整原始输出 item 包括加密 reasoning 与函数调用，保留顺序和不透明字段。

连接失效后，插件可使用同一调用属主和配置下的完整原始历史，携带已准入的回调输出，移除旧 `previous_response_id`，进行一次完整上下文 WebSocket 重建。若重建请求在产生任何语义输出之前遇到可恢复传输失败，`auto` 可在宿主授权的同一尝试次数和 deadline 内，把实际重建的完整请求交给 SSE。已完成工具不会重执行。

仅有旧 response 游标和新增工具输出不能直接降级 HTTP。历史不完整或属于其他会话、宿主显式声明 ConnectionBound、策略/协议/认证错误、强制 WebSocket、已提交语义输出或预算耗尽，均禁止这条降级路径。没有游标本身也不是上下文完整的证明。

既有 `one_full_context_rebuild` 回执声明实际发送了完整、同属主且已移除旧游标的上下文，并记录最终传输和从零计数的真实上游尝试。重建正文属于当前调用，结束后释放；不新增历史正文表或全会话 ephemeral 缓存。

## 参数、工具与错误

支持 reasoning_effort、temperature、top_p、max_output_tokens、response_format、tool_choice、store 等 Responses 参数，并转发 parallel_tool_calls、include、service_tier、prompt_cache_key 和 metadata。原生函数调用与结果分别映射为 function_call 和 function_call_output。

供应商失败保留 `provider_upstream_error` 及原始 `provider_details.upstream_error` 的空值和未知字段。HTTP 失败保留实际状态码与原始正文；事件错误不虚构 HTTP 状态。结构化 SSE / WebSocket 错误终态停止流处理，本地恢复回执附在供应商事实旁，不覆盖它们。

模型采用动态发现与静态描述的混合方式，价格元数据保持动态。两种传输都不合成 `response.processed` ACK。

## 打包

先按目标平台编译 `openai-provider`，再使用底座的 `scripts/node/plugin/cli.js package` 打包；官方签名与跨平台发布交由既有 release workflow。
