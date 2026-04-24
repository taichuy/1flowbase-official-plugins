# 1flowbase Official Plugins

---

## 中文


`1flowbase` 官方 provider 插件仓库。

这个仓库承载：

- 官方 provider 插件源码.
- 发布与打包自动化
- 官方插件注册表 `official-registry.json`

## 仓库结构

- `host-extensions/`：宿主能力扩展目录
- `runtime-extensions/`：运行时扩展目录
- `runtime-extensions/model-providers/`：模型供应商运行时扩展目录
- `capability-plugins/`：能力插件目录
- `capability-plugins/nodes/`：节点能力插件目录
- `official-registry.json`：已发布插件目录元数据
- `scripts/`：注册表与发布辅助脚本
- `.github/workflows/`：CI 与发布自动化

当前官方 model provider 位于 `runtime-extensions/model-providers/<provider_code>/` 下，通常包含：

- `manifest.yaml`：插件稳定身份、版本号与运行时元数据
- `Cargo.toml` 与 `src/`：Rust provider runtime 源码
- `provider/`：provider 协议定义与运行时代码
- `models/`：内置模型元数据
- `i18n/`：界面文案
- `readme/`：provider 说明文档
- `demo/`：本地调试页面资源

## 当前官方 Provider

- `openai_compatible`：OpenAI-compatible API provider 插件

## 发布流程

仓库当前包含两个 GitHub Actions workflow：

- `provider-ci`：在 `pull_request` 和 `push main` 时运行，校验 registry JSON、执行 provider 打包 dry-run，并运行脚本测试
- `provider-release`：在 `main` 分支收到 `runtime-extensions/model-providers/**/manifest.yaml` 变更时运行

正式发布由版本号驱动：

`manifest.yaml` 是 provider 发布版本的唯一维护位置。`Cargo.toml` 中的 `version` 仅用于满足 Cargo 对包元数据的要求，不参与插件发布版本管理。

1. 修改 provider 实现代码。
2. 更新 `runtime-extensions/model-providers/<provider_code>/manifest.yaml` 中的 `version:`。
3. 将变更合并到 `main`。
4. GitHub Actions 会自动：
   - 检测哪些 provider 的版本发生了变化
   - 创建或复用 `<provider_code>-v<version>` release tag
   - 为多个 Linux target 构建 Rust binary 并打包为 `.1flowbasepkg`
   - 发布 GitHub Release 资产
   - 更新 latest-only `official-registry.json`，其中每个 provider 条目包含 `artifacts[]`

如果只改代码而没有修改 provider 版本号，就不会触发正式发布。

## 本地同步

当 GitHub Actions 发布流程自动提交 `official-registry.json` 后，本地 `main` 可能出现“本地领先、远端也领先”的状态。终端同步时可以使用：

```bash
node scripts/sync-main.mjs
```

该脚本只会自动处理满足以下条件的远端领先提交：

- 作者是 `github-actions[bot]`
- commit message 是 `chore: update official plugin registry for version changes`
- 只修改 `official-registry.json`

满足条件时，脚本会自动 `fetch`、`rebase origin/main` 并 `push origin main`。如果远端包含其它提交，或当前工作区不干净，脚本会停止并提示手动处理。

也可以配置一个本地 Git alias：

```bash
git config alias.sync-main '!node scripts/sync-main.mjs'
```

之后使用 `git sync-main`。

## Repair Release

当某个 `<provider_code>-v<version>` tag 已经存在，但需要对同一版本补发或修复多平台产物时，可手动触发 `provider-release` workflow，并设置：

- `provider_code`：目标 provider，例如 `openai_compatible`
- `version`：目标版本，例如 `0.3.9`
- `allow_existing_tag_repair`：设为 `true`

这个模式适用于：

- 某些平台在首次发布时失败，需要补齐缺失产物
- workflow 本身修复后，需要对同一版本重新打包验证

`provider-release` 在 repair 模式下会先删除同一 `provider + version + os/arch` 的旧 release asset，再上传新包。因此即使包名中的 checksum 发生变化，同一平台最终也只会保留一份 `.1flowbasepkg`。

## Release Assets 说明

GitHub Release 页面中的 `Assets` 数量，不等于插件平台包数量。

- `.1flowbasepkg`：由 workflow 上传的真实插件安装包
- `Source code (zip)` / `Source code (tar.gz)`：GitHub 针对 tag 自动提供的源码归档，不是 1flowbase 插件包，也不参与 `official-registry.json`

例如一个 provider 发布了 `darwin/linux/windows x amd64/arm64` 共 6 个平台包时，release 页面通常会显示：

- `6` 个 `.1flowbasepkg`
- `2` 个 GitHub 自动源码包

合计 `Assets 8` 属于正常现象。

## 新增 Provider

1. 在 `runtime-extensions/model-providers/<provider_code>/` 下创建新目录。
2. 至少补齐以下文件：
   - `manifest.yaml`
   - `provider/<provider_code>.yaml`
   - `Cargo.toml`
   - `src/main.rs`
3. 按需补充 `models/`、`i18n/`、`readme/`、`demo/`。
4. 确保 `provider-ci` 通过。
5. 当需要正式发布时，提升该 provider 的 `version`。

## 对主仓库的依赖

provider 打包由主仓库负责执行：

- `https://github.com/taichuy/1flowbase`

发布 workflow 会检出这个主仓库，并使用其中的插件打包 CLI 生成 `.1flowbasepkg` 产物。
