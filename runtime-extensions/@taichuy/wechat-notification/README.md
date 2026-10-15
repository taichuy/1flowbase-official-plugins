# 微信私聊通知

通过腾讯 iLink Bot API 扫码授权，将文本通知发送给**扫码并确认授权的微信用户**。这是 `1flowbase.notification.send` 的正式 Rust RuntimeExtension；不需要部署其他微信服务、监听器或数据库。

[English](README.en.md)
需要提供 `notifications.instances.authorization` 接口的 Host；版本号门槛不能代替这项能力。


## 使用

1. 从正式运行时扩展目录安装 `1flowbase/wechat-notification`，分配给工作区并授权 `notification.send` 贡献。
2. 创建类型为 `1flowbase/wechat-notification/private` 的通知实例，无需填写用户配置。
3. 执行授权操作 `begin`，action 为 `qr`，使用微信扫描返回的二维码并在微信中确认。Host 调用 `poll` 直到 `authorized`；二维码过期后重新开始。
4. 绑定用户向机器人发送任意消息，开启24小时通知窗口；每条新的用户消息按腾讯消息创建时间滚动续期。然后发送输入 `{"text":"通知内容"}`。只有扫码用户能成为该实例的接收者；其他用户或机器人消息不能改变窗口、收件人或 context。

启用/禁用、扫码授权、通知窗口是独立状态。窗口过期保留扫码授权，向机器人重新发消息即可续期，无需重新扫码。取消待扫码挑战使用 `cancel`；`maintain` 和每次发送会同步当前账号的入站消息，不启动后台服务。未打开页面或没有发送任务时不声称持续接收；延迟同步仍从真实消息时间计算，不延长有效期。该扩展只发送私聊文本，不处理群聊、媒体或交互会话。

需要支持 `NotificationAuthorizationView.delivery_window`、Host注入 `NotificationRuntimeInput.delivery_window` 及订阅失败码的 Host；先升级 Host，再安装 1.1.0。旧实例没有消息时间证据时显示“待确认”，不会假定已订阅或伪造首次消息时间。

## 结果与状态

明确的 iLink 成功响应返回 `succeeded`，结果为 `{"accepted":true,"delivery_id":"..."}`。成功状态可由 `ret: 0` 确认，也支持 HTTP 成功回包只有非空 `message_id`、省略默认状态字段的形式；显式错误或格式错误的状态字段不会被 `message_id` 覆盖。`accepted` 表示腾讯供应商接受请求，**不表示用户已收到或已读**。`delivery_id` 是该请求使用的 iLink `client_id`。

- 没有授权：`authorization_required`；token/session `-14`：`authorization_invalid`，清除账户凭证，需要重新扫码。
- 明确的 `-2` / prepare failed：`recipient_not_ready`，保持 `authorized`。可向账号发一条微信消息后再发起新通知。
- 已授权但尚未收到可验证用户消息：`subscription_required`；通知窗口过期：`subscription_expired`。两者在外部发送前阻断，返回安全原因，由Host完成失败日志。向机器人发送任意消息后刷新或重新触发通知。
- 入站同步失败、消息时间缺失/无效或旧实例无可验证时间：`subscription_unavailable`，不能将它误称为用户从未发过消息。已知有效窗口仍可在其原到期前发送，但失败同步不会续期。
- 其他明确供应商错误：`provider_rejected`。
- 发送超时、连接中断或无法证明接受的响应：`unknown / provider_result_unknown`。扩展不会自动重试发送；重新触发通知前需考虑重复发送风险。

每次发送前尝试一次 `getupdates`，只接纳绑定用户的 `message_type=1` 消息，按有效 `create_time_ms`（毫秒）取最大值，滚动到该时间+24小时；所有消息item类型都可续期，不要求文本或 context。重复/乱序消息不会回拨窗口，未来、缺失和非法时间不会以拉取时间替代。合法getupdates允许省略默认零状态字段，不会因此丢弃消息。

这个准备请求沿用 1 秒客户端取消长轮询语义，没有虚构服务端短轮询字段；超时表示无法取得证据，不代表空消息批次。只有仍有效的已知窗口允许沿用已有 context 或无 context 发送。窗口状态和时间通过独立 typed `delivery_window` 交Host持久化，绝不混入 managed secrets，也不将24小时判断交给前端。

## 凭证与网络边界

二维码挑战、bot token、bot id、绑定用户、base URL、游标和 context 只通过 `managed_secret_patch` 交 Host 加密保存，并由 Host 注入 `managed_secrets`。插件不读写本地凭证文件，不打印凭证或原始供应商响应；公开授权视图只有状态和二维码展示信息。新的扫码授权会清除旧账号状态。

生产请求固定从 `https://ilinkai.weixin.qq.com` 开始，供应商返回的 base URL / QR redirect 仅允许 HTTPS 的 `*.weixin.qq.com`，无 URL 账户、额外路径、查询或非 443 端口。HTTP 自动 redirect 关闭。配置不能传入任意 endpoint。

## 构建与验证入口

依赖 Host SDK 固定提交 `75338d4c3481273fc01f909e577be636933b6fef`。SDK 的 `execute_authorizable_notification` 校验输入/结果，`serve` 提供共享 dispatcher；没有另写 stdio 协议。manifest 的可执行文件路径为 `bin/wechat-notification`。

```bash
cargo test --manifest-path runtime-extensions/@taichuy/wechat-notification/Cargo.toml
cargo build --release --manifest-path runtime-extensions/@taichuy/wechat-notification/Cargo.toml
```

`src/_tests/behavior.rs` 的注入式 iLink mock 覆盖扫码流程、Host secrets 恢复、redirect/过期/取消、无 context 直接发送、扫码用户绑定、失效/拒绝和未知发送结果。mock 不证明真实扫码、腾讯服务的可用性或实际微信送达。真实验证需安装正式包，真实用户扫码后执行通知并检查微信收件；不要把 mock 通过当作外部验证。

## 协议参考

第一方 [Tencent/openclaw-weixin](https://github.com/Tencent/openclaw-weixin) 的 API 类型、请求头及可选 context 发送协议；本地 Hermes Weixin 适配器（MIT）用于交叉核对 QR 状态和错误分类。实现独立编写，不包含大段上游源码。

## 界面多语言

1.1.1 的通知类型名称、说明、输入标签与扫码动作词典随本插件 manifest 的 `notification_types[].i18n` 发布，提供 `zh_Hans` / `en_US`。Host需先支持该可选typed字段，前端按当前语言显示；词典缺项依次回退英文词典和原始声明。用户填写的渠道名称、消息正文、配置值与字段/action代码不翻译。内置系统通知由底座前端维护，不包含在本插件词典。升级不会清除原账号授权或订阅窗口。
