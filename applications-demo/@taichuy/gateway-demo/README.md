# Gateway Demo

此模板来自本地 `/route` 工作台，包含页面、关联应用、数据模型定义及关系、MCP 实例 `1flowbase`。页面中的分页、筛选和区块设置随页面文档一起保存。

`manifest.json` 使用 `1flowbase.application-template-archive/v1`，分片保留完整 `1flowbase.portable-template/v1` 定义。`package.release.template_id` 为稳定身份，`release_version` 为正整数。内容变更必须递增版本，同版本不同 ZIP bytes 拒绝发布。发布、签名与工具用法见 [应用模板说明](../../README.md)。

release v3 同步报表三个区块的源码、`reportState` 端口声明和 `usage.reportState` 映射：概览统一请求数据，趋势图及用户表共享当前结果；筛选更新期间保留已有结果，切回页面复用当前查询。升级时这些定义一起安装，页面和区块的模板 ID 保持不变。

模板仅携带定义，不包含业务数据记录、用户密码、供应商密钥或外部数据库连接。系统内置数据模型按 code 引用，页面依赖宿主内置的 `1flowbase@2.0.0`。

后端镜像内置本目录。默认在启动时安装或更新模板，已有资源直接覆盖，缺失资源新增；不会根据用户是否编辑进行保护。设置 `API_APPLICATION_TEMPLATE_AUTO_UPDATE=false` 可同时关闭空库初始化和后续自动更新。扩展中心的“应用模板”仍允许手动安装。

升级不删除模板未提及的资源与业务记录。安装失败不会记录为成功版本，后续启动可重试。

## 工作流节点 MCP

release v4 增加工作流节点工具分组，提供 HTTP、SQL、数据 CRUD、模板转换、代码和变量聚合的配置示例与单点调试入口。先读取 `workflow_ops_create_application` 的完整说明，再创建或复用应用、获取节点目录、保存编排并调试。节点调用在 `mcp_call` 外层传入这份前置说明的当前 `des_id`；应用、节点与权限仍由后端校验。单点执行不会自动运行上游，写节点会真实修改授权数据。需支持此描述校验契约的宿主（导出版本 0.5.3）。模板不包含验收应用或测试记录。

## Workflow node MCP

Release v4 adds configuration examples and node-preview tools for HTTP, SQL, data CRUD, template transforms, code, and variable aggregation. Read the complete `workflow_ops_create_application` description first, create or reuse an application, discover its nodes, save the graph, and preview a target node. Pass the prerequisite description’s current `des_id` at the outer `mcp_call` level. The backend still validates the application, node, and permissions. A preview does not run upstream nodes, and write nodes modify authorized data. The host must support this description-validation contract (exported from 0.5.3). This template excludes acceptance applications and test records.
