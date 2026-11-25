import { request } from '@1flowbase/plugin-settings';

export type ScopeMode = 'readonly' | 'full';

export type OAuthConfig = {
  configured: boolean;
  auth_mode: string;
  client_id: string;
  has_client_secret: boolean;
  default_scope_mode: ScopeMode;
  updated_at: string | null;
};

export type AccountSummary = {
  account_id: string;
  email: string;
  display_name: string;
  avatar_url: string | null;
  scope_mode: ScopeMode;
  granted_scopes: string[];
  enabled: boolean;
  status: string;
  last_error: string | null;
  has_credential: boolean;
  site_count: number;
  last_synced_at: string | null;
  created_at: string;
  updated_at: string;
};

export type AccountList = {
  items: AccountSummary[];
  total: number;
  page: number;
  page_size: number;
};

export type SiteSummary = {
  site_record_id: string;
  account_id: string;
  site_url: string;
  property_type: string;
  permission_level: string;
  sitemap_count: number | null;
  last_synced_at: string;
};

export type SitemapInfo = {
  path?: string;
  lastSubmitted?: string;
  isPending?: boolean;
  isSitemapsIndex?: boolean;
  type?: string;
  lastDownloaded?: string;
  warnings?: string | number;
  errors?: string | number;
  contents?: Array<{ type?: string; submitted?: string | number; indexed?: string | number }>;
};

export type SitemapEntry = {
  loc: string;
  lastmod?: string | null;
  changefreq?: string | null;
  priority?: string | null;
  source_sitemap?: string | null;
};

export type SitemapEntriesResult = {
  kind: 'urlset' | 'sitemapindex';
  site_url: string;
  feedpath: string;
  total_found: number;
  returned_count: number;
  truncated: boolean;
  child_sitemaps: string[];
  entries: SitemapEntry[];
};

const root = '/api/console/managed-services/google-search-console';
const acctPath = (accountId: string) => `/accounts/${encodeURIComponent(accountId)}`;

const call = <T,>(
  path: string,
  method: 'GET' | 'POST' | 'PUT' | 'DELETE',
  signal?: AbortSignal,
  body?: unknown
) => request<T>({ path: `${root}${path}`, method, body, signal });

export const gscApi = {
  getOAuthConfig: (signal?: AbortSignal) => call<OAuthConfig>('/oauth-config', 'GET', signal),
  saveOAuthConfig: (
    body: { client_id: string; client_secret?: string; default_scope_mode: ScopeMode },
    signal?: AbortSignal
  ) => call<OAuthConfig>('/oauth-config', 'PUT', signal, body),
  startOAuth: (
    body: { redirect_uri: string; scope_mode?: ScopeMode; reauth_account_id?: string },
    signal?: AbortSignal
  ) => call<{ authorization_url: string; expires_in_seconds: number; scope_mode: ScopeMode }>(
    '/oauth/start',
    'POST',
    signal,
    body
  ),
  completeOAuth: (body: { code: string; state: string }, signal?: AbortSignal) =>
    call<{ account: AccountSummary; sites: SiteSummary[]; site_count: number; sync_error: string | null }>(
      '/oauth/complete',
      'POST',
      signal,
      body
    ),
  listAccounts: (page: number, page_size: number, search: string, signal?: AbortSignal) => {
    const query = new URLSearchParams({ page: String(page), page_size: String(page_size) });
    if (search) query.set('search', search);
    return call<AccountList>(`/accounts?${query}`, 'GET', signal);
  },
  updateAccount: (accountId: string, enabled: boolean, signal?: AbortSignal) =>
    call<{ account: AccountSummary }>(acctPath(accountId), 'PUT', signal, { enabled }),
  testAccount: (accountId: string, signal?: AbortSignal) =>
    call<{ authenticated: true; status: string; site_count: number; checked_at: string | null }>(
      `${acctPath(accountId)}/test`,
      'POST',
      signal
    ),
  deleteAccount: (accountId: string, signal?: AbortSignal) =>
    call<{ deleted: true }>(acctPath(accountId), 'DELETE', signal),
  listSites: (accountId: string, signal?: AbortSignal) =>
    call<{ items: SiteSummary[]; total: number; page: number; page_size: number }>(
      `/sites?${new URLSearchParams({ account_id: accountId, page: '1', page_size: '100' })}`,
      'GET',
      signal
    ),
  syncSites: (accountId: string, signal?: AbortSignal) =>
    call<{ account_id: string; items: SiteSummary[]; total: number; synced_at: string | null }>(
      `${acctPath(accountId)}/sites/sync`,
      'POST',
      signal
    ),
  listSitemaps: (accountId: string, siteUrl: string, signal?: AbortSignal) =>
    call<{ site_url: string; sitemaps: SitemapInfo[] }>(
      `${acctPath(accountId)}/sitemaps?${new URLSearchParams({ site_url: siteUrl })}`,
      'GET',
      signal
    ),
  submitSitemap: (accountId: string, siteUrl: string, feedpath: string, signal?: AbortSignal) =>
    call<{ submitted: true }>(
      `${acctPath(accountId)}/sitemaps`,
      'PUT',
      signal,
      { site_url: siteUrl, feedpath }
    ),
  fetchSitemapEntries: (
    accountId: string,
    siteUrl: string,
    feedpath: string,
    followIndex: boolean,
    signal?: AbortSignal
  ) =>
    call<SitemapEntriesResult>(
      `${acctPath(accountId)}/sitemaps/entries`,
      'POST',
      signal,
      { site_url: siteUrl, feedpath, follow_index: followIndex, max_entries: 200 }
    ),
};

export function extractErrorCode(failure: unknown): string {
  if (failure && typeof failure === 'object') {
    const obj = failure as { code?: unknown; data?: { code?: unknown } };
    if (typeof obj.code === 'string' && obj.code.length > 0) return obj.code;
    if (obj.data && typeof obj.data.code === 'string' && obj.data.code.length > 0) return obj.data.code;
  }
  return 'request_failed';
}
