# 1flowbase Official Plugins

---

## 中文


`1flowbase` 官方 provider 插件仓库。

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
   - 使用官方私钥将 provider 打包并签名为 `.1flowbasepkg`
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

## 发布签名配置

`provider-release` 依赖以下 GitHub Actions Secrets：

- `OFFICIAL_PLUGIN_SIGNING_PRIVATE_KEY_PEM`：Ed25519 PKCS8 私钥 PEM
- `OFFICIAL_PLUGIN_SIGNING_KEY_ID`：与主仓库 `API_OFFICIAL_PLUGIN_TRUSTED_PUBLIC_KEYS_JSON` 中一致的 key id

发布时会把 `_meta/official-release.json` 与 `_meta/official-release.sig` 一并写入插件包，并在 registry 条目中写入 `signature_algorithm` 与 `signing_key_id`。
