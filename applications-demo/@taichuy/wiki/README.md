# Wiki

[English](#english)

按需安装的个人知识库应用模板，包含目录树、Markdown 文章编辑与预览、标签、搜索和移动端布局。它不是默认内置应用。

## 导入与复用

在扩展中心选择 Wiki 应用模板安装；也可在仓库根目录打包 ZIP：

```sh
node --input-type=module -e "import { buildArchive } from './scripts/application-template/archive.mjs'; import fs from 'node:fs'; fs.writeFileSync('/tmp/wiki.zip', await buildArchive('applications-demo/@taichuy/wiki'));"
```

打开 1flowbase 的 `/settings/backups`，点击导入并选择 `wiki.zip`，检查预览后安装。安装后从顶栏打开 `wiki`，创建自己的文章。

模板包含一个页面、`wiki_nodes` 与 `wiki_tags` 两个数据模型及其关系、一个 JS UI 区块的源码和插件依赖声明。不携带源知识库文章、标签记录、用户、会话或凭据。需要支持该区块的 `1flowbase@2.0.0` 插件与富文本组件；本版本从 1flowbase 0.5.8 导出并验证。

后端会重映射资源 ID 和引用。重复安装遵循应用模板的逐项基线合并规则，保留本地修改。此模板定义中的中文 UI 文案保持源页面原样。

## 更新来源

在具有源页面访问权限的 1flowbase 开发仓运行：

```sh
node scripts/node/export-application-template/cli.js --target /absolute/path/1flowbase-official-plugins/applications-demo/@taichuy/wiki
```

`export.config.json` 保存资源选择与稳定模板身份；其中 loopback 地址是维护者本地导出入口，安装者不需要连接它。内容变化会递增发布版本。README 与导出配置不进入 ZIP，发布签名和目录条目由仓库 workflow 生成。

## English

Wiki is an on-demand personal knowledge-base template with a document tree, Markdown editing and preview, tags, search, and a responsive layout. It is not a default built-in application.

Install Wiki from the Extension Center, or run the packaging command above from this repository's root. In 1flowbase, open `/settings/backups`, import `wiki.zip`, review the preview, and install. Open `wiki` in the top bar to create your own articles.

The package contains one page, the `wiki_nodes` and `wiki_tags` data models and their relationships, JS UI block source, and plugin dependency declarations. It does not include source articles, tag records, users, sessions, or credentials. It requires the `1flowbase@2.0.0` plugin and rich-text components used by the block. This release was exported and verified with 1flowbase 0.5.8. UI text remains in Chinese, as authored in the source page.

The installer remaps resource IDs and references. Reinstallation follows the application's baseline-aware merge rules and preserves local changes. Maintainers can refresh the template using the export command above in the source 1flowbase checkout. The loopback URL in `export.config.json` is only a maintainer export endpoint. Changed content increments the release version. Documentation and export configuration are excluded from the ZIP; repository workflows generate signatures and catalog entries.
