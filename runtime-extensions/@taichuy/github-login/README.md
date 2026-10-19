# GitHub 一键登录

[English](README.en.md)

官方受管认证插件，贡献 `1flowbase/github-login` 登录方式。使用 GitHub OAuth App、state 和 PKCE S256，仅申请 `user:email`，不申请仓库权限。Host 持有配置密文，负责登录事务、不可变数字 GitHub ID 绑定、首次注册和会话；插件仅验证 GitHub 身份及注册年限。

## 配置

需要包含受管外部认证支持的宿主，最低兼容发行目标为 **0.5.8**。已发行的 **v0.5.7 不包含此能力**。当前开发联调应使用已包含本次认证改动的 `dev` 构建；若它仍报告版本 0.5.7，仅在核对源码后使用扩展中心现有的精确兼容性确认，不能借此在旧发行版上启用缺失能力。

1. 安装插件，在认证中心创建 GitHub 登录入口。
2. 在 GitHub **Settings → Developer settings → OAuth Apps** 创建应用，将 Host 显示的只读「回调地址」原样填写到 **Authorization callback URL**。
3. 将 OAuth App 的「客户端ID」和「客户端密钥」填入认证中心并保存，启用登录入口。
4. 按需打开「允许新用户注册」，设置「GitHub账号最低注册年限」（非负整数，默认 0 不限）。已有绑定用户不受注册开关、年限或邮箱缺失影响。

登录按钮通过 Host 的 `ctx.auth.redirectToExternalLogin()` 跳转到 `/api/public/auth/external/start?login_entry_id=当前入口ID`，无需在 UI Block 填写配置、密钥或绝对地址。`public_variable_keys` 为空；密钥、OAuth code、PKCE verifier 和 access token 不进入公共 UI、身份 profile 或错误信息。

## 身份和注册限制

身份键为 `github_user_id` + GitHub 数字 ID，GitHub 用户名变化不改变绑定。profile 仅保留 `login`、`name`、`avatar_url`、`created_at`、已验证主邮箱（存在时）和 `email_verified`。未验证或非主邮箱不用作注册邮箱；Host 只在首次注册时要求邮箱。

最低年限按 UTC 日历周年检查；2 月 29 日账号在非闰年的周年日固定为 2 月 28 日。无效或未来的 `created_at` 拒绝首次注册，包括年限设置为 0 时。插件不创建用户，也不改变已绑定账号。

## 构建与验证

运行 `cargo test --manifest-path runtime-extensions/@taichuy/github-login/Cargo.toml` 可执行定向 fixtures，包括 multiplex Host HTTP mock、PKCE、秘密不泄漏、邮箱筛选、稳定 ID 和周年边界。所有 HTTP 通过 runtime-extension-sdk 的 Host egress 发起，插件不构造网络客户端。

现有 `list-provider-package-targets.mjs` 自动发现此目录；`provider-release` 统一生成各平台包、签名和 registry 项。开发变更不主动创建 release。SDK git revision 必须指向包含认证 contract reexport 的 Host 提交；本地联调可使用不提交的 Cargo patch 配置。

## 依赖范围

本插件新建独立 Rust 包，宿主不新增第三方依赖。插件使用现有 `runtime-extension-sdk`（git revision 固定在 `Cargo.toml` / `Cargo.lock`）提供通信和 HTTP 出口；不自行实现 worker 协议或网络客户端。直接第三方依赖的锁定版本和用途如下：

| 包 | 锁定版本 | 用途 |
| --- | --- | --- |
| `tokio` | 1.53.3 | SDK worker 的异步运行时与测试 IO |
| `serde` / `serde_json` | 1.0.229 / 1.0.154 | typed 配置、认证请求和 JSON 响应 |
| `base64` | 0.22.1 | Host HTTP wire 的请求/响应正文编码 |
| `url` | 2.5.8 | OAuth URL 和表单编码 |
| `time` | 0.3.55 | UTC 日期与日历周年检查 |

登录入口声明 `entry_action: external_login`：用户在登录方式列表点击 GitHub 后，宿主直接发起授权，不再先打开包含第二个登录按钮的区块。未声明动作的入口默认 `render_ui`，保留表单流程。直接访问入口链接或仅有该入口时仍需用户点击；取消或失败后不自动重试。
