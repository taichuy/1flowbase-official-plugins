# Langfuse 受管连接器

公开 Apache-2.0 运行时插件，版本 **0.1.0**，发布者 `1flowbase`。
插件拥有 Langfuse API、Basic 认证、分页、metadata 映射和设置页；不包含私有商业插件源码或资产。
[English documentation](README.en.md)

## 使用

1. 安装并激活官方 Langfuse 运行时包，授予其操作及贡献所需权限，打开 **设置 → Langfuse**
   （`/settings/langfuse`）。宿主必须支持配套版本的 `ApplicationLogsV1` 和 `HttpEgressV1`；
   仅宿主版本号不能代替回调契约。
2. 输入服务地址（Cloud EU/US 或自托管，支持路径前缀）和项目 public/secret API key，
   点击保存、测试连接。重新加载只读取保存的地址和凭据配置状态，不返回任何密钥。
   两个密钥框均留空可保留凭据；替换时必须同时提供。出站受宿主网络策略约束。
3. 导出：选择有权访问的 AgentFlow 或 AgentLogs 来源，填写含时区的 RFC3339 开始时间范围。
   下界包含、上界不含；遍历所有页面，逐条完整导出。回执仅统计 Langfuse 完整确认的记录和 Span。
4. 导入：仅选择 **AgentLogs** 目标。输入多个 Trace ID（逗号、空白或换行分隔），或按日期选择 Trace。
   日期按 Trace 开始时间筛选，下界包含、上界不含。Langfuse v4 `events_only` 明确返回旧端点不可用时，
   使用 v2 observation 遍历所有游标页发现候选；不带日期限制取完整 Trace，以最早 observation 判断是否在区间内。
   每条选中 Trace 的 observation 全部分页取完才导入。接收数、重复数、记录 ID 均来自宿主真实回执；
   只创建历史事实，不创建可执行 AgentFlow run。

页面提供中英文切换。不要在服务 URL 中填写密钥。插件不直接连接宿主 HTTP API 或数据库。
宿主重新验证真实用户的应用权限、工作区和接收开关；页面可见不授予导入权限。

## 回执、完整性和失败

操作受宿主单次调用 deadline、取消和既有 multiplex frame 安全预算约束。插件不添加业务条数或正文大小上限，
不静默截断，不创建后台任务，不隐藏重试。完整源超过传输预算会明确失败；耗时过长可缩小日期范围后重试。

OTLP HTTP 2xx 仍须核验 acknowledgement：有拒绝或警告就不能宣称完整成功。
支持标准 OTLP JSON/protobuf 回执和 Langfuse `otel-ingestion-job` 队列回执；后者确认正文上传和异步队列接收，
不代表处理已完成或 observation 已持久化。队列及认证私有 metadata 不返回、不记录。
失败显示安全错误码、已确认进度及当前步骤的结构化详情。部分导入事件数包含失败步骤已提交量；
宿主未返回失败步骤记录 ID 时，该步骤的已提交记录数单独展示，不伪造完整记录列表。前序步骤可能已经提交；
失败不代表整体回滚。重复导入依靠稳定源身份和宿主去重。远端未确认的导出可能已有不确定提交，
不会计入已确认导出数。

中立 OTLP `1flowbase.agent_log.source_fact` 映射为 Langfuse metadata `1flowbase_agent_log`
单个带版本 base64 叶子，导入时恢复。原始 dotted key、完整长正文、稳定身份、顺序和关系保存在 capsule 中。
仅完整旧 capsule 可兼容；损坏或扁平化有歧义的 self metadata 明确拒绝。
外部 observation 输入/输出先转换为标准 GenAI 消息、工具、用量和 parent span，并保留完整远端原始结构。
`endTime` 不会推断 TaskEnd、最终正文或成功状态。

重复 observation ID 仅有远端 `createdAt`/`updatedAt` 记账字段变化时，折叠为一条完整原始记录：
按 RFC3339 实际时刻优先选最新 `updatedAt`、其次 `createdAt`，相同时按原始表示确定顺序，保留所选行真实时间及 raw。
正文、capsule、用量、observation 时间等其余字段任何差异仍明确报冲突。
普通 404、认证和服务器错误不会触发 `events_only` 日期发现回退。

## API 与开发

API 前缀 `/api/console/managed-services/langfuse`，沿用宿主会话、CSRF 和操作授权：

| 方法 | 路径 | 输入/结果 |
| --- | --- | --- |
| GET | `/config` | `base_url`、`credential_configured`，无密钥 |
| PUT | `/config` | `base_url`，可选成对 `public_key`/`secret_key` |
| POST | `/test` | 项目 API 连通性，不返回项目数据 |
| GET | `/applications` | 可见 `agent_flow` / `agent_logs` 应用 |
| POST | `/records/query` | `application_id`，可选日期、`cursor`、`limit` |
| POST | `/export` | `application_id` + 非空 `record_ids` / `log_conversation_ids` / 双日期三选一 |
| POST | `/import` | AgentLogs `application_id` + 非空 `trace_ids` 或双日期 |

业务 DTO 含 `success`；失败保留 `error.code`、可选 `status` 和 `details`。
地址在宿主 PluginData `connection` 自有集合；两种密钥在 `PluginCredentialClient`。
日志和网络走 SDK typed client 和统一回调，无自有通信解复用器。
旧内置 Langfuse 路由由已安装插件设置页及 API 替代；标准 OTLP、原生归档仍由底座提供。

Cargo SDK 固定公开宿主精确 SHA，装配时更新到配套提交；无提交的本地 path dependency。
TSX 复用宿主 React、Ant Design、`@1flowbase/plugin-settings`，无需 npm 依赖。
`src/_tests/` 包含完整性、标准消息、真实 SDK 回调分页/去重/部分提交/目标拒绝、队列回执、
v4 日期边界与完整 Trace、游标循环、observation 版本与真实冲突 fixture。
三个表单使用不同稳定名称，重复 DTO 字段也能对应唯一的 label 目标；
`ui/_tests/` 是复用已认证宿主页面的浏览器消费 fixture，不自行登录。
所有构建和测试统一交配套 CI/集中 QA，未执行 fixture 不代表验收通过。
现有 manifest 扫描发布流水线负责打包；不手工维护生成 catalog 或历史。
