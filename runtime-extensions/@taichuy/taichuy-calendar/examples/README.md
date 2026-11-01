# 页面源码示例 / Editable page examples

- `composed-page.tsx`：完整响应式页面组合。通过 `CalendarProvider` 共享状态，布局、Tabs、黄历锚点均可编辑。
- `custom-calculation.tsx`：直接导入五个已授权业务包，自行渲染结果，不依赖整页组件。

将源码复制到已获本插件授权的 JSX 区块编辑器。模块来自插件的 `code_modules`，不是任意 npm 解析器；
未安装、未分配或未启用插件的工作区不能导入这些模块。详见上一级中英文 README 的 API 与边界。

`composed-page.tsx` composes responsive panels under one shared provider. `custom-calculation.tsx`
uses all five authorized business packages without the full-page component. Copy either source into
an authorized JSX block. Imports are governed by plugin `code_modules`, not unrestricted npm access.
See the parent English README for API and browser-surface limits.
