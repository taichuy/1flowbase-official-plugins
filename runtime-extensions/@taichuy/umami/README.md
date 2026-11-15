# Umami

[English](README.en.md)

在 1flowbase 后台维护系统共享的 Umami 连接，并通过已注册接口或专属 MCP 实例管理 Umami。

## 配置与使用

1. 安装并激活插件，在设置中打开 **Umami**（`/settings/umami`）。宿主需要包含 `PluginEphemeralV1` 和完整请求体 MCP 映射能力。
2. 新增连接，填写名称、Umami 基础地址、用户名和密码。地址示例：`https://analytics.example.com`；若 Umami 部署在子路径，则填写 `https://example.com/analytics`，不填写 `/websites` 页面路径。
3. 可先测试草稿，再保存。测试不会保存草稿或密码；保存连接时，后端会独立认证变更后的连接信息。
4. 修改连接时，密码留空保留已保存密码。停用连接后不能通过插件调用该连接的接口。修改或删除连接会清理其临时认证。
5. 在 MCP 设置中使用自动创建的 `umami` 实例。先调用 `hosts_list` 取得 `host_id`，再发现和调用对应分组工具。

设置页复用宿主共享 DataTable、请求桥接和角色 operation 权限。系统 root 可直接操作；其他用户按各接口的当前角色权限执行。列表只返回是否已配置密码，不返回密码、凭据 ID 或认证 token。

## 认证与存储

- 连接元数据：插件拥有的 `umami_hosts` 集合，经 `PluginDataV1` 访问。
- 密码：宿主 `PluginCredentialClient` 加密存储。插件不直接连接数据库、Redis 或本机密钥文件。
- token：仅存入宿主 ephemeral，缓存 TTL 为一小时。worker 重启后可复用；缓存丢失或过期后，使用已保存密码重新登录。token 是不透明值，不尝试解析加密 JWT 的过期时间。
- 上游返回 401 时，先用同一 token 验证会话：会话仍有效则报告权限不足；确认失效后最多重新登录并重试一次。403、网络中断和服务端错误不会触发盲目重放。
- 同一共享 worker 内的同主机请求合并登录；不同用户的 worker 不保证全局只产生一次登录。密码或地址变更使用新的凭据 revision，旧缓存不会被新连接复用。
- 本插件不实现双因素认证账号的登录和自动续登流程。已启用该功能的账号返回明确的不支持状态，不自动消费验证码或恢复码。相关上游 API 仍保留在接口清单中。
- 远端修改账号密码后，需要同步更新保存的连接密码。远端 API 权限继续由 Umami 本身判断。

## 完整接口与 MCP

清单固定于 Umami **3.5.0**、commit `b18d5144dfe7342edeeca6f61d13ba3baabed9ad`：

- 186 个当前 `/api` 操作，包含管理、分析、认证、采集及内部 API；不计 `OPTIONS`。
- 16 个由上游 rewrite 保留的历史报告兼容操作。
- 6 个插件连接管理操作。

共注册 **208 个接口和 208 个 MCP 工具**。上游没有实现的 Umami Cloud 外部网关 API、静态页面、短链与像素 URL、`/mcp` 协议端点不属于该 REST 清单。部署版本不同可能返回 404 或不同业务结果；插件保留上游状态及响应，不伪造成功。

HTTP 路径格式为：

```text
/api/console/managed-services/umami/hosts/{host_id}/api/...
```

参数保留 Umami 的字段名，例如 `websiteId`、`startAt`、`pageSize`。MCP 中普通对象参数直接按字段提供；批量数组和联合请求体通过 `body` 作为整个请求体传入。返回包含 `success`、上游 `status` 和原始 `data`；文本响应保留文本和 `content_type`，非 UTF-8 字节标记 `encoding: base64`。

登录操作使用连接中保存的账号密码，返回认证状态和用户信息，token 保留在 ephemeral。SSO 和双因素验证响应里的协议级 token 也保留在临时存储，不出现在普通结果里。创建 API key 等显式凭据管理操作仍返回其业务结果，相关字段禁止进入普通操作事件。

安装后激活、重新激活或升级会依据包内模板创建/强制重置 `umami` MCP 实例、工具、分组和描述。对该托管实例的手工修改会被覆盖；如需保留自定义配置，请另建实例。连接数据、密码及无关实例不随 MCP 模板重置。

## 开发与验证

新插件直接声明以下既有依赖，复用本机已有缓存；宿主未新增第三方依赖，未下载新包。版本由 `Cargo.lock` 固定。

| 包 | 版本 | 用途与范围 |
| --- | --- | --- |
| runtime-extension-sdk | 0.6.3 / Git revision 见 Cargo.toml | 插件 worker、宿主数据/凭据/ephemeral/HTTP 回调 |
| tokio | 1.54.1 | 插件异步 worker 与同主机锁 |
| serde / serde_json | 1.0.229 / 1.0.154 | 插件 wire DTO 和 JSON |
| base64 | 0.22.1 | HTTP 回调字节编码 |
| url | 2.5.8 | 地址、路径和查询参数编码 |
| uuid | 1.29.0 | 连接与凭据 revision 标识 |

开发生成器复用宿主已安装的 TypeScript 5.9.3 和 Zod 4.4.3，只用于从本地源码提取契约，不安装 Umami 服务端依赖，也不引入 `@umami/api-client` 或 `@umami/mcp`。

```bash
# 修改源码快照时，明确传入本地 Umami 与宿主 web 根目录。
node scripts/extract-upstream.cjs /path/to/umami /path/to/1flowbase/web _assets/api/upstream-contracts.json
node scripts/generate.mjs
node scripts/generate.mjs --check
node --test scripts/_tests/*.test.mjs
cargo test --locked
```

提取器不执行 Umami route handler；路由和 contract 必须逐项吻合。日期枚举取自源码；仅用于运行时校验的时区回调不会执行。生成快照保留版本、commit、原始路由及来源文件，MCP 模板校验全部文件 SHA-256。`ui/_tests/run.mjs` 使用宿主现有前端模块和本机 Chrome 验证真实页面，不构建整个前端。

本地开发可通过临时 Cargo patch 指向同一 assembly 的 SDK，不提交绝对路径。正式包必须固定到包含所需宿主能力的可获取 Git revision。包版本变更时需同步修改生成器中的 `version` 并重新生成。多平台发布产物由官方仓库现有 CI/release 流程验证。

上游契约快照来自 MIT 许可的 Umami；许可全文见 `_assets/api/LICENSE.umami`。本插件代码使用仓库的 Apache-2.0 许可。
