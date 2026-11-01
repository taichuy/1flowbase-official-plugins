# 太初y 日历排盘 · 可编辑页面模板

一页一块，块内显式组合 `CalendarProvider`、`Calendar`、`Almanac`、`BirthInfo`、`ZiWei`、`Bazi`。
导入后直接编辑区块 JSX 即可调整列布局、Tabs、组件顺序和逻辑，不依赖整页黑盒。

- 先安装、分配并启用 `taichuy-calendar@0.1.7`，再用现有应用模板的预览/导入/安装操作。
- 安装会新建页面，宿主自动映射插件安装 ID；不会覆盖用户当前日历页。
- 模板只携带页面定义与源码，不携带用户数据、会话或凭据。
- JSX 编辑器中的“页面模板”是可复用代码模板；本目录则是完整可移植页面模板，两者不混同。
- 修改插件的 `examples/composed-page.tsx` 后，运行 `node scripts/build-page-template.mjs` 更新分片及 ZIP。
- 本源码目录不表示已发布官方签名云端目录；正式发布沿用 applications-demo release 流程。

[English](README.en.md)

模板使用带 `calendar-composition` slug 的顶栏根页面，符合宿主标准打开路径契约。安装前预览会报告同名路由冲突，不覆盖已有页面。
