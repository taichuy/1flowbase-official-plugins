# 1Flowbase Official Plugins

[中文](#中文) | [English](#english)

---

## 中文

[切换到 English](#english)

`1Flowbase` 官方 provider 插件仓库。

这个仓库承载：

- 官方 provider 插件源码
- 发布与打包自动化
- 官方插件注册表 `official-registry.json`

## 仓库结构

- `models/`：provider 插件目录
- `official-registry.json`：已发布插件目录元数据
- `scripts/`：注册表与发布辅助脚本
- `.github/workflows/`：CI 与发布自动化

每个 provider 位于 `models/<provider_code>/` 下，通常包含：

- `manifest.yaml`：插件元数据与版本号
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
- `provider-release`：在 `main` 分支收到 `models/**/manifest.yaml` 变更时运行

正式发布由版本号驱动：

1. 修改 provider 实现代码。
2. 更新 `models/<provider_code>/manifest.yaml` 中的 `version:`。
3. 将变更合并到 `main`。
4. GitHub Actions 会自动：
   - 检测哪些 provider 的版本发生了变化
   - 创建或复用 `<provider_code>-v<version>` release tag
   - 将 provider 打包为 `.1flowbasepkg`
   - 发布 GitHub Release 资产
   - 更新 `official-registry.json`

如果只改代码而没有修改 provider 版本号，就不会触发正式发布。

## 新增 Provider

1. 在 `models/<provider_code>/` 下创建新目录。
2. 至少补齐以下文件：
   - `manifest.yaml`
   - `provider/<provider_code>.yaml`
   - `provider/<provider_code>.js`
3. 按需补充 `models/`、`i18n/`、`readme/`、`demo/`。
4. 确保 `provider-ci` 通过。
5. 当需要正式发布时，提升该 provider 的 `version`。

## 对主仓库的依赖

provider 打包由主仓库负责执行：

- `https://github.com/taichuy/1flowbase`

发布 workflow 会检出这个主仓库，并使用其中的插件打包 CLI 生成 `.1flowbasepkg` 产物。

---

## English

[Switch to 中文](#中文)

Official provider plugin repository for `1Flowbase`.

This repository contains:

- official provider plugin source code
- release and packaging automation
- the official plugin registry in `official-registry.json`

## Repository Layout

- `models/`: provider plugin source directories
- `official-registry.json`: published plugin catalog metadata
- `scripts/`: registry and release helper scripts
- `.github/workflows/`: CI and release automation

Each provider lives under `models/<provider_code>/` and typically includes:

- `manifest.yaml`: plugin metadata and version
- `provider/`: provider contract definition and runtime implementation
- `models/`: bundled model metadata
- `i18n/`: UI labels and descriptions
- `readme/`: provider-specific documentation
- `demo/`: local demo assets

## Current Official Provider

- `openai_compatible`: OpenAI-compatible API provider plugin

## Release Flow

This repository currently uses two GitHub Actions workflows:

- `provider-ci`: runs on pull requests and pushes to `main`, validates the registry JSON, dry-runs provider packaging, and runs script tests
- `provider-release`: runs on pushes to `main` when `models/**/manifest.yaml` changes

Formal releases are version-driven:

1. Update the provider implementation.
2. Bump `version:` in `models/<provider_code>/manifest.yaml`.
3. Merge the change into `main`.
4. GitHub Actions automatically:
   - detects which providers changed version
   - creates or reuses the release tag `<provider_code>-v<version>`
   - packages the provider as `.1flowbasepkg`
   - publishes the GitHub Release asset
   - updates `official-registry.json`

If code changes but the provider version does not change, no formal release is published.

## Adding A New Provider

1. Create a new directory under `models/<provider_code>/`.
2. Add at minimum:
   - `manifest.yaml`
   - `provider/<provider_code>.yaml`
   - `provider/<provider_code>.js`
3. Add any required `models/`, `i18n/`, `readme/`, and `demo/` files.
4. Ensure `provider-ci` passes.
5. Bump the provider `version` when you want the plugin to be formally released.

## Host Project Dependency

Provider packaging is performed by the host repository:

- `https://github.com/taichuy/1flowbase`

The release workflow checks out that repository and uses its plugin packaging CLI to produce `.1flowbasepkg` artifacts.
