# 微信私聊通讯

[English](README.en.md)

2.0.0 是不兼容升级：同一位扫码绑定的微信用户可以发送文本和文件，并收到 Agent Flow 的最终文本回复。插件分别贡献 `1flowbase.notification.send` 与 `1flowbase.notification.receive`，负责腾讯 iLink 协议适配；Host 负责持续轮询、持久收件箱去重、游标提交、导入选定文件表、执行 Agent Flow 和发送最终回复。

## 配置

1. 先升级到支持通讯 receive/download 契约、持久收件箱和频道 MCP 自动配置的 Host。manifest 中的最低版本不代表这些能力一定存在。安装官方运行时插件 `1flowbase/wechat-notification`，分配到工作区，授权 `notification.send` 和 `notification.receive`。
2. 创建类型为 `1flowbase/wechat-notification/private` 的频道，选择 Agent Flow 和文件表。插件供应商配置为空，凭证由 Host 管理。
3. 使用 `qr` 操作开始授权，微信扫码并确认。Host 轮询到 `authorized`；二维码过期后重新开始。收件和回复始终限定扫码用户，其他发送者不能改变私聊绑定。
4. 启用频道。即使关闭设置页，Host 也持续发起有界 Receive 调用。文本、图片、语音、文档和视频附件进入持久收件箱；Host 把附件下载到选定文件表后再调用所选 Agent Flow，仅发送最终回复。
5. Host 为每个频道创建独立 MCP 实例。插件声明 `history_messages`、`history_files`、`read_file`，提供解释渐进查询历史和文件的默认可编辑系统提示词；历史工具传入会话消息中的 conversation_id，继续查询时将 next_cursor 原样传为 cursor，read_file 使用 history_files 返回的 attachment_id。ID 为不透明标识；Host 固定当前会话和发送者作用域。用户可以为 Agent Flow 添加其他 MCP。

## 游标与回复窗口

只有 Receive 调用 `getupdates`。成功轮询（包括空批次）返回不透明的**候选游标**，Host 持久化收件箱后才提交；插件不再把游标写入受管 secret patch。供应商批次超过单次消息数时，用候选游标保留待处理消息并逐批排空，不截断丢消息。重放保留供应商消息 ID，由 Host 去重；无效或畸形批次失败且不返回游标。

授权维护和发送只读取 Host 保存的回复窗口与 context，不消费更新。绑定用户的有效消息按腾讯创建时间开启或续期 24 小时窗口。过期保留账号授权，再发消息即可续期。旧实例缺少时间证据时保持未验证，直到 Receive 提供证据；停用的频道不轮询。

供应商明确确认发送成功后返回 `succeeded` 与 `{"accepted":true,"delivery_id":"..."}`。默认零状态字段缺失时，非空 `message_id` 也可确认成功，但不能覆盖明确错误或畸形状态。成功仅表示供应商接受，不代表用户已收到；`delivery_id` 是提交的 `client_id`。

- 未授权：`authorization_required`。供应商 `-14`：`authorization_invalid`，清理账号凭证，需重新扫码。
- 明确 `-2` / prepare failed：`recipient_not_ready`，保留授权。
- 无有效消息证据：`subscription_required`；窗口过期：`subscription_expired`；缺少可验证时间：`subscription_unavailable`。这些状态在发出外部发送请求前阻止发送。
- 其他明确供应商错误：`provider_rejected`。
- 发送结果不确定、超时或断线：`unknown / provider_result_unknown`。插件不自动重试发送。

## 媒体与网络边界

附件引用只由 Host 保存，包含供应商消息、附件身份和不透明媒体 locator/key，不向模型暴露下载地址。媒体仅访问 HTTPS `novac2c.cdn.weixin.qq.com/c2c/download`，使用供应商发出的 `encrypted_query_param`。拒绝重定向、任意域名、其他路径、URL 凭证和异常端口；CDN 请求不携带机器人 bearer token。

供应商使用 AES-128-ECB 和 PKCS#7。插件把锁文件已有的 `aws-lc-rs` **1.16.3** 显式声明为直接依赖，没有引入新第三方包；兼容供应商的 base64 原始密钥与 base64 十六进制密钥两种编码。ECB 不提供认证加密：实现检查 HTTPS、密文块对齐、末尾 padding、HTTP Content-Range 和可用时的明文长度；协议没有认证媒体摘要，因此不能脱离 HTTPS 独立证明来源完整性。

Host 按字节偏移请求，每次最多返回 48 KiB 解码后的明文。CDN Range 请求按 AES 块对齐，并多读取一块判断真实文件末尾，只有到末尾才移除 padding。没有文件总大小上限，也不整文件缓冲。CDN 忽略 Range 返回 HTTP 200 时，插件流式丢弃前缀，仅保留所需块，发现后续字节后立即停止读取。该方式不限制文件总大小，但后续 chunk 需重复下载前缀；真实 CDN 互操作和性能仍需已授权账号验证。

## 验证与协议证据

Mock fixture 覆盖扫码绑定、私聊隔离、重复与重放身份、游标溢出、空批次和畸形批次、授权失效、仅用缓存发送、媒体引用、AES 下载、大文件和非对齐 chunk、不安全地址及损坏 padding/长度。由 Root 集中 QA 在新 SDK 上执行；fixture 本身不代表微信实网验收通过。

协议参考 [NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent/blob/0e427d788d253630d38a3a51cd76849e73cd6925/gateway/platforms/weixin.py)（MIT，Python；2026-10-25 观察到 256,843 Stars、56,194 forks）。其消息类型映射、AES 密钥编码和 CDN locator 与本任务吻合。本实现适配其协议模式，持久化由 Host 负责，下载使用有界 typed chunk，不复制其整文件下载与游标持久化方式。

SDK git revision 暂保留上一次已发布 pin，由 Root 发布新 Host 契约提交后更新。集中本地验证时临时将 `runtime-extension-sdk` patch 到 assembly 的 `api/crates/runtime-extension-sdk`，不提交本地路径或尚未发布的 git SHA。
