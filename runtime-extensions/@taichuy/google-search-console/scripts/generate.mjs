import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

export const pluginRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const version = '0.1.4';
const minHost = '0.6.6';
const pluginId = 'google-search-console';
const instanceId = 'google_search_console';
const apiRoot = `/api/console/managed-services/${pluginId}`;

const obj = (properties = {}, required = [], extra = {}) => ({
  type: 'object',
  properties,
  required,
  additionalProperties: false,
  ...extra,
});
const str = (description, extra = {}) => ({
  type: 'string',
  minLength: 1,
  ...(description ? { description } : {}),
  ...extra,
});
const nullable = (schema) => {
  const clone = structuredClone(schema);
  if (typeof clone.type === 'string') {
    clone.type = [clone.type, 'null'];
  }
  return clone;
};
const input = (pathProps = {}, pathReq = [], query = obj(), body = obj()) =>
  obj({ path: obj(pathProps, pathReq), query, body }, ['path', 'query', 'body']);
const hash = (text) => `sha256:${createHash('sha256').update(text).digest('hex')}`;

const accountIdProp = str('Connected Google account ID. Obtain it from accounts_list.');
const siteUrlProp = str('GSC property URL, e.g. "https://1flowbase.taichuy.com/" (with trailing slash) or "sc-domain:example.com".');
const feedpathProp = str('Full sitemap URL, e.g. "https://1flowbase.taichuy.com/sitemap.xml".');
const scopeEnum = { type: 'string', enum: ['readonly', 'full'] };

const oauthConfigSchema = obj({
  configured: { type: 'boolean' },
  auth_mode: { type: 'string' },
  client_id: { type: 'string' },
  has_client_secret: { type: 'boolean' },
  default_scope_mode: scopeEnum,
  updated_at: { type: ['string', 'null'] },
}, ['configured', 'auth_mode', 'client_id', 'has_client_secret', 'default_scope_mode', 'updated_at']);

const accountSchema = obj({
  account_id: accountIdProp,
  email: str('Google account email.'),
  display_name: str('Google account display name.'),
  avatar_url: nullable(str()),
  scope_mode: scopeEnum,
  granted_scopes: { type: 'array', items: { type: 'string' } },
  enabled: { type: 'boolean' },
  status: { type: 'string', enum: ['active', 'reauth_required', 'error'] },
  last_error: nullable(str()),
  has_credential: { type: 'boolean' },
  site_count: { type: 'integer', minimum: 0 },
  last_synced_at: nullable(str()),
  created_at: str(),
  updated_at: str(),
}, ['account_id', 'email', 'display_name', 'scope_mode', 'granted_scopes', 'enabled', 'status', 'has_credential', 'site_count', 'created_at', 'updated_at']);

const siteSchema = obj({
  site_record_id: str(),
  account_id: accountIdProp,
  site_url: siteUrlProp,
  property_type: { type: 'string', enum: ['url_prefix', 'domain'] },
  permission_level: str(),
  sitemap_count: { type: ['integer', 'null'] },
  last_synced_at: str(),
}, ['site_record_id', 'account_id', 'site_url', 'property_type', 'permission_level', 'last_synced_at']);

const metricRowSchema = obj({
  keys: { type: 'array', items: { type: 'string' } },
  clicks: { type: 'number' },
  impressions: { type: 'number' },
  ctr: { type: 'number' },
  position: { type: 'number' },
}, ['clicks', 'impressions', 'ctr', 'position'], { additionalProperties: true });

const totalsSchema = obj({
  clicks: { type: 'number' },
  impressions: { type: 'number' },
  ctr: { type: 'number' },
  position: { type: 'number' },
}, ['clicks', 'impressions', 'ctr', 'position']);

const sitemapSchema = {
  type: 'object',
  properties: {
    path: { type: 'string' },
    lastSubmitted: { type: 'string' },
    isPending: { type: 'boolean' },
    isSitemapsIndex: { type: 'boolean' },
    type: { type: 'string' },
    lastDownloaded: { type: 'string' },
    warnings: { type: ['string', 'integer'] },
    errors: { type: ['string', 'integer'] },
    contents: { type: 'array', items: { type: 'object', additionalProperties: true } },
  },
  additionalProperties: true,
};

const rec = (action_code, selector) => ({ resource_code: 'accounts', action_code, selector });

