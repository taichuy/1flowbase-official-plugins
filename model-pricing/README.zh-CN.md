# 模型价格目录

[English](README.md)

本目录是官方 USD 模型价格模板的来源。源文件位于
`@<provider_code>/<model-key>/pricing.json`，目录名必须与 `provider_code` 一致，
`upstream_model_id` 是模型身份真值。每个供应商／模型仅保留一份当前标准配置。
用户安装后的价格由用户独立维护，消费端不得覆盖用户手动修改。

## 每小时价格同步

`model-pricing-sync` GitHub Action 在 `main` 上每小时第 17 分钟（UTC）运行，
也支持手动触发。主数据源为 [Models.dev](https://models.dev/api.json)，
单个字段缺失或非法时，从 [LiteLLM 价格表](https://github.com/BerriAI/litellm/blob/main/model_prices_and_context_window.json)
的同一原生供应商补充。转换脚本仅使用 Node.js 标准库，不安装 SDK 或网关。
上游 MIT 声明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

只更新本目录已有模型，不新增上游模型，不删除未找到的模型。
输入、输出、缓存读取、缓存写入四个价格必须全部有效且非负，才更新整个模型。
缺失或非法值不会被补成 0 或输入价格；明确的 0 可以同步。
统一输出 USD／百万 token 的十进制字符串，LiteLLM 的 USD／token 使用十进制换算。

供应商映射限定原生服务商，不按模型名称借用中转商价格。`kimi` 映射国际版
`moonshotai`／`moonshot`，不使用 `moonshotai-cn`；`google` 映射 `google`／`gemini`，
`zai` 映射 `zai`／`z_ai`。未知供应商（含 `zero/any`）、未启用或非当前配置，
以及设置 `extensions.price_sync: false` 的配置会跳过。

同步只修改四类默认单价。已有上下文、波峰波谷、缓存 TTL 条件、规则顺序及其他配置
保留，不导入或重算条件规则；默认价格变化后，这些规则仍需单独审核。
价格有变化时生成新 UUID，以同步观察时间填写 `effective_from`，不将其声称为厂商调价时间。
`extensions.source` 改为 `community_pricing`，`extensions.price_sync` 记录观察时间及各计量项
的数据源 URL、供应商、模型和字段；移除旧厂商验证信息。数值不变时保留 ID、时间、
来源和文件内容，因此没有更新时不产生提交。

有更新时递增 `catalog_version`，重新生成并检查索引、分页、搜索索引、聚合快照、seed 和
维护状态，然后由 Action 提交到 `main`。历史 ID 保留在维护状态中。
两份上游快照均须下载成功；网络、JSON 或目录格式错误会使任务失败，避免在来源不完整时
改变价格优先级。请求最多重试三次，每次限时 60 秒。日志及 Action 摘要列出更新、未变和
跳过项。签名 tag release 继续使用既有独立发布流程，每小时同步不创建 tag 或签名发布资产。

实时预览（不写入）：

```bash
node scripts/model-pricing/sync.mjs --dry-run
```

离线复现已下载数据（两份文件必须同时提供）：

```bash
node scripts/model-pricing/sync.mjs --dry-run \
  --models-dev-file /tmp/models-dev.json --litellm-file /tmp/litellm.json
```

验证同步和价格目录：

```bash
node --test scripts/model-pricing/_tests/*.test.mjs \
  scripts/_tests/model-pricing-catalog.test.mjs scripts/_tests/convert-model-pricing-v2.test.mjs
node scripts/model-pricing-catalog.mjs --check
```

## 格式与发布

源格式为 `1flowbase.model-pricing-source/v2`。身份、时段和四组
`*_token_unit_size`／`*_token_unit_price` 位于顶层，价格为十进制字符串。
标准计量项为 `input`、`output`、`cache_hit`、`cache_write`。
`rules` 是必填数组；空数组表示仅使用默认值，每条规则仅含 `when` 和稀疏 `overrides`。
匹配规则按数组顺序应用，后面的规则只替换自身声明的字段。

条件支持输入 token 的 `gt`／`gte`、`cache_write_ttl_seconds`、RFC3339 生效起止时间、
IANA 时区、1..127 星期掩码及成对的 HH:MM:SS 本地时间窗。
时间窗须声明时区，起点包含、终点不包含；其他时段条件继承顶层配置。
TTL 条件只能覆盖缓存写入字段，须有真实 TTL 证据，未匹配时保留默认值。
详细契约见 `schemas/model-pricing/v2/` 和 `scripts/model-pricing-validation.mjs`。

生成产物位于 `catalog/v1/`、`dist/catalog-seed.json` 和 `_maintenance/`。
载荷 schema 为 v2，保留 `catalog/v1/` URL 是为了维持发现入口。
已发布 ID 不可修改，价格修订必须使用新 ID。
签名版本使用 `model-pricing-v<catalog_version>` tag，Ed25519 签名覆盖规范化聚合 `rules` 字节。
套餐、积分、订阅与未公布价格不属于本目录。

## v1 迁移

```bash
node scripts/convert-model-pricing-v2.mjs [repo-root]
node scripts/model-pricing-catalog.mjs
```

转换器离线运行，拒绝未知格式，保留已有效的 v2 文件与旧价格。
旧数据缺少独立缓存写入价格时，转换器沿用旧输入价格以保留旧计费行为，不代表厂商报价。
完整日分区、日期区间和输入分层转换为默认值及有差异的覆盖；TTL 没有证据时不猜测。
不可变状态仅允许一次经校验的 v1 迁移：校验旧聚合、各源校验和及 ID，再比对等价配置，
保留退役 ID，不认可任意价格变化。旧 schema 和 legacy fixture 只用于离线迁移及历史证据。
