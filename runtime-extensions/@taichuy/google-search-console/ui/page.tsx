import React, { useEffect, useMemo, useRef, useState } from 'react';
import {
  Alert, Button, Descriptions, Drawer, Dropdown, Form, Grid,
  Input, Modal, Radio, Space, Spin, Switch, Tag, Typography
} from 'antd';
import { DataTable, DataTableColumnSettings, DataTableLayout, usePersistedDataTableConfiguration } from '@1flowbase/data-table';
import { RecordPermissionEditor, getRecordCapabilities, type RecordPermissionCapabilities } from '@1flowbase/plugin-settings';
import {
  extractErrorCode, gscApi, type AccountList, type AccountSummary, type OAuthConfig,
  type ScopeMode, type SitemapEntriesResult, type SitemapInfo, type SiteSummary
} from './api';
import { copy, errorText } from './copy';

type Context = { ui: { locale?: string }; currentUser?: { isRoot?: boolean; permissions?: readonly string[] } };
type Feedback = { errorCode?: string; customError?: string; notice?: string };
type OAuthBroadcast = { type: 'gsc_oauth_result'; ok: boolean; email?: string; siteCount?: number; syncError?: string | null; errorCode?: string; ts: number };
type OAuthFormValues = { client_id: string; client_secret?: string; default_scope_mode: ScopeMode };

const OAUTH_CHANNEL = '1flowbase.gsc.oauth.v1';
const OAUTH_STORAGE_KEY = '1flowbase.gsc.oauth.event';
const POPUP_NAME = '1flowbase_gsc_oauth_popup';

function readCallbackParams(): { code?: string; state?: string; error?: string } | null {
  const params = new URLSearchParams(window.location.search);
  const code = params.get('code') ?? undefined;
  const state = params.get('state') ?? undefined;
  const error = params.get('error') ?? undefined;
  if (!code && !state && !error) return null;
  return { code, state, error };
}

function broadcastOAuth(payload: OAuthBroadcast) {
  try {
    if (typeof BroadcastChannel !== 'undefined') {
      const ch = new BroadcastChannel(OAUTH_CHANNEL);
      ch.postMessage(payload);
      ch.close();
    }
  } catch { /* ignore */ }
  try { localStorage.setItem(OAUTH_STORAGE_KEY, JSON.stringify(payload)); } catch { /* ignore */ }
}

