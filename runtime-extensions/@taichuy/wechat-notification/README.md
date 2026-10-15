# 微信私聊通知

通过腾讯 iLink Bot API 扫码授权，将文本通知发送给**扫码并确认授权的微信用户**。这是 `1flowbase.notification.send` 的正式 Rust RuntimeExtension；不需要部署其他微信服务、监听器或数据库。

[English](README.en.md)
需要提供 `notifications.instances.authorization` 接口的 Host；版本号门槛不能代替这项能力。


## 使用

1. 从正式运行时扩展目录安装 `1flowbase/wechat-notification`，分配给工作区并授权 `notification.send` 贡献。
2. 创建类型为 `1flowbase/wechat-notification/private` 的通知实例，无需填写用户配置。
3. 执行授权操作 `begin`，action 为 `qr`，使用微信扫描返回的二维码并在微信中确认。Host 调用 `poll` 直到 `authorized`；二维码过期后重新开始。
4. 发送输入 `{"text":"通知内容"}`。只有扫码用户能成为该实例的接收者；其他用户的入站消息不能改变收件人或 context。

启用/禁用是 Host 的独立管理设置，和授权状态分开。取消待扫码挑战使用 `cancel`；`maintain` 在单次调用内刷新当前授权账号的游标和 context，不启动后台服务。该扩展只发送私聊文本，不处理群聊、媒体或交互会话。

## 结果与状态

明确的 iLink 成功响应返回 `succeeded`，结果为 `{"accepted":true,"delivery_id":"..."}`。成功状态可由 `ret: 0` 确认，也支持 HTTP 成功回包只有非空 `message_id`、省略默认状态字段的形式；显式错误或格式错误的状态字段不会被 `message_id` 覆盖。`accepted` 表示腾讯供应商接受请求，**不表示用户已收到或已读**。`delivery_id` 是该请求使用的 iLink `client_id`。

- 没有授权：`authorization_required`；token/session `-14`：`authorization_invalid`，清除账户凭证，需要重新扫码。
- 明确的 `-2` / prepare failed：`recipient_not_ready`，保持 `authorized`。可向账号发一条微信消息后再发起新通知。
- 其他明确供应商错误：`provider_rejected`。
- 发送超时、连接中断或无法证明接受的响应：`unknown / provider_result_unknown`。扩展不会自动重试发送；重新触发通知前需考虑重复发送风险。

每次发送前尝试一次 `getupdates`，仅取绑定扫码用户的最新 context。这个准备请求的客户端超时为 1 秒（遵循第一方客户端可取消长轮询的语义，没有虚构服务端短轮询字段）；超时/暂时读取失败仍使用已有 context 或不带 context 直接尝试发送。没有入站 context 不会在本地拒绝已经授权的发送，收件人是否可用由 iLink 决定。

## 凭证与网络边界

二维码挑战、bot token、bot id、绑定用户、base URL、游标和 context 只通过 `managed_secret_patch` 交 Host 加密保存，并由 Host 注入 `managed_secrets`。插件不读写本地凭证文件，不打印凭证或原始供应商响应；公开授权视图只有状态和二维码展示信息。新的扫码授权会清除旧账号状态。

生产请求固定从 `https://ilinkai.weixin.qq.com` 开始，供应商返回的 base URL / QR redirect 仅允许 HTTPS 的 `*.weixin.qq.com`，无 URL 账户、额外路径、查询或非 443 端口。HTTP 自动 redirect 关闭。配置不能传入任意 endpoint。

## 构建与验证入口

依赖 Host SDK 固定提交 `948f41cad5b2f951c748a0a0104464588b95d88d`。SDK 的 `execute_authorizable_notification` 校验输入/结果，`serve` 提供共享 dispatcher；没有另写 stdio 协议。manifest 的可执行文件路径为 `bin/wechat-notification`。

```bash
cargo test --manifest-path runtime-extensions/@taichuy/wechat-notification/Cargo.toml
cargo build --release --manifest-path runtime-extensions/@taichuy/wechat-notification/Cargo.toml
```

`src/_tests/behavior.rs` 的注入式 iLink mock 覆盖扫码流程、Host secrets 恢复、redirect/过期/取消、无 context 直接发送、扫码用户绑定、失效/拒绝和未知发送结果。mock 不证明真实扫码、腾讯服务的可用性或实际微信送达。真实验证需安装正式包，真实用户扫码后执行通知并检查微信收件；不要把 mock 通过当作外部验证。

## 协议参考

第一方 [Tencent/openclaw-weixin](https://github.com/Tencent/openclaw-weixin) 的 API 类型、请求头及可选 context 发送协议；本地 Hermes Weixin 适配器（MIT）用于交叉核对 QR 状态和错误分类。实现独立编写，不包含大段上游源码。