function buildOperations() {
  return [
    {
      id: 'oauth_config.get',
      method: 'GET',
      path: '/oauth-config',
      summary: 'Get Google OAuth configuration',
      description: 'Read configured Google OAuth Client ID and default scope mode. Client secrets are never returned.',
      concurrency: 'parallel',
      input_schema: input(),
      output_schema: oauthConfigSchema,
    },
    {
      id: 'oauth_config.save',
      method: 'PUT',
      path: '/oauth-config',
      summary: 'Save Google OAuth configuration',
      description: 'Save Google OAuth Client ID, encrypted Client Secret, and default scope mode.',
      concurrency: 'serialized',
      input_schema: input({}, [], obj(), obj({
        client_id: str('Google Cloud OAuth 2.0 Web Client ID.'),
        client_secret: { type: 'string', writeOnly: true, format: 'password', description: 'Omit or leave blank when editing to keep existing secret.' },
        default_scope_mode: scopeEnum,
      }, ['client_id'])),
      output_schema: oauthConfigSchema,
    },
    {
      id: 'oauth.start',
      method: 'POST',
      path: '/oauth/start',
      summary: 'Start Google OAuth flow',
      description: 'Generate a PKCE S256 Google authorization URL bound to the current caller and workspace.',
      concurrency: 'parallel',
      record_permission: rec('manage', { kind: 'record', pointer: '/body/reauth_account_id', allow_missing: true }),
      input_schema: input({}, [], obj(), obj({
        redirect_uri: str('Exact settings callback URL ending with /settings/google-search-console.'),
        scope_mode: scopeEnum,
        reauth_account_id: str('Optional account ID when re-authorizing an existing account.'),
      }, ['redirect_uri'])),
      output_schema: obj({
        authorization_url: str(),
        expires_in_seconds: { type: 'integer' },
        scope_mode: scopeEnum,
      }, ['authorization_url', 'expires_in_seconds', 'scope_mode']),
    },
    {
      id: 'oauth.complete',
      method: 'POST',
      path: '/oauth/complete',
      summary: 'Complete Google OAuth callback',
      description: 'Verify state, exchange authorization code with PKCE, store encrypted refresh token, and sync verified sites.',
      concurrency: 'serialized',
      record_permission: rec('manage', { kind: 'record', pointer: '/body/account_id', allow_missing: true }),
      input_schema: input({}, [], obj(), obj({
        code: str('Authorization code returned by Google.'),
        state: str('One-time OAuth state returned by Google.'),
        account_id: str('Optional account ID scope constraint.'),
      }, ['code', 'state'])),
      output_schema: obj({
        account: accountSchema,
        sites: { type: 'array', items: siteSchema },
        site_count: { type: 'integer', minimum: 0 },
        sync_error: nullable(str()),
      }, ['account', 'sites', 'site_count', 'sync_error']),
    },
    {
      id: 'accounts.list',
      method: 'GET',
      path: '/accounts',
      summary: 'List connected Google Search Console accounts',
      description: 'List connected Google Search Console accounts accessible to the current role. Tokens and credentials are never returned.',
      concurrency: 'parallel',
      record_permission: rec('view', { kind: 'list' }),
      mcp: { group: '/gsc/accounts', name: 'accounts_list', risk: 'low' },
      input_schema: input({}, [], obj({
        page: { type: 'integer', minimum: 1, description: 'Page number starting at 1.' },
        page_size: { type: 'integer', minimum: 1, maximum: 100, description: 'Items per page (1-100).' },
        search: { type: 'string', description: 'Filter by email or display name.' },
      })),
      output_schema: obj({
        items: { type: 'array', items: accountSchema },
        total: { type: 'integer', minimum: 0 },
        page: { type: 'integer', minimum: 1 },
        page_size: { type: 'integer', minimum: 1 },
      }, ['items', 'total', 'page', 'page_size']),
    },
    {
      id: 'accounts.get',
      method: 'GET',
      path: '/accounts/{account_id}',
      summary: 'Get Google Search Console account',
      description: 'Get metadata and connection status for a connected Google Search Console account.',
      concurrency: 'parallel',
      record_permission: rec('view', { kind: 'record', pointer: '/path/account_id' }),
      mcp: { group: '/gsc/accounts', name: 'accounts_get', risk: 'low' },
      input_schema: input({ account_id: accountIdProp }, ['account_id']),
      output_schema: obj({ account: accountSchema }, ['account']),
    },
    {
      id: 'accounts.update',
      method: 'PUT',
      path: '/accounts/{account_id}',
      summary: 'Update Google account enabled state',
      description: 'Enable or disable a connected Google Search Console account.',
      concurrency: 'serialized',
      record_permission: rec('manage', { kind: 'record', pointer: '/path/account_id' }),
      input_schema: input({ account_id: accountIdProp }, ['account_id'], obj(), obj({
        enabled: { type: 'boolean' },
      }, ['enabled'])),
      output_schema: obj({ account: accountSchema }, ['account']),
    },
    {
      id: 'accounts.test',
      method: 'POST',
      path: '/accounts/{account_id}/test',
      summary: 'Test Google Search Console account connection',
      description: 'Force an OAuth token refresh check and sync verified sites for this account.',
      concurrency: 'serialized',
      record_permission: rec('manage', { kind: 'record', pointer: '/path/account_id' }),
      input_schema: input({ account_id: accountIdProp }, ['account_id']),
      output_schema: obj({
        authenticated: { const: true },
        status: { type: 'string' },
        site_count: { type: 'integer', minimum: 0 },
        checked_at: nullable(str()),
      }, ['authenticated', 'status', 'site_count', 'checked_at']),
    },
    {
      id: 'accounts.delete',
      method: 'DELETE',
      path: '/accounts/{account_id}',
      summary: 'Delete Google Search Console account',
      description: 'Remove a connected Google account, its encrypted refresh token, and cached sites.',
      concurrency: 'serialized',
      record_permission: rec('delete', { kind: 'record', pointer: '/path/account_id' }),
      input_schema: input({ account_id: accountIdProp }, ['account_id']),
      output_schema: obj({ deleted: { type: 'boolean' } }, ['deleted']),
    },
    {
      id: 'sites.list',
      method: 'GET',
      path: '/sites',
      summary: 'List synced Search Console sites',
      description: 'List cached Search Console properties across accessible Google accounts, or filter by account_id.',
      concurrency: 'parallel',
      record_permission: rec('view', { kind: 'list' }),
      mcp: { group: '/gsc/sites', name: 'sites_list', risk: 'low' },
      input_schema: input({}, [], obj({
        account_id: str('Optional connected Google account ID filter.'),
        search: { type: 'string', description: 'Filter by site_url substring.' },
        page: { type: 'integer', minimum: 1 },
        page_size: { type: 'integer', minimum: 1, maximum: 100 },
      })),
      output_schema: obj({
        items: { type: 'array', items: siteSchema },
        total: { type: 'integer', minimum: 0 },
        page: { type: 'integer', minimum: 1 },
        page_size: { type: 'integer', minimum: 1 },
      }, ['items', 'total', 'page', 'page_size']),
    },
    {
      id: 'sites.sync',
      method: 'POST',
      path: '/accounts/{account_id}/sites/sync',
      summary: 'Sync verified sites from Google Search Console',
      description: 'Fetch the latest verified sites list from Google Search Console API for an account and update cached sites.',
      concurrency: 'serialized',
      record_permission: rec('manage', { kind: 'record', pointer: '/path/account_id' }),
      mcp: { group: '/gsc/sites', name: 'sites_sync', risk: 'medium' },
      input_schema: input({ account_id: accountIdProp }, ['account_id']),
      output_schema: obj({
        account_id: accountIdProp,
        items: { type: 'array', items: siteSchema },
        total: { type: 'integer', minimum: 0 },
        synced_at: nullable(str()),
      }, ['account_id', 'items', 'total', 'synced_at']),
    },
    {
      id: 'sites.get',
      method: 'GET',
      path: '/accounts/{account_id}/sites/detail',
      summary: 'Get Search Console site details',
      description: 'Retrieve live permission level and metadata for a specific Search Console property.',
      concurrency: 'parallel',
      record_permission: rec('view', { kind: 'record', pointer: '/path/account_id' }),
      mcp: { group: '/gsc/sites', name: 'sites_get', risk: 'low' },
      input_schema: input({ account_id: accountIdProp }, ['account_id'], obj({ site_url: siteUrlProp }, ['site_url'])),
      output_schema: obj({
        site_url: siteUrlProp,
        property_type: { type: 'string', enum: ['url_prefix', 'domain'] },
        permission_level: str(),
      }, ['site_url', 'property_type', 'permission_level']),
    },
    {
      id: 'sites.add',
      method: 'PUT',
      path: '/accounts/{account_id}/sites',
      summary: 'Add Search Console site property',
      description: 'Add a site to the Google Search Console account. Requires full webmasters scope.',
      concurrency: 'serialized',
      record_permission: rec('manage', { kind: 'record', pointer: '/path/account_id' }),
      mcp: { group: '/gsc/sites', name: 'sites_add', risk: 'high' },
      input_schema: input({ account_id: accountIdProp }, ['account_id'], obj(), obj({ site_url: siteUrlProp }, ['site_url'])),
      output_schema: obj({ added: { type: 'boolean' }, site: siteSchema }, ['added', 'site']),
    },
    {
      id: 'sites.delete',
      method: 'DELETE',
      path: '/accounts/{account_id}/sites',
      summary: 'Remove Search Console site property',
      description: 'Remove a site property from the Google Search Console account. Requires full webmasters scope.',
      concurrency: 'serialized',
      record_permission: rec('manage', { kind: 'record', pointer: '/path/account_id' }),
      mcp: { group: '/gsc/sites', name: 'sites_delete', risk: 'high' },
      input_schema: input({ account_id: accountIdProp }, ['account_id'], obj({ site_url: siteUrlProp }, ['site_url'])),
      output_schema: obj({ deleted: { type: 'boolean' }, site_url: siteUrlProp }, ['deleted', 'site_url']),
    },
    {
      id: 'sites.overview',
      method: 'POST',
      path: '/accounts/{account_id}/sites/overview',
      summary: 'Get comprehensive SEO overview for a site',
      description: 'Aggregate site metadata, sitemaps, period-over-period search performance totals, daily trend, top queries, top pages, and device breakdown in one call.',
      concurrency: 'parallel',
      record_permission: rec('query', { kind: 'record', pointer: '/path/account_id' }),
      mcp: { group: '/gsc/sites', name: 'sites_overview', risk: 'low' },
      input_schema: input({ account_id: accountIdProp }, ['account_id'], obj(), obj({
        site_url: siteUrlProp,
        days: { type: 'integer', minimum: 1, maximum: 480, description: 'Number of days to summarize (default 28, ending 2 days ago).' },
        top_limit: { type: 'integer', minimum: 1, maximum: 100, description: 'Max top queries/pages to return (default 20).' },
        search_type: { type: 'string', enum: ['web', 'image', 'video', 'news', 'discover', 'googleNews'], description: 'Search surface type (default web).' },
      }, ['site_url'])),
      output_schema: obj({
        site: obj({ site_url: siteUrlProp, property_type: { type: 'string' }, permission_level: str() }, ['site_url', 'property_type', 'permission_level']),
        window: obj({
          days: { type: 'integer' },
          search_type: { type: 'string' },
          start_date: str(),
          end_date: str(),
          previous_start_date: str(),
          previous_end_date: str(),
        }, ['days', 'search_type', 'start_date', 'end_date', 'previous_start_date', 'previous_end_date']),
        totals: totalsSchema,
        previous_totals: totalsSchema,
        sitemaps: { type: 'array', items: sitemapSchema },
        daily_trend: { type: 'array', items: metricRowSchema },
        top_queries: { type: 'array', items: metricRowSchema },
        top_pages: { type: 'array', items: metricRowSchema },
        devices: { type: 'array', items: metricRowSchema },
      }, ['site', 'window', 'totals', 'previous_totals', 'sitemaps', 'daily_trend', 'top_queries', 'top_pages', 'devices']),
    },
    {
      id: 'sitemaps.list',
      method: 'GET',
      path: '/accounts/{account_id}/sitemaps',
      summary: 'List sitemaps for a site',
      description: 'List all sitemaps registered in Google Search Console for a site along with status, warnings, errors, and indexed counts.',
      concurrency: 'parallel',
      record_permission: rec('view', { kind: 'record', pointer: '/path/account_id' }),
      mcp: { group: '/gsc/sitemaps', name: 'sitemaps_list', risk: 'low' },
      input_schema: input({ account_id: accountIdProp }, ['account_id'], obj({
        site_url: siteUrlProp,
        sitemap_index: { type: 'string', description: 'Optional sitemap index URL to list child sitemaps.' },
      }, ['site_url'])),
      output_schema: obj({ site_url: siteUrlProp, sitemaps: { type: 'array', items: sitemapSchema } }, ['site_url', 'sitemaps']),
    },
    {
      id: 'sitemaps.get',
      method: 'GET',
      path: '/accounts/{account_id}/sitemaps/detail',
      summary: 'Get sitemap details',
      description: 'Retrieve detailed status and submission metrics for a specific sitemap in Google Search Console.',
      concurrency: 'parallel',
      record_permission: rec('view', { kind: 'record', pointer: '/path/account_id' }),
      mcp: { group: '/gsc/sitemaps', name: 'sitemaps_get', risk: 'low' },
      input_schema: input({ account_id: accountIdProp }, ['account_id'], obj({
        site_url: siteUrlProp,
        feedpath: feedpathProp,
      }, ['site_url', 'feedpath'])),
      output_schema: obj({ site_url: siteUrlProp, sitemap: sitemapSchema }, ['site_url', 'sitemap']),
    },
    {
      id: 'sitemaps.submit',
      method: 'PUT',
      path: '/accounts/{account_id}/sitemaps',
      summary: 'Submit sitemap to Google Search Console',
      description: 'Submit or resubmit a sitemap URL for a site. Requires full webmasters scope.',
      concurrency: 'serialized',
      record_permission: rec('manage', { kind: 'record', pointer: '/path/account_id' }),
      mcp: { group: '/gsc/sitemaps', name: 'sitemaps_submit', risk: 'medium' },
      input_schema: input({ account_id: accountIdProp }, ['account_id'], obj(), obj({
        site_url: siteUrlProp,
        feedpath: feedpathProp,
      }, ['site_url', 'feedpath'])),
      output_schema: obj({ submitted: { type: 'boolean' }, site_url: siteUrlProp, feedpath: feedpathProp }, ['submitted', 'site_url', 'feedpath']),
    },
    {
      id: 'sitemaps.delete',
      method: 'DELETE',
      path: '/accounts/{account_id}/sitemaps',
      summary: 'Delete sitemap from Google Search Console',
      description: 'Remove a sitemap submission record from Google Search Console. Requires full webmasters scope.',
      concurrency: 'serialized',
      record_permission: rec('manage', { kind: 'record', pointer: '/path/account_id' }),
      mcp: { group: '/gsc/sitemaps', name: 'sitemaps_delete', risk: 'high' },
      input_schema: input({ account_id: accountIdProp }, ['account_id'], obj({
        site_url: siteUrlProp,
        feedpath: feedpathProp,
      }, ['site_url', 'feedpath'])),
      output_schema: obj({ deleted: { type: 'boolean' }, site_url: siteUrlProp, feedpath: feedpathProp }, ['deleted', 'site_url', 'feedpath']),
    },
    {
      id: 'sitemaps.fetch_entries',
      method: 'POST',
      path: '/accounts/{account_id}/sitemaps/entries',
      summary: 'Fetch and parse URLs inside a sitemap XML',
      description: 'Safely fetch a verified site sitemap XML (urlset or sitemapindex) over Host Network Egress and extract page URLs (<loc>, <lastmod>, <changefreq>, <priority>).',
      concurrency: 'parallel',
      record_permission: rec('view', { kind: 'record', pointer: '/path/account_id' }),
      mcp: { group: '/gsc/sitemaps', name: 'sitemaps_fetch_entries', risk: 'low' },
      input_schema: input({ account_id: accountIdProp }, ['account_id'], obj(), obj({
        site_url: siteUrlProp,
        feedpath: feedpathProp,
        follow_index: { type: 'boolean', description: 'When true and feedpath is a sitemapindex, expand child sitemaps (default false).' },
        max_entries: { type: 'integer', minimum: 1, maximum: 5000, description: 'Maximum entries to return (default 500, max 5000).' },
        max_child_sitemaps: { type: 'integer', minimum: 1, maximum: 20, description: 'Max child sitemaps to fetch when follow_index is true (default 5, max 20).' },
      }, ['site_url', 'feedpath'])),
      output_schema: obj({
        kind: { type: 'string', enum: ['urlset', 'sitemapindex'] },
        site_url: siteUrlProp,
        feedpath: feedpathProp,
        total_found: { type: 'integer', minimum: 0 },
        returned_count: { type: 'integer', minimum: 0 },
        truncated: { type: 'boolean' },
        child_sitemaps: { type: 'array', items: { type: 'string' } },
        entries: {
          type: 'array',
          items: obj({
            loc: str(),
            lastmod: nullable(str()),
            changefreq: nullable(str()),
            priority: nullable(str()),
            source_sitemap: nullable(str()),
          }, ['loc']),
        },
      }, ['kind', 'site_url', 'feedpath', 'total_found', 'returned_count', 'truncated', 'child_sitemaps', 'entries']),
    },
    {
      id: 'analytics.query',
      method: 'POST',
      path: '/accounts/{account_id}/search-analytics/query',
      summary: 'Query Google Search Analytics',
      description: 'Query clicks, impressions, CTR, and average position across dimensions (query, page, date, country, device, searchAppearance) with optional filters and pagination.',
      concurrency: 'parallel',
      record_permission: rec('query', { kind: 'record', pointer: '/path/account_id' }),
      mcp: { group: '/gsc/analytics', name: 'search_analytics_query', risk: 'low' },
      input_schema: input({ account_id: accountIdProp }, ['account_id'], obj(), obj({
        site_url: siteUrlProp,
        start_date: str('Start date in YYYY-MM-DD format.'),
        end_date: str('End date in YYYY-MM-DD format.'),
        dimensions: {
          type: 'array',
          items: { type: 'string', enum: ['country', 'device', 'page', 'query', 'searchAppearance', 'date'] },
          description: 'Dimensions to group results by.',
        },
        type: { type: 'string', enum: ['web', 'image', 'video', 'news', 'discover', 'googleNews'], description: 'Search type filter (default web).' },
        aggregation_type: { type: 'string', enum: ['auto', 'byPage', 'byProperty', 'byNewsShowcasePanel'] },
        dimension_filter_groups: {
          type: 'array',
          description: 'Optional Search Console API dimensionFilterGroups array.',
          items: {
            type: 'object',
            properties: {
              groupType: { type: 'string', enum: ['and'] },
              filters: {
                type: 'array',
                items: {
                  type: 'object',
                  properties: {
                    dimension: { type: 'string', enum: ['country', 'device', 'page', 'query', 'searchAppearance'] },
                    operator: { type: 'string', enum: ['contains', 'equals', 'notContains', 'notEquals', 'includingRegex', 'excludingRegex'] },
                    expression: { type: 'string' },
                  },
                  required: ['dimension', 'operator', 'expression'],
                },
              },
            },
          },
        },
        row_limit: { type: 'integer', minimum: 1, maximum: 25000, description: 'Max rows to return (1-25000, default 1000).' },
        start_row: { type: 'integer', minimum: 0, description: 'Zero-based pagination offset.' },
        data_state: { type: 'string', enum: ['final', 'all'], description: 'Use "all" to include fresh unfinalized data.' },
      }, ['site_url', 'start_date', 'end_date'])),
      output_schema: obj({
        site_url: siteUrlProp,
        rows: { type: 'array', items: metricRowSchema },
        response_aggregation_type: { type: ['string', 'null'] },
      }, ['site_url', 'rows']),
    },
    {
      id: 'inspection.inspect',
      method: 'POST',
      path: '/accounts/{account_id}/url-inspection/inspect',
      summary: 'Inspect URL index status in Google',
      description: 'Inspect the live Google index status, crawl status, canonical URL, and rich results for a URL (Google quota: 2,000/day per site).',
      concurrency: 'parallel',
      record_permission: rec('query', { kind: 'record', pointer: '/path/account_id' }),
      mcp: { group: '/gsc/inspection', name: 'url_inspect', risk: 'low' },
      input_schema: input({ account_id: accountIdProp }, ['account_id'], obj(), obj({
        site_url: siteUrlProp,
        inspection_url: str('Full URL inside the site property to inspect.'),
        language_code: str('Optional BCP-47 language code for localized issue messages, e.g. "zh-CN" or "en-US".'),
      }, ['site_url', 'inspection_url'])),
      output_schema: obj({
        site_url: siteUrlProp,
        inspection_url: str(),
        inspection_result: { type: 'object', additionalProperties: true },
      }, ['site_url', 'inspection_url', 'inspection_result']),
    },
  ].map(({ id, mcp, concurrency, ...op }) => ({
    ...op,
    operation_id: id,
    interface_id: `${pluginId}.${id}`,
    contribution_id: `${pluginId}.${id}`,
    handler: id,
    path: `${apiRoot}${op.path}`,
    execution_concurrency: concurrency,
    mcp,
  }));
}

