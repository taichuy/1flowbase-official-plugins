# Google Search Console 插件 (`@taichuy/google-search-console`)

为 1flowbase 提供面向 Google Search Console (GSC) 的原生插件设置页与 MCP 实例（`google_search_console`）。支持一键 Google OAuth 弹窗授权、凭证持久化与自动刷新、多账号与多站点管理、站内数据概览、Sitemap 查询/解析/管理、Search Analytics 查询与 URL 索引检查。

## 功能概览

- **一键 Google OAuth 2.0 (PKCE) 授权**：
  - 在设置页配置 Google Cloud OAuth Client（`client_id` 与 `client_secret`，其中 `client_secret` 仅写入 1flowbase Host 加密凭证库，永不明文回传）。
  - 点击「使用 Google 登录」发起浏览器弹窗授权（支持 `BroadcastChannel` / `localStorage` 跨窗口回传与同页回退）。
  - 默认采用只读权限 `webmasters.readonly`（`readonly` 模式），可选 `webmasters`（`full` 模式，用于提交或删除 Sitemap、添加或删除站点）。
- **多账号与站点持久化**：
  - 已授权账号与站点快照保存在插件托管数据表中，支持自动 `refresh_token` 单飞刷新、手动同步与撤销授权。
- **GSC 数据能力**：
  - `sites`：同步与列表、30 天点击/展示/CTR/平均排名总览、属性详情、新增/移除站点。
  - `sitemaps`：列举站点 Sitemap、查询 Sitemap 详情、抓取并解析 `<urlset>` / `<sitemapindex>` XML URL 条目（含 SSRF 校验）、提交/删除 Sitemap。
  - `analytics`：Search Analytics 多维检索查询。
  - `inspection`：URL Indexing Inspection API 检查。
- **MCP 实例 (`google_search_console`)**：
  - 暴露 15 个标准 MCP 工具（`plugin.google-search-console.*`），默认按 `/gsc` 入口及 `accounts` / `sites` / `sitemaps` / `analytics` / `inspection` 分组挂载，供 AI 直接分析站点表现与 Sitemap 状态。

## 配置步骤

1. 在 [Google Cloud Console](https://console.cloud.google.com/) 启用 **Google Search Console API**。
2. 创建 **Web 应用** 类型的 OAuth 客户端 ID，将 1flowbase 设置页展示的**授权回调 URI（Authorized Redirect URI）**（例如 `https://your-host/settings/google-search-console`）填入 `Authorized redirect URIs`。
3. 在 1flowbase「设置 → 插件 → Google Search Console」填入 `Client ID` 与 `Client Secret` 并保存。
4. 点击「使用 Google 登录」完成授权，随后即可同步站点并在 AI 对话中使用 MCP 工具。

## 开发与校验

```bash
cargo test --offline
node scripts/generate.mjs --check
node --test scripts/_tests/manifest.test.mjs
```