function formatSyncTime(value: string | null): string {
  if (!value) return '-';
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return value;
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export default function GoogleSearchConsoleSettingsPage({ ctx }: { ctx: Context }) {
  const isZh = ctx.ui.locale?.toLowerCase().startsWith('zh') ?? false;
  const t = copy[isZh ? 'zh' : 'en'];
  const can = (op: string) => ctx.currentUser?.isRoot === true || ctx.currentUser?.permissions?.includes(`google-search-console.${op}`) === true;
  const canList = can('accounts.list');
  const canConfigGet = can('oauth_config.get');
  const canConfigSave = can('oauth_config.save');
  const canStartOAuth = can('oauth.start');
  const mobile = Grid.useBreakpoint().md === false;
  const touchStyle = mobile ? { minHeight: 44, minWidth: 44 } : undefined;
  const redirectUri = `${window.location.origin}/settings/google-search-console`;

  const initialCallbackRef = useRef(readCallbackParams());
  const isPopupMode = useRef(window.name === POPUP_NAME || Boolean(window.opener));
  const [callbackState, setCallbackState] = useState<'none' | 'processing' | 'done' | 'failed'>(
    initialCallbackRef.current ? 'processing' : 'none'
  );
  const [callbackMessage, setCallbackMessage] = useState('');

  const [oauthForm] = Form.useForm<OAuthFormValues>();
  const [oauthConfig, setOAuthConfig] = useState<OAuthConfig>({
    configured: false, auth_mode: 'custom_oauth_client', client_id: '',
    has_client_secret: false, default_scope_mode: 'readonly', updated_at: null
  });
  const [oauthDrawerOpen, setOAuthDrawerOpen] = useState(false);
  const [configLoaded, setConfigLoaded] = useState(false);

  const [oauthWaiting, setOAuthWaiting] = useState(false);
  const [list, setList] = useState<AccountList>({ items: [], total: 0, page: 1, page_size: 20 });
  const [capabilities, setCapabilities] = useState<Record<string, RecordPermissionCapabilities>>({});
  const [search, setSearch] = useState('');
  const [searchDraft, setSearchDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>({});
  const [deleting, setDeleting] = useState<AccountSummary | null>(null);
  const [permissionAccount, setPermissionAccount] = useState<AccountSummary | null>(null);

  const [drawerAccount, setDrawerAccount] = useState<AccountSummary | null>(null);
  const [sites, setSites] = useState<SiteSummary[]>([]);
  const [sitesLoading, setSitesLoading] = useState(false);
  const [selectedSite, setSelectedSite] = useState<SiteSummary | null>(null);
  const [sitemaps, setSitemaps] = useState<SitemapInfo[]>([]);
  const [sitemapsLoading, setSitemapsLoading] = useState(false);
  const [newSitemapUrl, setNewSitemapUrl] = useState('');
  const [entriesResult, setEntriesResult] = useState<SitemapEntriesResult | null>(null);
  const [entriesLoading, setEntriesLoading] = useState(false);
  const [followIndex, setFollowIndex] = useState(false);

  const mounted = useRef(false);
  const lastBroadcastTs = useRef(0);
  const popupTimer = useRef<number | null>(null);

  const canRecord = (account: AccountSummary, action: string) => {
    const cap = capabilities[account.account_id];
    if (cap) return cap.allowed_actions.includes(action);
    return ctx.currentUser?.isRoot === true;
  };
  const canManageRecordPerms = (account: AccountSummary) =>
    capabilities[account.account_id]?.can_manage_permissions ?? (ctx.currentUser?.isRoot === true);

  async function refreshAll(signal?: AbortSignal) {
    const tasks: Promise<unknown>[] = [];
    if (canConfigGet) {
      tasks.push(
        gscApi.getOAuthConfig(signal).then(cfg => {
          if (!mounted.current || signal?.aborted) return;
          setOAuthConfig(cfg);
          setConfigLoaded(true);
        })
      );
    }
    if (canList) {
      tasks.push(loadAccounts(1, list.page_size, search, signal));
    }
    await Promise.all(tasks);
  }

  async function loadAccounts(page = list.page, pageSize = list.page_size, term = search, signal?: AbortSignal) {
    if (!canList) return;
    const result = await gscApi.listAccounts(page, pageSize, term, signal);
    if (!mounted.current || signal?.aborted) return;
    setList(result);
    if (result.items.length > 0) {
      try {
        const capRes = await getRecordCapabilities(
          'google-search-console',
          'accounts',
          result.items.map(item => item.account_id),
          signal
        );
        if (mounted.current && !signal?.aborted) {
          setCapabilities(Object.fromEntries(capRes.items.map(item => [item.record_id, item])));
        }
      } catch { /* ignore capability probe fallback */ }
    } else {
      setCapabilities({});
    }
  }

  useEffect(() => {
    if (oauthDrawerOpen) {
      oauthForm.setFieldsValue({
        client_id: oauthConfig.client_id,
        client_secret: '',
        default_scope_mode: oauthConfig.default_scope_mode,
      });
    }
  }, [oauthDrawerOpen, oauthConfig]);

  useEffect(() => {
    mounted.current = true;
    const ctrl = new AbortController();

    async function boot() {
      const cb = initialCallbackRef.current;
      if (cb) {
        try { window.history.replaceState({}, '', window.location.pathname); } catch { /* ignore */ }
        if (cb.error || !cb.code || !cb.state) {
          const errorCode = cb.error === 'access_denied' ? 'google_oauth_cancelled' : 'google_oauth_failed';
          const payload: OAuthBroadcast = { type: 'gsc_oauth_result', ok: false, errorCode, ts: Date.now() };
          broadcastOAuth(payload);
          if (isPopupMode.current) {
            setCallbackState('failed');
            setCallbackMessage(errorText(errorCode, t));
            return;
          }
          setCallbackState('none');
          setFeedback({ errorCode });
        } else {
          try {
            const completed = await gscApi.completeOAuth({ code: cb.code, state: cb.state }, ctrl.signal);
            const payload: OAuthBroadcast = {
              type: 'gsc_oauth_result', ok: true, email: completed.account.email,
              siteCount: completed.site_count, syncError: completed.sync_error, ts: Date.now()
            };
            broadcastOAuth(payload);
            if (isPopupMode.current) {
              setCallbackState('done');
              setCallbackMessage(t.oauthCallbackDone);
              window.setTimeout(() => { try { window.close(); } catch { /* ignore */ } }, 450);
              return;
            }
            setCallbackState('none');
            setFeedback({ notice: `${t.oauthSuccess} (${completed.account.email}, ${completed.site_count})` });
          } catch (err) {
            const errorCode = extractErrorCode(err);
            broadcastOAuth({ type: 'gsc_oauth_result', ok: false, errorCode, ts: Date.now() });
            if (isPopupMode.current) {
              setCallbackState('failed');
              setCallbackMessage(errorText(errorCode, t));
              return;
            }
            setCallbackState('none');
            setFeedback({ errorCode });
          }
        }
      }

      setBusy(true);
      try { await refreshAll(ctrl.signal); }
      catch (err) {
        if (mounted.current && !ctrl.signal.aborted) setFeedback({ errorCode: extractErrorCode(err) });
      } finally {
        if (mounted.current && !ctrl.signal.aborted) setBusy(false);
      }
    }

    const handleBroadcast = (data: unknown) => {
      if (!data || typeof data !== 'object') return;
      const msg = data as OAuthBroadcast;
      if (msg.type !== 'gsc_oauth_result' || !msg.ts || msg.ts <= lastBroadcastTs.current) return;
      lastBroadcastTs.current = msg.ts;
      setOAuthWaiting(false);
      if (popupTimer.current !== null) { window.clearInterval(popupTimer.current); popupTimer.current = null; }
      if (msg.ok) {
        setFeedback({ notice: `${t.oauthSuccess} (${msg.email ?? ''}, ${msg.siteCount ?? 0})` });
        void loadAccounts(1, list.page_size, search);
      } else {
        setFeedback({ errorCode: msg.errorCode ?? 'google_oauth_failed' });
      }
    };

    const channel = typeof BroadcastChannel !== 'undefined' ? new BroadcastChannel(OAUTH_CHANNEL) : null;
    if (channel) channel.onmessage = (event) => handleBroadcast(event.data);
    const onStorage = (event: StorageEvent) => {
      if (event.key !== OAUTH_STORAGE_KEY || !event.newValue) return;
      try { handleBroadcast(JSON.parse(event.newValue)); } catch { /* ignore */ }
    };
    window.addEventListener('storage', onStorage);

    void boot();
    return () => {
      mounted.current = false;
      ctrl.abort();
      if (channel) channel.close();
      window.removeEventListener('storage', onStorage);
      if (popupTimer.current !== null) { window.clearInterval(popupTimer.current); popupTimer.current = null; }
    };
  }, []);

  async function runAction(action: () => Promise<void>, successNotice?: string) {
    setBusy(true);
    setFeedback({});
    try {
      await action();
      if (mounted.current && successNotice) setFeedback({ notice: successNotice });
    } catch (err) {
      if (mounted.current) setFeedback({ errorCode: extractErrorCode(err) });
    } finally {
      if (mounted.current) setBusy(false);
    }
  }

  async function beginGoogleLogin(reauthAccount?: AccountSummary, overrideScope?: ScopeMode) {
    if (!canStartOAuth || busy) return;
    setBusy(true);
    setFeedback({});
    try {
      const scope = overrideScope ?? (reauthAccount ? reauthAccount.scope_mode : oauthConfig.default_scope_mode);
      const started = await gscApi.startOAuth({
        redirect_uri: redirectUri,
        scope_mode: scope,
        ...(reauthAccount ? { reauth_account_id: reauthAccount.account_id } : {}),
      });
      const width = 540;
      const height = 700;
      const left = Math.max(0, Math.round(window.screenX + (window.outerWidth - width) / 2));
      const top = Math.max(0, Math.round(window.screenY + (window.outerHeight - height) / 2));
      const popup = window.open(
        started.authorization_url,
        POPUP_NAME,
        `width=${width},height=${height},left=${left},top=${top},menubar=no,toolbar=no,status=no`
      );
      if (!popup) {
        setFeedback({ notice: t.popupBlockedNotice });
        window.location.assign(started.authorization_url);
        return;
      }
      setOAuthWaiting(true);
      if (popupTimer.current !== null) window.clearInterval(popupTimer.current);
      let ticks = 0;
      popupTimer.current = window.setInterval(() => {
        ticks += 1;
        if (popup.closed || ticks > 500) {
          if (popupTimer.current !== null) { window.clearInterval(popupTimer.current); popupTimer.current = null; }
          if (mounted.current) setOAuthWaiting(false);
        }
      }, 600);
    } catch (err) {
      if (mounted.current) setFeedback({ errorCode: extractErrorCode(err) });
    } finally {
      if (mounted.current) setBusy(false);
    }
  }

  async function openSitesDrawer(account: AccountSummary) {
    setDrawerAccount(account);
    setSelectedSite(null);
    setSitemaps([]);
    setEntriesResult(null);
    setSitesLoading(true);
    try {
      const res = await gscApi.listSites(account.account_id);
      if (mounted.current) setSites(res.items);
    } catch (err) {
      if (mounted.current) setFeedback({ errorCode: extractErrorCode(err) });
    } finally {
      if (mounted.current) setSitesLoading(false);
    }
  }

  async function loadSiteSitemaps(site: SiteSummary) {
    if (!drawerAccount) return;
    setSelectedSite(site);
    setEntriesResult(null);
    setSitemapsLoading(true);
    try {
      const res = await gscApi.listSitemaps(drawerAccount.account_id, site.site_url);
      if (mounted.current) setSitemaps(res.sitemaps);
    } catch (err) {
      if (mounted.current) setFeedback({ errorCode: extractErrorCode(err) });
    } finally {
      if (mounted.current) setSitemapsLoading(false);
    }
  }

  async function previewSitemapUrls(feedpath: string) {
    if (!drawerAccount || !selectedSite) return;
    setEntriesLoading(true);
    try {
      const res = await gscApi.fetchSitemapEntries(drawerAccount.account_id, selectedSite.site_url, feedpath, followIndex);
      if (mounted.current) setEntriesResult(res);
    } catch (err) {
      if (mounted.current) setFeedback({ errorCode: extractErrorCode(err) });
    } finally {
      if (mounted.current) setEntriesLoading(false);
    }
  }

  const columns = useMemo(() => [
    {
      key: 'email', title: t.colAccount, dataIndex: 'email' as const, width: 240, sizing: 'fill' as const, mobileRole: 'title' as const,
      render: (_: unknown, row: AccountSummary) => (
        <Space orientation="vertical" size={0}>
          <Typography.Text strong>{row.display_name || row.email}</Typography.Text>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>{row.email}</Typography.Text>
        </Space>
      )
    },
    {
      key: 'scope_mode', title: t.colScope, dataIndex: 'scope_mode' as const, width: 96,
      render: (value: unknown) => <Tag>{value === 'full' ? t.scopeTagFull : t.scopeTagReadonly}</Tag>
    },
    {
      key: 'status', title: t.colStatus, dataIndex: 'status' as const, width: 112,
      render: (value: unknown) => value === 'active'
        ? <Tag color="success">{t.statusActive}</Tag>
        : value === 'reauth_required'
          ? <Tag color="warning">{t.statusReauth}</Tag>
          : <Tag color="error">{t.statusError}</Tag>
    },
    { key: 'site_count', title: t.colSites, dataIndex: 'site_count' as const, width: 84 },
    {
      key: 'enabled', title: t.colEnabled, dataIndex: 'enabled' as const, width: 84,
      render: (value: unknown, row: AccountSummary) => (
        <Switch
          checked={Boolean(value)}
          disabled={busy || !can('accounts.update') || !canRecord(row, 'manage')}
          onChange={(checked) => {
            void runAction(async () => {
              await gscApi.updateAccount(row.account_id, checked);
              await loadAccounts();
            }, t.updatedOk);
          }}
        />
      )
    },
    {
      key: 'last_synced_at', title: t.colSynced, dataIndex: 'last_synced_at' as const, width: 150,
      render: (value: unknown) => formatSyncTime(typeof value === 'string' ? value : null)
    },
    {
      key: 'actions', title: t.colActions, width: 190, mobileRole: 'actions' as const,
      render: (_: unknown, row: AccountSummary) => {
        const targetScope: ScopeMode = row.scope_mode === 'full' ? 'readonly' : 'full';
        const moreItems = [
          ...(can('accounts.test') && canRecord(row, 'manage') ? [{
            key: 'test', label: t.testConn, disabled: busy || !row.enabled,
            onClick: () => void runAction(async () => { await gscApi.testAccount(row.account_id); await loadAccounts(); }, t.testedOk)
          }] : []),
          ...(canStartOAuth && canRecord(row, 'manage') ? [{
            key: 'reauth', label: t.reauth, disabled: busy,
            onClick: () => void beginGoogleLogin(row)
          }, {
            key: 'switch_scope',
            label: targetScope === 'full' ? t.upgradeToFull : t.downgradeToReadonly,
            disabled: busy,
            onClick: () => void beginGoogleLogin(row, targetScope)
          }] : []),
          ...(canManageRecordPerms(row) ? [{
            key: 'perms', label: t.permissions, disabled: busy,
            onClick: () => setPermissionAccount(row)
          }] : []),
          ...(can('accounts.delete') && canRecord(row, 'delete') ? [{
            key: 'delete', label: t.remove, danger: true, disabled: busy,
            onClick: () => setDeleting(row)
          }] : []),
        ];
        return (
          <Space size={4}>
            <Button type="link" size="small" style={touchStyle} disabled={busy || !can('sites.list') || !canRecord(row, 'view')}
              onClick={() => void openSitesDrawer(row)}>{t.viewSites}</Button>
            {can('sites.sync') && canRecord(row, 'manage') && (
              <Button type="link" size="small" style={touchStyle} disabled={busy || !row.enabled}
                onClick={() => void runAction(async () => { await gscApi.syncSites(row.account_id); await loadAccounts(); }, t.syncedOk)}>{t.syncSites}</Button>
            )}
            {moreItems.length > 0 && (
              <Dropdown trigger={['click']} menu={{ items: moreItems }}>
                <Button type="text" size="small" style={touchStyle} disabled={busy}>{t.more}</Button>
              </Dropdown>
            )}
          </Space>
        );
      }
    }
  ], [t, busy, capabilities, list.page, list.items.length, oauthConfig.default_scope_mode]);

  const configuration = usePersistedDataTableConfiguration({ columns, storageKey: '1flowbase.gsc.accounts.columns.v1' });

  if (callbackState === 'processing' || callbackState === 'done' || callbackState === 'failed') {
    return (
      <section style={{ maxWidth: 480, margin: '48px auto', padding: 24 }}>
        <div style={{ background: '#ffffff', border: '1px solid #e8edea', borderRadius: 12, padding: 24 }}>
          <Space orientation="vertical" size={16} style={{ width: '100%' }}>
            <Typography.Title level={4} style={{ margin: 0 }}>{t.title}</Typography.Title>
            {callbackState === 'processing' && <Space><Spin /><Typography.Text>{t.oauthCallbackProcessing}</Typography.Text></Space>}
            {callbackState === 'done' && <Alert type="success" showIcon title={callbackMessage} />}
            {callbackState === 'failed' && <Alert type="error" showIcon title={callbackMessage} />}
            {callbackState !== 'processing' && (
              <Button type="primary" onClick={() => { try { window.close(); } catch { /* ignore */ } }}>{t.closeWindow}</Button>
            )}
          </Space>
        </div>
      </section>
    );
  }

  return (
    <section data-testid="gsc-settings" style={{ width: '100%', minWidth: 0, padding: mobile ? 12 : 16, boxSizing: 'border-box' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: 12, flexWrap: 'wrap', padding: '4px 0 12px' }}>
        <div style={{ minWidth: 0 }}>
          <Space size={8} align="center" wrap>
            <Typography.Title level={4} style={{ margin: 0, fontSize: 20, lineHeight: '28px', fontWeight: 600 }}>
              {t.title}
            </Typography.Title>
            {configLoaded && (
              <Tag color={oauthConfig.configured ? 'success' : 'default'}>
                {oauthConfig.configured ? t.configuredTag : t.unconfiguredTag}
              </Tag>
            )}
          </Space>
          <Typography.Text type="secondary" style={{ display: 'block', marginTop: 4 }}>
            {t.subtitlePrefix}
            <Typography.Link href="/settings/mcp-management">{t.mcpLinkLabel}</Typography.Link>
            {t.subtitleSuffix}
          </Typography.Text>
        </div>
      </div>

      {(oauthWaiting || feedback.errorCode || feedback.customError || feedback.notice) && (
        <div aria-live="polite" style={{ marginBottom: 12 }}>
          <Space orientation="vertical" size={8} style={{ width: '100%' }}>
            {oauthWaiting && <Alert type="info" showIcon title={t.signingIn} />}
            {(feedback.errorCode || feedback.customError) && (
              <Alert type="error" showIcon closable onClose={() => setFeedback({})}
                title={feedback.customError ?? errorText(feedback.errorCode!, t)} />
            )}
            {feedback.notice && (
              <Alert type="success" showIcon closable onClose={() => setFeedback({})} title={feedback.notice} />
            )}
          </Space>
        </div>
      )}

      {!canList ? (
        <Alert type="warning" showIcon title={t.noList} />
      ) : (
        <DataTableLayout>
          <DataTable
            columns={columns}
            configuration={configuration}
            rowKey="account_id"
            dataSource={list.items}
            loading={busy}
            emptyText={t.emptyAccounts}
            page={list.page}
            pageSize={list.page_size}
            total={list.total}
            onPageChange={(page: number) => {
              if (!busy) void runAction(() => loadAccounts(page, list.page_size, search));
            }}
            toolbar={
              <Space wrap style={{ width: '100%', justifyContent: 'space-between' }}>
                <Space wrap>
                  {!oauthConfig.configured && canConfigGet ? (
                    <Button
                      type="primary"
                      disabled={busy}
                      style={touchStyle}
                      onClick={() => setOAuthDrawerOpen(true)}
                    >
                      {t.configureOAuth}
                    </Button>
                  ) : (
                    <>
                      {canStartOAuth && (
                        <Button
                          type="primary"
                          disabled={busy}
                          loading={oauthWaiting}
                          style={touchStyle}
                          onClick={() => void beginGoogleLogin()}
                        >
                          {t.signInGoogle}
                        </Button>
                      )}
                      {canConfigGet && (
                        <Button disabled={busy} style={touchStyle} onClick={() => setOAuthDrawerOpen(true)}>
                          {t.oauthSettings}
                        </Button>
                      )}
                    </>
                  )}
                  <Button
                    disabled={busy}
                    style={touchStyle}
                    onClick={() => void runAction(() => refreshAll())}
                  >
                    {t.refresh}
                  </Button>
                </Space>
                <Space wrap>
                  <Input.Search
                    aria-label={t.search}
                    placeholder={t.search}
                    value={searchDraft}
                    disabled={busy}
                    allowClear
                    style={{ width: mobile ? '100%' : 220 }}
                    onChange={(e) => setSearchDraft(e.target.value)}
                    onSearch={(value) => {
                      const term = value.trim();
                      setSearch(term);
                      void runAction(() => loadAccounts(1, list.page_size, term));
                    }}
                  />
                  <DataTableColumnSettings
                    columns={columns}
                    configuration={configuration}
                    ariaLabel={t.columns}
                    placeholder={t.columns}
                    resetLabel={t.resetColumns}
                  />
                </Space>
              </Space>
            }
          />
        </DataTableLayout>
      )}

      <Drawer
        open={oauthDrawerOpen}
        title={t.oauthDrawerTitle}
        size={mobile ? '100%' : 480}
        getContainer={false}
        rootStyle={{ position: 'fixed' }}
        maskClosable={!busy}
        closable={!busy}
        onClose={() => { if (!busy) setOAuthDrawerOpen(false); }}
      >
        <Space orientation="vertical" size={16} style={{ width: '100%' }}>
          <Alert
            type="info"
            showIcon
            title={
              <Space orientation="vertical" size={4} style={{ width: '100%', fontSize: 13 }}>
                <div>
                  {t.setupStep1Prefix}
                  <Typography.Link
                    href="https://console.cloud.google.com/apis/library/searchconsole.googleapis.com"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    {t.setupStep1Link}
                  </Typography.Link>
                </div>
                <div>
                  {t.setupStep2Prefix}
                  <Typography.Link
                    href="https://console.cloud.google.com/apis/credentials/consent"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    {t.setupStep2Link}
                  </Typography.Link>
                  {t.setupStep2Suffix}
                </div>
                <div>
                  {t.setupStep3Prefix}
                  <Typography.Link
                    href="https://console.cloud.google.com/apis/credentials"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    {t.setupStep3Link}
                  </Typography.Link>
                  {t.setupStep3Suffix}
                </div>
              </Space>
            }
          />
          <div>
            <div style={{ marginBottom: 6, fontSize: 14, fontWeight: 500, color: '#16211d' }}>
              {t.redirectUriLabel}
            </div>
            <div style={{ background: '#f8fcf9', border: '1px solid #e8edea', borderRadius: 8, padding: '8px 12px' }}>
              <Typography.Paragraph
                copyable={{ text: redirectUri }}
                style={{
                  margin: 0,
                  fontFamily: 'SFMono-Regular, Menlo, Monaco, Consolas, monospace',
                  fontSize: 12,
                  wordBreak: 'break-all',
                }}
              >
                {redirectUri}
              </Typography.Paragraph>
            </div>
            <Typography.Text type="secondary" style={{ display: 'block', marginTop: 6, fontSize: 12 }}>
              {t.redirectUriHint}
            </Typography.Text>
          </div>

          <Form
            form={oauthForm}
            layout="vertical"
            initialValues={{ default_scope_mode: 'readonly' }}
            onFinish={(values) => {
              if (!canConfigSave) return;
              void runAction(async () => {
                const saved = await gscApi.saveOAuthConfig({
                  client_id: values.client_id.trim(),
                  ...(values.client_secret?.trim() ? { client_secret: values.client_secret.trim() } : {}),
                  default_scope_mode: values.default_scope_mode
                });
                setOAuthConfig(saved);
                setConfigLoaded(true);
                setOAuthDrawerOpen(false);
              }, t.oauthSaved);
            }}
          >
            <Form.Item name="client_id" label={t.clientId} rules={[{ required: true, whitespace: true, message: t.required }]}>
              <Input autoComplete="off" placeholder="1234567890-xxxx.apps.googleusercontent.com" disabled={!canConfigSave || busy} />
            </Form.Item>
            <Form.Item
              name="client_secret"
              label={t.clientSecret}
              extra={oauthConfig.has_client_secret ? t.clientSecretKeep : t.clientSecretReq}
              rules={oauthConfig.has_client_secret ? [] : [{ required: true, whitespace: true, message: t.clientSecretReq }]}
            >
              <Input.Password autoComplete="new-password" disabled={!canConfigSave || busy} />
            </Form.Item>
            <Form.Item name="default_scope_mode" label={t.defaultScope}>
              <Radio.Group disabled={!canConfigSave || busy}>
                <Radio value="readonly">{t.scopeReadonly}</Radio>
                <Radio value="full">{t.scopeFull}</Radio>
              </Radio.Group>
            </Form.Item>
            <Space>
              {canConfigSave && (
                <Button type="primary" htmlType="submit" loading={busy} style={touchStyle}>
                  {t.saveOAuth}
                </Button>
              )}
              <Button disabled={busy} style={touchStyle} onClick={() => setOAuthDrawerOpen(false)}>
                {t.cancel}
              </Button>
            </Space>
          </Form>
        </Space>
      </Drawer>

      {permissionAccount && (
        <RecordPermissionEditor
          pluginCode="google-search-console"
          resourceCode="accounts"
          recordId={permissionAccount.account_id}
          open
          locale={ctx.ui.locale}
          onCancel={() => setPermissionAccount(null)}
          onSaved={() => { setPermissionAccount(null); void loadAccounts(); }}
        />
      )}

      <Modal
        open={Boolean(deleting)}
        title={t.deleteTitle}
        okText={t.remove}
        cancelText={t.cancel}
        okButtonProps={{ danger: true, disabled: busy }}
        confirmLoading={busy}
        onCancel={() => { if (!busy) setDeleting(null); }}
        onOk={() => {
          if (!deleting) return;
          void runAction(async () => {
            await gscApi.deleteAccount(deleting.account_id);
            setDeleting(null);
            await loadAccounts(1, list.page_size, search);
          }, t.deletedOk);
        }}
      >
        <Typography.Paragraph strong>{deleting?.email}</Typography.Paragraph>
        <Typography.Paragraph>{t.deleteHelp}</Typography.Paragraph>
      </Modal>

      <Drawer
        open={Boolean(drawerAccount)}
        title={`${t.drawerTitle}${drawerAccount ? ` — ${drawerAccount.email}` : ''}`}
        size={mobile ? '100%' : 680}
        getContainer={false}
        rootStyle={{ position: 'fixed' }}
        onClose={() => setDrawerAccount(null)}
      >
        {drawerAccount && (
          <Space orientation="vertical" size={16} style={{ width: '100%' }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
              <Typography.Text strong>{t.sitesTitle}</Typography.Text>
              {can('sites.sync') && canRecord(drawerAccount, 'manage') && (
                <Button
                  type="primary"
                  size="small"
                  loading={sitesLoading}
                  disabled={!drawerAccount.enabled}
                  onClick={() => {
                    setSitesLoading(true);
                    void gscApi.syncSites(drawerAccount.account_id)
                      .then((res) => { if (mounted.current) { setSites(res.items); void loadAccounts(); } })
                      .catch((err) => { if (mounted.current) setFeedback({ errorCode: extractErrorCode(err) }); })
                      .finally(() => { if (mounted.current) setSitesLoading(false); });
                  }}
                >
                  {t.syncSites}
                </Button>
              )}
            </div>

            {sitesLoading ? <Spin /> : sites.length === 0 ? (
              <Alert type="info" showIcon title={t.sitesEmpty} />
            ) : (
              <div style={{ border: '1px solid #e8edea', borderRadius: 8, overflow: 'hidden', background: '#ffffff' }}>
                {sites.map((site, idx) => {
                  const selected = selectedSite?.site_url === site.site_url;
                  return (
                    <div
                      key={site.site_record_id}
                      style={{
                        display: 'flex',
                        justifyContent: 'space-between',
                        alignItems: 'center',
                        gap: 12,
                        flexWrap: 'wrap',
                        padding: '10px 12px',
                        background: selected ? '#e6f7f2' : '#ffffff',
                        borderBottom: idx < sites.length - 1 ? '1px solid #e8edea' : undefined,
                      }}
                    >
                      <Space size={8} wrap>
                        <Typography.Text strong style={{ overflowWrap: 'anywhere' }}>{site.site_url}</Typography.Text>
                        <Tag>{site.property_type}</Tag>
                        <Tag>{site.permission_level}</Tag>
                      </Space>
                      <Button size="small" type={selected ? 'primary' : 'link'} onClick={() => void loadSiteSitemaps(site)}>
                        {t.sitemapsTitle}
                      </Button>
                    </div>
                  );
                })}
              </div>
            )}

            {selectedSite && (
              <div style={{ border: '1px solid #e8edea', borderRadius: 8, overflow: 'hidden', background: '#ffffff' }}>
                <div style={{ padding: '10px 12px', background: '#f8fcf9', borderBottom: '1px solid #e8edea', fontWeight: 600 }}>
                  {t.sitemapsTitle}: {selectedSite.site_url}
                </div>
                <div style={{ padding: 12 }}>
                  <Space orientation="vertical" size={12} style={{ width: '100%' }}>
                    {drawerAccount.scope_mode === 'full' && can('sitemaps.submit') && canRecord(drawerAccount, 'manage') && (
                      <Space.Compact style={{ width: '100%' }}>
                        <Input
                          placeholder={t.submitSitemapPlaceholder}
                          value={newSitemapUrl}
                          onChange={(e) => setNewSitemapUrl(e.target.value)}
                        />
                        <Button
                          type="primary"
                          disabled={!newSitemapUrl.trim() || sitemapsLoading}
                          onClick={() => {
                            setSitemapsLoading(true);
                            void gscApi.submitSitemap(drawerAccount.account_id, selectedSite.site_url, newSitemapUrl.trim())
                              .then(() => { setNewSitemapUrl(''); setFeedback({ notice: t.sitemapSubmitted }); return loadSiteSitemaps(selectedSite); })
                              .catch((err) => { if (mounted.current) setFeedback({ errorCode: extractErrorCode(err) }); })
                              .finally(() => { if (mounted.current) setSitemapsLoading(false); });
                          }}
                        >
                          {t.submitSitemapBtn}
                        </Button>
                      </Space.Compact>
                    )}

                    {sitemapsLoading ? <Spin /> : sitemaps.length === 0 ? (
                      <Typography.Text type="secondary">{t.sitemapsEmpty}</Typography.Text>
                    ) : (
                      <Space orientation="vertical" size={10} style={{ width: '100%' }}>
                        <Space>
                          <Switch size="small" checked={followIndex} onChange={setFollowIndex} />
                          <Typography.Text type="secondary">{t.followIndexLabel}</Typography.Text>
                        </Space>
                        <div style={{ border: '1px solid #e8edea', borderRadius: 8, overflow: 'hidden' }}>
                          {sitemaps.map((sm, idx) => (
                            <div
                              key={sm.path ?? String(idx)}
                              style={{
                                padding: 12,
                                borderBottom: idx < sitemaps.length - 1 ? '1px solid #e8edea' : undefined,
                              }}
                            >
                              <Space orientation="vertical" size={6} style={{ width: '100%' }}>
                                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                                  <Typography.Text code style={{ overflowWrap: 'anywhere' }}>{sm.path}</Typography.Text>
                                  {sm.path && (
                                    <Button size="small" loading={entriesLoading} onClick={() => void previewSitemapUrls(sm.path!)}>
                                      {t.previewUrlsBtn}
                                    </Button>
                                  )}
                                </div>
                                <Descriptions size="small" column={2} items={[
                                  { key: 'sub', label: 'lastSubmitted', children: sm.lastSubmitted ?? '-' },
                                  { key: 'dl', label: 'lastDownloaded', children: sm.lastDownloaded ?? '-' },
                                  { key: 'warn', label: 'warnings', children: String(sm.warnings ?? 0) },
                                  { key: 'err', label: 'errors', children: String(sm.errors ?? 0) },
                                ]} />
                              </Space>
                            </div>
                          ))}
                        </div>
                      </Space>
                    )}

                    {entriesLoading && <Spin />}
                    {entriesResult && (
                      <div style={{ border: '1px solid #e8edea', borderRadius: 8, overflow: 'hidden' }}>
                        <div style={{ padding: '8px 12px', background: '#f8fcf9', borderBottom: '1px solid #e8edea', fontWeight: 500, fontSize: 13 }}>
                          {t.previewUrlsTitle} ({entriesResult.returned_count}/{entriesResult.total_found})
                        </div>
                        <div style={{ maxHeight: 260, overflow: 'auto', padding: '4px 12px' }}>
                          {entriesResult.entries.map((entry, idx) => (
                            <div
                              key={`${entry.loc}-${idx}`}
                              style={{
                                paddingBlock: 6,
                                borderBottom: idx < entriesResult.entries.length - 1 ? '1px solid #e8edea' : undefined,
                              }}
                            >
                              <Typography.Text code style={{ fontSize: 12, overflowWrap: 'anywhere' }}>{entry.loc}</Typography.Text>
                              {entry.lastmod && (
                                <Typography.Text type="secondary" style={{ marginLeft: 8, fontSize: 12 }}>
                                  {entry.lastmod}
                                </Typography.Text>
                              )}
                            </div>
                          ))}
                        </div>
                      </div>
                    )}
                  </Space>
                </div>
              </div>
            )}
          </Space>
        )}
      </Drawer>
    </section>
  );
}