function mcpArguments(inputSchema, method) {
  const fields = {};
  const required = [];
  const mappings = [];
  const descriptors = [];
  const add = (name, schema, location, isReq) => {
    if (fields[name]) throw new Error(`Duplicate MCP parameter name: ${name}`);
    fields[name] = structuredClone(schema);
    if (isReq) required.push(name);
    mappings.push({
      interface_param: name,
      mcp_param: name,
      source: { kind: 'mcp_argument', path: name },
      required: isReq,
      description: schema.description ?? `Parameter ${name} (${location}).`,
    });
    descriptors.push({
      name,
      field_type: Array.isArray(schema.type) ? schema.type[0] : (schema.type ?? 'object'),
      parameter_type: location === 'body' ? 'json_body' : 'url',
      description: schema.description ?? '',
      required: isReq,
      schema: structuredClone(schema),
    });
  };
  for (const location of ['path', 'query']) {
    const loc = inputSchema.properties[location];
    for (const [name, schema] of Object.entries(loc.properties ?? {})) {
      add(name, schema, location, location === 'path' || (loc.required ?? []).includes(name));
    }
  }
  if (!['GET', 'DELETE', 'HEAD'].includes(method)) {
    const body = inputSchema.properties.body;
    for (const [name, schema] of Object.entries(body.properties ?? {})) {
      add(name, schema, 'body', (body.required ?? []).includes(name));
    }
  }
  return {
    parameter_schema_snapshot: obj(fields, required),
    input_mapping: { interface_parameters: descriptors, mappings },
  };
}

