# Langfuse managed connector

Public Apache-2.0-licensed runtime extension, version **0.2.0**, publisher `1flowbase`.
This package owns Langfuse's API, Basic authentication, pagination, metadata and
settings page. It contains no private commercial plugin source or assets.

## Use

1. Install and activate the official Langfuse runtime package, grant its managed
   operations and required contributions, and open **Settings → Langfuse**
   (`/settings/langfuse`). The host must support `ApplicationLogsV1` and
   `HttpEgressV1` from the paired release. Host version alone does not substitute
   for those callback contracts.
2. Select **Add instance**. In the Instance drawer, enter a name, service URL
   (Cloud EU/US or self-hosted, including any path prefix), and the **project's
   public and secret API keys**, then Save. Each saved connection has its own row
   in the shared table, with column preferences and pagination. Names and URLs
   may repeat. Use the row's Edit, Test connection, Export, Import or Delete action.
   Reload retrieves names, URLs and credential-configured flags; keys are held
   in the host credential service and never returned to the page. Key inputs
   are blank when editing: leave both empty to retain only that instance's pair,
   or supply both to replace it. Renaming keeps the stable instance identity
   and credentials. Delete removes only that instance and its saved keys.
   An existing connection appears as the `default` instance. Outbound traffic
   remains subject to the host's configured network policy.
3. Open a row's **Export** drawer and choose a visible AgentFlow or AgentLogs
   source and an RFC3339 started-at interval. The lower bound is inclusive and upper bound exclusive.
   All pages are traversed; each selected record is exported in full. The receipt
   counts only records/spans fully acknowledged by Langfuse.
4. Open a row's **Import** drawer and choose a visible **AgentLogs** target. Enter
   multiple trace IDs separated by commas/whitespace, or select traces by an RFC3339 date interval.
   Date selection uses trace start time: inclusive lower and exclusive upper bound.
   On Langfuse v4 `events_only`, an explicit legacy endpoint capability response
   switches discovery to all pages of v2 observations. Candidate traces are fetched
   without date filters; their earliest observation must fall in the interval.
   Every selected trace's observations are fetched completely before importing.
   Accepted events, duplicate events and record IDs come from the host; imports
   create historical facts, never executable AgentFlow runs.

The page follows the host language setting for English and Simplified Chinese.
Export and import always use the instance selected by the row action.
Do not put API keys in service URLs. The plugin never connects to the host's HTTP
API or database. The host rechecks the actual user's application permissions,
workspace and reception switch on imports; page visibility grants no authority.

## Receipts and failure semantics

Operations are synchronous under the host's per-call deadline/cancellation and
existing multiplex frame budget. There are no plugin business count/body caps,
background jobs or silently truncated facts. An oversized complete source fails
explicitly. Prefer a narrower date interval when a call exceeds its deadline;
there is no hidden automatic retry.

An HTTP 2xx OTLP response is insufficient: its acknowledgement must contain no
rejections or warnings. Standard OTLP JSON/protobuf acknowledgements and the
Langfuse `otel-ingestion-job` queue receipt are supported. The latter confirms
blob upload and queue acceptance for asynchronous processing; it does not confirm
completed persistence. Private queue/auth metadata is never returned or logged.
On failure the page displays the safe error code, confirmed
prior progress, and the current step's reported counts and record IDs.
Reported rejected spans and acknowledgement warnings are shown as labeled fields.
Partial import event counts include the failing step's confirmed commits; its record count is shown separately
when the host cannot return those record IDs. A failed
call may already have committed earlier steps. Repeating imports uses stable
source identities and host deduplication; do not interpret a failed operation as
an automatic rollback. An unacknowledged remote export can have an uncertain
commit state; its spans are not reported as confirmed exports.

Neutral OTLP source capsules (`1flowbase.agent_log.source_fact`) are mapped into
Langfuse metadata `1flowbase_agent_log` as one versioned base64 leaf and restored
on import. Dotted raw keys, long bodies, identity, order and relationships remain
inside the capsule. Intact legacy capsules are accepted; malformed or flattened
ambiguous self metadata fails closed. External Langfuse observations normalize
input/output into standard GenAI messages, tools, usage and parent spans while
retaining the full remote observation as raw OTLP data. `endTime` never fabricates
TaskEnd, final content or successful status.

Repeated observation IDs may differ only in the remote `createdAt`/`updatedAt`
bookkeeping fields. These versions collapse to one complete original row, choosing
the latest `updatedAt`, then `createdAt` by RFC3339 instant, with a deterministic
raw representation tie break. Its actual timestamps and raw data are preserved.
Any other difference, including content, capsule, usage or observation times,
fails explicitly. Generic 404, authentication and server errors never trigger
the `events_only` date-discovery fallback.

## Managed API

All paths below are relative to `/api/console/managed-services/langfuse` and use
host session/CSRF and operation authorization. Each response is a business DTO
with `success`; failures preserve `error.code`, optional `status` and `details`.
The capability transport envelope is independent of that business result.

