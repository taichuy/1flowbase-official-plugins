# Langfuse managed connector

Public Apache-2.0-licensed runtime extension, version **0.1.1**, publisher `1flowbase`.
This package owns Langfuse's API, Basic authentication, pagination, metadata and
settings page. It contains no private commercial plugin source or assets.

## Use

1. Install and activate the official Langfuse runtime package, grant its managed
   operations and required contributions, and open **Settings → Langfuse**
   (`/settings/langfuse`). The host must support `ApplicationLogsV1` and
   `HttpEgressV1` from the paired release. Host version alone does not substitute
   for those callback contracts.
2. Enter the service URL (Cloud EU/US or self-hosted, including any path prefix)
   and the **project's public and secret API keys**, then Save and Test connection.
   Reload retrieves the saved URL and credential-configured flag. Both keys are
   held in the host's credential service and are never returned to the page.
   Leave both key inputs empty to retain existing keys; supply both to replace.
   Outbound traffic is subject to the host's configured network policy.
3. For export, choose a visible AgentFlow or AgentLogs source and an RFC3339
   started-at interval. The lower bound is inclusive and upper bound exclusive.
   All pages are traversed; each selected record is exported in full. The receipt
   counts only records/spans fully acknowledged by Langfuse.
4. For import, choose a visible **AgentLogs** target. Enter multiple trace IDs
   separated by commas/whitespace, or select traces by an RFC3339 date interval.
   Date selection uses trace start time: inclusive lower and exclusive upper bound.
   On Langfuse v4 `events_only`, an explicit legacy endpoint capability response
   switches discovery to all pages of v2 observations. Candidate traces are fetched
   without date filters; their earliest observation must fall in the interval.
   Every selected trace's observations are fetched completely before importing.
   Accepted events, duplicate events and record IDs come from the host; imports
   create historical facts, never executable AgentFlow runs.

The page includes English and Simplified Chinese with an explicit language toggle.
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
prior progress, and the current step's structured host/remote details. Partial import event counts
include the failing step's confirmed commits; its record count is shown separately
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
| GET | `/config` | `base_url`, `credential_configured`; no keys |
| PUT | `/config` | `base_url`, optional paired `public_key`/`secret_key` |
| POST | `/test` | Project API connectivity; no project data returned |
| GET | `/applications` | Visible `agent_flow`/`agent_logs` applications |
| POST | `/records/query` | `application_id`, optional `started_at_from`, `started_at_to`, `cursor`, `limit` |
| POST | `/export` | `application_id` + exactly one nonempty `record_ids`, `log_conversation_ids`, or both date bounds |
| POST | `/import` | AgentLogs `application_id` + nonempty `trace_ids` or both date bounds |

Config URL is a nonsecret plugin-owned `connection` collection. Credentials use
`PluginCredentialClient`. Application and network access use typed SDK clients
and the SDK's shared correlated callbacks, without an independent demultiplexer.
The old built-in Langfuse exchange routes are replaced by this installed settings
page/API; standard OTLP and native archives remain host features.

## Development and validation

The Cargo SDK dependency is pinned to an exact public host revision. Assembly
updates that pin to the paired host commit before CI. No local path dependencies
or npm dependencies are required: the packaged TSX uses host-provided React,
Ant Design and `@1flowbase/plugin-settings`.

`src/_tests/` covers capsule fidelity, external output normalization, truthful
unfinished traces, acknowledgement/pagination validation and real SDK callback
workflows (saved config, all host pages, all remote observation pages, dedup and
partial receipts, queue acceptance and rejection, v4 date boundaries/whole traces,
cursor loops, observation versions and conflicts, foreign targets). The three
forms have separate stable names so repeated DTO field names retain unique label
targets. `ui/_tests/` contains the settings browser
consumer fixture; it uses an already authenticated host page and never signs in.
Run the paired plugin CI/centralized QA; unexecuted fixtures are not passing
runtime evidence. Publishing uses the existing manifest-scanning provider package
pipeline; generated catalogs and release history are not source inputs.