function buildRecordResources(operations) {
  const actionMeta = {
    view: { label: 'View', description: 'List and inspect connected Google accounts, synced sites, sitemaps, and sitemap XML entries.' },
    query: { label: 'Query analytics & inspect URLs', description: 'Run Search Analytics queries, site overviews, and URL Inspection on this account.' },
    manage: { label: 'Manage & sync', description: 'Re-authorize, toggle enabled state, test connection, sync sites, and manage sites or sitemaps.' },
    delete: { label: 'Delete', description: 'Disconnect and delete this Google account and its saved OAuth refresh token.' },
  };
  const buckets = { view: [], query: [], manage: [], delete: [] };
  for (const op of operations) {
    if (op.record_permission) {
      buckets[op.record_permission.action_code].push(op.interface_id);
    }
  }
  return [{
    resource_code: 'accounts',
    collection_code: 'gsc_accounts',
    id_field: 'account_id',
    label: 'Google Search Console accounts',
    description: 'Role access to each connected Google Search Console account in the caller workspace.',
    actions: Object.entries(actionMeta).map(([action_code, meta]) => ({
      action_code,
      label: meta.label,
      description: meta.description,
      operation_ids: buckets[action_code],
    })),
  }];
}

function assembleSettingsSource(root) {
  const importsBySpec = new Map();
  const bodies = [];
  const importRe = /^import\s+([\s\S]*?)\s+from\s+['"]([^'"]+)['"];?\r?\n?/gm;

  for (const file of ['api.ts', 'copy.ts', 'page.tsx']) {
    const raw = fs.readFileSync(path.join(root, 'ui', file), 'utf8');
    const body = raw
      .replace(importRe, (_, clause, spec) => {
        if (spec === './api' || spec === './copy') return '';
        const entry = importsBySpec.get(spec) ?? { defaultName: null, named: [] };
        const braceIdx = clause.indexOf('{');
        if (braceIdx === -1) {
          entry.defaultName ??= clause.trim();
        } else {
          const defPart = clause.slice(0, braceIdx).replace(/,\s*$/, '').trim();
          if (defPart) entry.defaultName ??= defPart;
          const endBrace = clause.lastIndexOf('}');
          const namedRaw = clause.slice(braceIdx + 1, endBrace);
          for (const item of namedRaw.split(',').map((s) => s.trim()).filter(Boolean)) {
            if (!entry.named.includes(item)) entry.named.push(item);
          }
        }
        importsBySpec.set(spec, entry);
        return '';
      })
      .replace(/^export (?!default)/gm, '')
      .trim();
    if (body) bodies.push(body);
  }

  const importLines = [];
  for (const [spec, entry] of importsBySpec) {
    const parts = [];
    if (entry.defaultName) parts.push(entry.defaultName);
    if (entry.named.length) parts.push(`{ ${entry.named.join(', ')} }`);
    importLines.push(`import ${parts.join(', ')} from '${spec}';`);
  }

  return `// Generated by scripts/generate.mjs from ui/api.ts, ui/copy.ts and ui/page.tsx.\n${importLines.join('\n')}\n\n${bodies.join('\n\n')}\n`;
}