| Method | Path | Input / result |
| --- | --- | --- |
| GET | `/instances` | Paginated `items`, `page`, `page_size`, `total`; query `page` / `page_size` defaults 1 / 50 |
| POST | `/instances` | `name`, `base_url`, `public_key`, `secret_key`; returns `instance` |
| PUT | `/instances/{instance_id}` | `name`, `base_url`, optional paired keys; returns `instance` |
| DELETE | `/instances/{instance_id}` | Removes that instance and credential |
| POST | `/instances/{instance_id}/test` | Tests that instance against its project API |
| POST | `/openapi/discover` | `instance_id`, optional `search`, `tag`; complete matching operation metadata |
| POST | `/openapi/get` | `instance_id`, optional `operationId`; complete document or operation with parameters / referenced components |
| POST | `/openapi/execute` | `instance_id`, `operationId`, optional `path`, `query`, `body`; actual upstream response |
| GET | `/config` | Legacy `default` connection: `base_url`, `credential_configured`; no keys |
| PUT | `/config` | Legacy `default` only: `base_url`, optional paired `public_key`/`secret_key` |
| POST | `/test` | Legacy `default` project connectivity; no project data returned |
| GET | `/applications` | Visible `agent_flow`/`agent_logs` applications |
| POST | `/records/query` | `application_id`, optional `started_at_from`, `started_at_to`, `cursor`, `limit` |
| POST | `/export` | Optional `instance_id`, `application_id` + exactly one nonempty `record_ids`, `log_conversation_ids`, or both date bounds |
| POST | `/import` | Optional `instance_id`, AgentLogs `application_id` + nonempty `trace_ids` or both date bounds |

Instance DTOs contain exactly `instance_id`, `name`, `base_url` and
`credential_configured`. Pagination defaults do not cap the collection. Omitting
`instance_id` from legacy export/import selects `default`, never the first saved
row. The settings page always sends the selected row's `instance_id`.

OpenAPI requests read only `generated/api/openapi.yml` relative to the selected
instance's URL, preserving its path prefix. Discover lists matching operations;
get returns the document or the selected operation, inherited/overridden
parameters and local reference dependencies. Execute resolves the exact
`operationId` in that document and calls its method/path on the same instance.
Specification `servers` do not override the destination. Simple scalar path
segments and OpenAPI form query values/arrays are encoded by the connector;
unsupported encodings and missing required paths fail explicitly. JSON request
bodies retain their structure. No availability probes, alternate endpoints,
automatic operation substitution, retries or spec cache are introduced.

The execute response includes `response.status`, `headers`, authoritative
`body_base64` bytes and parsed JSON `body` when possible. Upstream 4xx/5xx remain
upstream responses, not fabricated success. Local validation, credential or host
transport errors use the normal safe failure DTO. Callers cannot override the
Authorization header, arbitrary headers or destination URL. The fixed operations
`openapi.discover`, `openapi.get`, `openapi.execute` and `instances.list` use
the existing managed-service authorization and host `/plugins/langfuse` MCP
projection (tool IDs `plugin.langfuse.<operation name>`);
upstream operations are discovered on demand rather than generated as tools.

Names and URLs live in the nonsecret plugin-owned `connection` collection.
Credentials use `PluginCredentialClient`: `langfuse_project` for `default`,
`langfuse_instance:<instance_id>` for every other instance. Application and network
access use typed SDK clients and the SDK's shared correlated callbacks, without an independent demultiplexer.
The old built-in Langfuse exchange routes are replaced by this installed settings
page/API; standard OTLP and native archives remain host features.

## Development and validation

The Cargo SDK dependency is pinned to an exact public host revision. Assembly
updates that pin to the paired host commit before CI. No local path dependencies
or npm dependencies are required: the packaged TSX uses host-provided React,
Ant Design, `@1flowbase/plugin-settings` and the public `@1flowbase/data-table`
module (layout, desktop/mobile rows, column preferences and pagination).

The OpenAPI integration declares `serde_yaml = "0.9.34"`, locked to
`0.9.34+deprecated`, to parse the selected instance's `generated/api/openapi.yml`
into JSON for discover/get/execute. The lock retains the upstream `deprecated`
version marker. Its resolved dependencies include `ryu 1.0.23` and
`unsafe-libyaml 0.2.11`; these versions reuse the existing host/cache dependency
closure. The already indirect `uuid 1.28.0` is now declared directly with `v4`
for stable generated instance IDs. This change upgrades no existing shared
external dependencies and adds no npm dependencies.

`src/_tests/` covers capsule fidelity, external output normalization, truthful
unfinished traces, acknowledgement/pagination validation and real SDK callback
workflows (saved config, all host pages, all remote observation pages, dedup and
partial receipts, queue acceptance and rejection, v4 date boundaries/whole traces,
cursor loops, observation versions and conflicts, foreign targets). The instance,
export and import drawers have separate stable form names so repeated DTO fields retain unique
label targets. `ui/_tests/` contains the settings browser consumer fixture for
instance isolation, blank keys, stable rename, row-bound exchange, date selection,
partial/dedup receipts and host locale changes; it uses an already authenticated
host page and never signs in. Central QA restores the original user locale preference.
Run the paired plugin CI/centralized QA; unexecuted fixtures are not passing
runtime evidence. This acceptance uses a CI-built package uploaded and installed
with the paired host. Langfuse has not been listed in the official registry.
The existing automatic release discovery skips this manifest because it has
`slot_codes: []` and no `notification_types`; it does not publish this connector
automatically. Generated catalogs and release history are not source inputs.
