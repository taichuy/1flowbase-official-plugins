# Gateway Demo

此模板来自本地 `/route` 工作台，包含页面、关联应用、数据模型定义及关系、MCP 实例 `1flowbase`。页面中的分页、筛选和区块设置随页面文档一起保存。

`manifest.json` 使用 `1flowbase.application-template-archive/v1`，分片保留完整 `1flowbase.portable-template/v1` 定义。本次源码迁移到 release v2；`package.release.template_id` 为稳定身份，`release_version` 为正整数。内容变更必须递增版本，同版本不同 ZIP bytes 拒绝发布。发布、签名与工具用法见 [应用模板说明](../../README.md)。

模板仅携带定义，不包含业务数据记录、用户密码、供应商密钥或外部数据库连接。系统内置数据模型按 code 引用，页面依赖宿主内置的 `1flowbase@2.0.0`。

后端镜像内置本目录。默认在启动时安装或更新模板，已有资源直接覆盖，缺失资源新增；不会根据用户是否编辑进行保护。设置 `API_APPLICATION_TEMPLATE_AUTO_UPDATE=false` 可同时关闭空库初始化和后续自动更新。扩展中心的“应用模板”仍允许手动安装。

升级不删除模板未提及的资源与业务记录。安装失败不会记录为成功版本，后续启动可重试。