export function generate(root = pluginRoot, { check = false } = {}) {
  const files = new Map();
  const writeJson = (file, data) => files.set(file, `${JSON.stringify(data, null, 2)}\n`);

  files.set('ui/settings.tsx', assembleSettingsSource(root));

  const operations = buildOperations();
  const mcpManifestFiles = [];
  const bindings = [];

  for (const op of operations) {
    if (!op.mcp) continue;
    const { group, name, risk } = op.mcp;
    const toolId = `plugin.${pluginId}.${name}`;
    const relPath = `tools/${group.replace(/^\/gsc\//, '')}/${name}.json`;
    const args = mcpArguments(op.input_schema, op.method);
    const toolDoc = {
      tool_id: toolId,
      name,
      short_description: op.summary,
      full_description: op.description,
      execution_target: { kind: 'interface_wrapper', interface_id: op.interface_id },
      parameter_schema_snapshot: args.parameter_schema_snapshot,
      input_mapping: args.input_mapping,
      result_schema_snapshot: op.output_schema,
      output_mapping: {},
      max_inline_chars: null,
      response_fields: null,
      permission_code_snapshot: null,
      risk_level_snapshot: risk,
      status: 'enabled',
    };
    const fullRel = `_assets/mcp/${relPath}`;
    writeJson(fullRel, toolDoc);
    mcpManifestFiles.push({ path: relPath, kind: 'tool', sha256: hash(files.get(fullRel)) });
    bindings.push({ group_path: group, tool_id: toolId, display_alias: null, visible: true, sort_order: bindings.length });
  }

  const instanceRel = `instances/${instanceId}.json`;
  const instanceFull = `_assets/mcp/${instanceRel}`;
  writeJson(instanceFull, {
    instance_id: instanceId,
    name: 'Google Search Console',
    description_short: '连接 Google 账号后分析 Search Console 站点、Sitemaps、搜索表现和收录状态。',
    status: 'enabled',
    default_entry_path: '/gsc',
    groups: [
      { path: '/gsc', display_name: 'Google Search Console', description_short: '站点、Sitemap、搜索流量分析与 URL 收录诊断。', enabled: true, sort_order: 0 },
      { path: '/gsc/accounts', display_name: 'accounts', description_short: '查看已授权的 Google Search Console 账号。', enabled: true, sort_order: 1 },
      { path: '/gsc/sites', display_name: 'sites', description_short: '列出、同步、查看与总览已验证站点。', enabled: true, sort_order: 2 },
      { path: '/gsc/sitemaps', display_name: 'sitemaps', description_short: '查看、提交 Sitemaps 及解析 Sitemap XML URL 列表。', enabled: true, sort_order: 3 },
      { path: '/gsc/analytics', display_name: 'analytics', description_short: '按关键词、页面、日期、国家与设备维度查询搜索流量表现。', enabled: true, sort_order: 4 },
      { path: '/gsc/inspection', display_name: 'inspection', description_short: '检查单页在 Google 索引中的抓取与收录状态。', enabled: true, sort_order: 5 },
    ],
    bindings,
    discovery_policy: {
      list_default_limit: 20,
      list_max_depth: 3,
      list_regex_enabled: true,
      list_regex_max_length: 256,
      list_return_fields: ['id', 'name', 'short_description'],
    },
  });
  mcpManifestFiles.unshift({ path: instanceRel, kind: 'instance', sha256: hash(files.get(instanceFull)) });

  writeJson('_assets/mcp/manifest.json', {
    schema_version: '1flowbase.mcp.bundle/v2',
    organization: 'taichuy',
    bundle_id: pluginId,
    bundle_version: version,
    locale: 'zh_Hans',
    minimum_host_version: minHost,
    exported_from_system_version: minHost,
    exported_at: '2026-11-22T00:00:00Z',
    files: mcpManifestFiles,
  });

  const permissions = [
    'service.execute',
    'plugin_data.owned.write',
    'credential.manage',
    'ephemeral.manage',
    'execution.gate',
    'network.http.request',
  ];

  const dataModels = [{
    contribution_version: '1flowbase.plugin-data-model/v1',
    storage_binding: 'main',
    owned_collections: [
      {
        collection_code: 'gsc_oauth_config',
        fields: [
          { field_code: 'config_id', field_type: 'string', nullable: false },
          { field_code: 'auth_mode', field_type: 'string', nullable: false },
          { field_code: 'client_id', field_type: 'string', nullable: false },
          { field_code: 'credential_id', field_type: 'string', nullable: false },
          { field_code: 'default_scope_mode', field_type: 'string', nullable: false },
        ],
      },
      {
        collection_code: 'gsc_accounts',
        fields: [
          { field_code: 'account_id', field_type: 'string', nullable: false },
          { field_code: 'google_sub', field_type: 'string', nullable: false },
          { field_code: 'email', field_type: 'string', nullable: false },
          { field_code: 'display_name', field_type: 'string', nullable: false },
          { field_code: 'avatar_url', field_type: 'string', nullable: true },
          { field_code: 'scope_mode', field_type: 'string', nullable: false },
          { field_code: 'granted_scopes', field_type: 'json', nullable: false },
          { field_code: 'enabled', field_type: 'boolean', nullable: false },
          { field_code: 'status', field_type: 'string', nullable: false },
          { field_code: 'last_error', field_type: 'string', nullable: true },
          { field_code: 'credential_id', field_type: 'string', nullable: false },
          { field_code: 'site_count', field_type: 'number', nullable: false },
          { field_code: 'last_synced_at', field_type: 'string', nullable: true },
        ],
      },
      {
        collection_code: 'gsc_sites',
        fields: [
          { field_code: 'site_record_id', field_type: 'string', nullable: false },
          { field_code: 'account_id', field_type: 'string', nullable: false },
          { field_code: 'site_url', field_type: 'string', nullable: false },
          { field_code: 'property_type', field_type: 'string', nullable: false },
          { field_code: 'permission_level', field_type: 'string', nullable: false },
          { field_code: 'sitemap_count', field_type: 'number', nullable: true },
          { field_code: 'last_synced_at', field_type: 'string', nullable: false },
        ],
      },
    ],
  }];

  const manifest = {
    manifest_version: 2,
    schema_version: '1flowbase.plugin.manifest/v2',
    plugin_id: pluginId,
    version,
    publisher_namespace: 'taichuy',
    vendor: 'Taichuy',
    display_name: 'Google Search Console',
    description: 'One-click Google OAuth login, Search Console sites, sitemaps, search analytics, URL inspection, and MCP tools.',
    source_kind: 'official_registry',
    trust_level: 'verified_official',
    consumption_kind: 'runtime_extension',
    execution_mode: 'process_per_call',
    slot_codes: [],
    binding_targets: ['system'],
    selection_mode: 'assignment_then_select',
    minimum_host_version: minHost,
    contract_version: '1flowbase.extension-bus/v1',
    permissions: {
      network: 'outbound_only',
      secrets: 'host_managed',
      storage: 'host_managed',
      mcp: 'none',
      subprocess: 'deny',
    },
    runtime: {
      protocol: 'stdio_json_multiplex_v1',
      entry: `bin/${pluginId}`,
      capabilities: ['runtime_host_call/v1'],
      limits: {},
    },
    managed: {
      module: {
        bus_version: 'v1',
        module_id: pluginId,
        module_version: version,
        module_kind: 'runtime',
        contributions: [
          ...operations.map((op) => ({
            contribution_id: op.contribution_id,
            contributor_module_id: pluginId,
            point_id: '1flowbase.managed-service.operation',
            contract_version: '1',
            required_permissions: permissions,
            mode: 'append',
          })),
          {
            contribution_id: `${pluginId}.data`,
            contributor_module_id: pluginId,
            point_id: '1flowbase.plugin-data.owned-collection',
            contract_version: '1flowbase.plugin-data-model/v1',
            required_permissions: ['plugin_data.owned.write'],
            mode: 'append',
          },
        ],
      },
      execution_bindings: [
        ...operations.map((op) => ({
          contribution_id: op.contribution_id,
          execution_mode: 'process_per_call',
          runtime: {
            protocol: 'stdio_json_multiplex_v1',
            entry: `bin/${pluginId}`,
            capabilities: ['runtime_host_call/v1'],
          },
          handler: op.handler,
        })),
        {
          contribution_id: `${pluginId}.data`,
          execution_mode: 'declarative_only',
          runtime: { protocol: 'stdio_json', entry: 'manifest.yaml' },
          handler: 'data',
          payload: { kind: 'data_models' },
        },
      ],
    },
    data_models: dataModels,
    managed_service: {
      scope: 'system',
      worker_lifetime: 'actor_workspace_scope',
      worker_idle_timeout_ms: 1800000,
      record_resources: buildRecordResources(operations),
      mcp_instance: {
        instance_id: instanceId,
        name: 'Google Search Console',
        description: 'Read verified sites, sitemaps, sitemap URLs, search analytics, and URL inspection from Google Search Console.',
        template: {
          path: '_assets/mcp/manifest.json',
          sha256: hash(files.get('_assets/mcp/manifest.json')),
        },
        reset_on_activation: true,
      },
      feature: {
        feature_id: `${pluginId}.management`,
        label: 'Google Search Console',
        description: 'Manage Google Search Console OAuth accounts and verified sites',
        route_id: pluginId,
        path: `/settings/${pluginId}`,
        icon: 'bar-chart',
      },
      operations: operations.map(({ operation_id, handler, mcp, record_permission, ...op }) => ({
        ...(record_permission ? { record_permission } : {}),
        ...op,
      })),
    },
    settings_pages: [{
      feature_id: `${pluginId}.management`,
      contribution_code: 'google_search_console_settings',
      source_file: 'ui/settings.tsx',
      language: 'tsx',
    }],
    keywords: ['google', 'search-console', 'seo', 'sitemaps', 'analytics', 'mcp'],
  };

  const scalar = (value) => (typeof value === 'string' && /^[A-Za-z0-9_./@-]+$/.test(value) ? value : JSON.stringify(value));
  const lines = [];
  for (const [name, value] of Object.entries(manifest)) {
    if (name === 'runtime') {
      lines.push('runtime:', ...Object.entries(value).map(([k, v]) => `  ${k}: ${scalar(v)}`));
    } else if (name === 'managed_service') {
      lines.push('managed_service:');
      for (const [k, v] of Object.entries(value)) {
        if (k === 'operations') lines.push('  operations:', ...v.map((op) => `    - ${JSON.stringify(op)}`));
        else lines.push(`  ${k}: ${scalar(v)}`);
      }
    } else {
      lines.push(`${name}: ${scalar(value)}`);
    }
  }
  files.set('manifest.yaml', `${lines.join('\n')}\n`);

  if (!check) {
    const mcpRoot = path.join(root, '_assets/mcp');
    if (fs.existsSync(mcpRoot)) fs.rmSync(mcpRoot, { recursive: true });
  }
  for (const [file, bytes] of files) {
    const target = path.join(root, file);
    if (check) {
      if (!fs.existsSync(target) || fs.readFileSync(target, 'utf8') !== bytes) {
        throw new Error(`Generated file differs: ${file}`);
      }
    } else {
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.writeFileSync(target, bytes, 'utf8');
    }
  }
  return { operations: operations.length, mcp_tools: bindings.length, files: files.size };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  console.log(JSON.stringify(generate(pluginRoot, { check: process.argv.includes('--check') })));
}

