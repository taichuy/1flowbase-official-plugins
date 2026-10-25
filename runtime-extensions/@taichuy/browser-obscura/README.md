# Obscura 浏览器插件

在 AI 会话中操作临时浏览器，并通过后台查看和管理同一批浏览器实例与标签页。插件直接链接 Obscura Rust 库，宿主通过受管插件协议启动和回收一个按用户及来源工作区隔离的插件进程。

## 安装与接入

需要支持 `actor_workspace_scope`、可信 `ManagedServiceCallContext` 和 `managed_service.mcp_instance` 的宿主构建。最低基础版本为 0.5.8；旧的 0.5.8 构建不包含这些新增契约，必须先部署对应宿主源码版本。精确 SDK revision 以 Cargo.toml 为准，CI 校验插件与宿主的配对 SHA。

1. 在扩展中心安装并启用 `browser-obscura`，首版发行目标为 Linux amd64 GNU。
2. 在当前工作区的 MCP 管理中找到 `browser_obscura`，按现有 MCP 认证流程复制连接配置到 AI 客户端或选择到应用会话中。
3. 打开 `/settings/browser-obscura` 管理实例。后台页面与 MCP 必须使用同一用户和工作区，才能看到同一批资源。

启用会首次创建专属 MCP 实例、工具、绑定与发现策略。重复启用及升级保留用户修改和删除，不强制恢复用户已删除的配置；需要新的副本时使用示例或现有导出/导入功能。停用、撤权或卸载后，相应操作不可继续执行。

## 使用顺序

通过 `mcp_list` / `mcp_get` 渐进式发现工具，推荐顺序：

```text
browsers_create → tabs_create → page_snapshot
    → page_fill / page_click / page_press_key → 新的 page_snapshot
    → page_screenshot → tabs_close / browsers_close
```

所有页面操作显式提供 `browser_id` 和 `tab_id`。元素操作还需要最新 `snapshot_id` 与 `ref`，不可在导航或页面变化后复用旧引用。一次点击或 Enter 可能提交表单；遇到超时或结果未知时先检查当前页面，不自动重复提交。

同一实例的 tabs 共享浏览器上下文；不同实例的 cookies 与 storage 隔离。首版不保存登录 profile，进程退出后旧 ID 失效，不自动恢复页面或重放操作。离开管理页只停止页面请求，不关闭 AI 创建的浏览器。

实例空闲时自动到期，创建时可设置 `idle_timeout_ms`；默认值以服务实现为准。查询与业务操作的续期行为、在途调用保护由后端统一执行。显式关闭浏览器会关闭其 tabs 并释放宿主资源租约。

管理页支持实例搜索、分页、创建、关闭、标签页详情、导航和截图。截图通过 HTTP 返回完整 PNG 数据，通过 MCP 映射为原生 image 内容及独立元数据。

业务失败走宿主标准 HTTP 422 错误响应，稳定错误码位于 `code`；MCP 保留为 `target_code`，不会误当截图数据或自动重放操作。

## 可编辑 MCP 示例

`_assets/mcp/` 是标准 `1flowbase.mcp.bundle/v2` 目录，包含带校验值的 manifest、tools 和 instances。示例使用独立 `browser_obscura_example` 身份与工具 ID，不覆盖自动创建的默认实例。修改文件后需重新生成对应 SHA-256。

在 `_assets/mcp/` 目录将 `manifest.json`、`tools/` 与 `instances/` 压缩为 ZIP，通过宿主 MCP 导入预览确认后导入。示例不包含 API key、浏览器 cookies 或用户数据。名称、描述、参数映射、结果映射、分组与发现策略都可按任务编辑；配置不能绕过业务权限。

## 能力与边界

- 引擎依赖固定为 `h4ckf0r0day/obscura@fdeac4421eaab3b1f0c26c71c2214fa5c8ad2fb9`；启用 render，保留同 revision 的 taffy/cosmic-text 上游补丁。没有 CDP 服务或额外浏览器 daemon。
- 提供导航、页面快照、点击、填写/追加输入、按键、滚动、条件等待和 PNG 截图；不提供任意宿主命令、文件系统、cookie 导出或跨用户接管。
- 保留 Obscura 的网络访问约束；生产默认不开放私网或 file URL。确定性测试在 fixture 内显式授权本地临时 HTTP 服务，不改变发行包默认值。
- Obscura 对部分 Web API、Service Worker 和复杂渲染的支持仍不等同 Chromium。只对实际验证的目标站点报告兼容性，不承诺任意网站可用。
- 底座管理认证、来源工作区、子进程、取消和 MCP 注册；插件负责页面对象、同资源操作顺序、后台页面任务与资源释放。强制回收插件进程会使其承载的临时实例失效。

## 开发与验证

生产依赖均固定在 Cargo.lock。插件新增直接依赖 Obscura crates，用于浏览器引擎；runtime-extension-sdk、Tokio、Serde、base64、UUID 和 URL 承担既有协议、运行与数据编码职责，不引入独立浏览器驱动框架。

引擎测试使用 nextest，每个测试独立进程：

```bash
cargo nextest run --release --locked --all-targets
cargo build --release --locked --bin browser-obscura
```

构建后由宿主 `scripts/node/plugin/cli.js package` 生成安装包。专属 CI `.github/workflows/browser-obscura-ci.yml` 校验精确宿主/插件 SHA，运行真实引擎测试并打包，证据输出到 `tmp/test-governance/browser-obscura/`。页面 fixture 为 `ui/_tests/settings.browser.mjs`，由宿主 page-debug 临时认证 session 挂载页面后在桌面/移动视口执行。

源码中存在测试不代表已通过；交付状态以 Root Issue #2679 的候选 SHA、QA 与安装/发布证据为准。

## 许可

插件与 Obscura 采用 Apache-2.0；上游依赖保留各自许可。参见 LICENSE、_assets/NOTICE 与 _assets/licenses/。本插件未复制私有商业插件源码或资产。
