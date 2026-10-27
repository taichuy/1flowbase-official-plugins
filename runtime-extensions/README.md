# Runtime Extensions

Runtime extensions are executed by the host but implement provider-specific behavior.

Current subtrees:

- `@taichuy/<provider_code>/` for official model provider runtime extensions
- `@taichuy/langfuse/` for the public managed Langfuse log connector and
  `/settings/langfuse` page. The host owns authorization, encrypted credentials,
  log facts and controlled HTTP; the plugin owns vendor mapping and exchange.
- `@taichuy/wechat-chat-channels/` for the managed WeChat notification type.
  The Host owns notification instance authorization, encrypted credentials and send
  logs; the plugin owns Tencent iLink QR login and text delivery. See its bilingual
  README for setup and the upstream conversation-window limitations.
- `@taichuy/codex-logs-collector/` for the native **client** logs collector.
  Client collectors use `collector-manifest.json` and a dedicated `collector-release`
  pipeline. They are downloaded and executed on the user's computer, not installed
  into the server runtime host or its slot registry. Their shared native SDK lives
  in `sdk/agent-logs-collector`; installation instructions are in the collector's
  bilingual README files.

Every runtime manifest declares a required `publisher_namespace`. Official manifests
use `1flowbase`; this publisher identity determines the runtime catalog organization
and ID independently of the repository owner and display/legal `vendor` metadata.
Manifests also publish `slot_codes` and may publish `keywords` (defaulting to an empty
list) for catalog classification and search. Registry publication records the actual
repository-relative manifest path as `manifest_locator`; that source path may use a
different organization segment than `publisher_namespace`.
