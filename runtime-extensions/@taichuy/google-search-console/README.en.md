# Google Search Console Plugin (`@taichuy/google-search-console`)

Official 1flowbase runtime extension providing a native Settings UI and an MCP instance (`google_search_console`) for Google Search Console (GSC). Supports one-click Google OAuth 2.0 PKCE popup login, encrypted credential storage with automatic refresh, multi-account & multi-property management, 30-day search overview, Sitemap listing/XML entry inspection/management, Search Analytics queries, and URL Index Inspection.

## Capabilities

- **One-Click Google OAuth 2.0 (PKCE) Login**:
  - Configure your Google Cloud OAuth Client (`client_id` & `client_secret`) once in Settings; secrets are stored exclusively in the 1flowbase Host encrypted credential vault and never returned in API responses.
  - Popup OAuth flow with `BroadcastChannel` / `localStorage` callback relay and automatic same-tab fallback.
  - Defaults to `webmasters.readonly` (`readonly` mode) with an optional `webmasters` (`full`) mode for sitemap submission/deletion and site addition/removal.
- **Managed Persistence & Token Refresh**:
  - Accounts and site snapshots persist in plugin managed tables with single-flight `refresh_token` rotation protected by `PluginExecutionGate`.
- **GSC APIs & MCP Tools (`google_search_console`)**:
  - 15 MCP tools (`plugin.google-search-console.*`) bound under `/gsc` (`accounts`, `sites`, `sitemaps`, `analytics`, `inspection`) covering account listing, site overview/management, sitemaps CRUD + SSRF-guarded XML `<urlset>`/`<sitemapindex>` parsing (`sitemaps_fetch_entries`), Search Analytics queries, and URL inspection.

## Setup

1. Enable **Google Search Console API** in Google Cloud Console.
2. Create an OAuth Client ID (**Web application**) and add the **Authorized Redirect URI** shown in the plugin settings page (e.g., `https://your-host/settings/google-search-console`).
3. Save `Client ID` and `Client Secret` in **Settings → Plugins → Google Search Console**, then click **Sign in with Google**.

## Verification

```bash
cargo test --offline
node scripts/generate.mjs --check
node --test scripts/_tests/manifest.test.mjs
```

