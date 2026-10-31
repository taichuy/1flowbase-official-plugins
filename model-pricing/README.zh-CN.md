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

同步以两份上游目录的供应商／模型身份并集为输入，自动发现新增供应商和模型，
不再以本地已有文件作为白名单。保留现有身份映射：`kimi` 对应国际版 `moonshotai`／`moonshot`，
`google` 对应 `google`／`gemini`，`zai` 对应 `zai`；其他供应商保留各自的上游身份。
不同服务商、地区或中转商的同名模型不会混用价格。仅移除两份源中均已消失的、由社区同步维护的配置；
本地手工配置、`zero/any`、禁用及 `extensions.price_sync: false` 配置保留。

完整模型清单及上游价格保存在 `_maintenance/upstream-models.json`。每个模型标明
`synchronized` 或 `unavailable` 及具体原因。输入、输出、缓存读取、缓存写入四个默认价格
必须全部有效且非负，才进入标准价格目录；缺字段不会填成 0 或输入价格，明确的 0 可以同步。
图片、音频、视频等当前四类 token 价格契约无法表达的模型仍记录在清单中，不伪装成可用报价。
因此完整模型清单不等于所有模型都有完整可用的标准价格。已有配置无法取得完整新报价时保留旧配置，
清单会标记不可用，不能将其视为已核验的当前报价。

统一输出 USD／百万 token 的十进制字符串，LiteLLM 的 USD／token 使用十进制换算。
同步默认价格及上游明确给出的上下文分层规则；Models.dev 的 `cost.tiers` 优先，LiteLLM
精确的 `_above_<N>k_tokens` 字段补充无主源分层的模型，不根据 `context_over_200k` 这个历史字段名猜阈值。
同步 LiteLLM 源码已定义的 1 小时／12 小时缓存 TTL 价格及其上下文组合条件。自动管理的规则在上游撤销时同步删除；
其他手工 TTL、复合条件及时段规则保留，需要单独审核。未导入 batch、flex、priority
等当前条件契约无法表达的服务等级价格。价格或上下文规则变化后生成新 UUID，以同步观察时间填写
`effective_from`，不将其声称为厂商调价时间。来源记录每个计量项的数据源；不继承旧厂商核验标记。
默认价格及规则不变时保留 ID、时间和文件内容；相同上游快照重复同步不产生差异。

价格目录有新增、更新或移除时递增 `catalog_version`，重新生成并检查索引、分页、搜索索引、
聚合快照、seed 和维护状态，由 Action 提交到 `main`。历史 ID 保留；写入失败会整体恢复源文件与生成产物。
两份上游快照均须下载成功，网络或无效快照使任务失败。请求最多重试三次，每次限时 60 秒。
Action 摘要及 `model-pricing-sync-report` artifact 记录发现、新增、更新、移除、未变及不可用项。
GitHub schedule 可能延迟，不保证整点附近执行。签名 tag release 沿用独立发布流程；同步不自动创建签名 release。

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

发布分页每批最多 20 个模型配置，与控制台默认页大小一致。先读取索引总数，再按需读取对应分页；安装同步逐页拉取与暂存，全部验证成功后提交。
