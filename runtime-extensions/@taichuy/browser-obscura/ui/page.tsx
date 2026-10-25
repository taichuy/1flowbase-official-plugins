import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Alert, Button, Descriptions, Divider, Drawer, Empty, Form, Grid, Input, InputNumber, List, Modal, Popconfirm, Space, Switch, Tag, Typography } from 'antd';
import { DataTable, DataTableColumnSettings, DataTableLayout, usePersistedDataTableConfiguration } from '@1flowbase/data-table';
import { request } from '@1flowbase/plugin-settings';

type BrowserSummary = { browser_id: string; name: string; status: 'running'; created_at_unix_ms: number; last_activity_at_unix_ms: number; tab_count: number; idle_timeout_ms: number };
type TabSummary = { browser_id: string; tab_id: string; title: string; url: string; status: 'ready' | 'loading' | 'failed'; snapshot_id: string | null };
type Failure = { success: false; error: { code: string; message?: string } };
type Result<T = object> = ({ success: true } & T) | Failure;
type BrowserList = { items: BrowserSummary[]; page: number; page_size: number; total: number };
type Detail = { browser: BrowserSummary; tabs: TabSummary[] };
type Screenshot = { data: string; mime_type: 'image/png'; metadata: { browser_id: string; tab_id: string; url: string; title: string; width: number; height: number } };
type Column = { key: string; title: string; width: number; dataIndex?: keyof BrowserSummary; sizing?: 'fill'; mobileRole?: 'title' | 'actions'; render?: (value: unknown, record: BrowserSummary) => React.ReactNode };
const root = '/api/console/managed-services/browser-obscura';
const copy = {
  en: {
    title: 'Browsers', intro: 'Manage browser instances created by you and your AI assistants. Closing this page keeps them running until their idle timeout.',
    create: 'Create browser', name: 'Name', idle: 'Idle timeout (milliseconds)', idleHelp: 'Leave empty to use the service default.', cancel: 'Cancel', done: 'Done', refresh: 'Refresh', search: 'Search browsers', empty: 'No browsers found',
    status: 'Status', running: 'Running', created_at_unix_ms: 'Created', last_activity_at_unix_ms: 'Last activity', tab_count: 'Tabs', actions: 'Actions', details: 'View details', closeBrowser: 'Close browser', closeBrowserConfirm: 'Close this browser and all its tabs?',
    columns: 'Columns', resetColumns: 'Reset columns', auto: 'Auto refresh', mcp: 'MCP settings', mcpHelp: 'Copy the browser_obscura connection configuration from MCP settings to connect your AI client.',
    detail: 'Browser details', tabs: 'Tabs', noTabs: 'No tabs. Create one to start browsing.', newTab: 'Create tab', initialUrl: 'Initial URL (optional)', url: 'URL', openTab: 'Select tab', closeTab: 'Close tab', closeTabConfirm: 'Close this tab?', untitled: 'Untitled tab',
    ready: 'Ready', loading: 'Loading', failed: 'Failed', navigate: 'Navigate', screenshot: 'Take screenshot', screenshotAlt: 'Browser tab screenshot', preview: 'Screenshot preview', selected: 'Selected tab', browser_id: 'Browser ID',
    requestFailed: 'The operation could not be completed. Refresh and try again.', missingBrowser: 'This browser is no longer available. Refresh the browser list.', missingTab: 'This tab is no longer available. Refresh the browser details.', invalidUrl: 'Enter a valid URL supported by the browser.', timeout: 'The browser did not respond in time. Refresh to check its current state.', unavailable: 'The browser service is unavailable. Try again later.', forbidden: 'You do not have permission to perform this operation.', capacity: 'The browser service has reached its capacity. Close an unused browser and try again.',
    created: 'Browser created', closed: 'Browser closed', refreshed: 'Refreshed', tabCreated: 'Tab created', tabClosed: 'Tab closed', navigated: 'Navigation completed', captured: 'Screenshot captured', required: 'This field is required',
  },
  zh: {
    title: '浏览器', intro: '管理你和 AI 助手创建的浏览器实例。离开页面后，实例会继续运行，直到空闲超时。',
    create: '创建浏览器', name: '名称', idle: '空闲超时（毫秒）', idleHelp: '留空使用服务默认值。', cancel: '取消', done: '完成', refresh: '刷新', search: '搜索浏览器', empty: '未找到浏览器',
    status: '状态', running: '运行中', created_at_unix_ms: '创建时间', last_activity_at_unix_ms: '最近活动', tab_count: '标签页数', actions: '操作', details: '查看详情', closeBrowser: '关闭浏览器', closeBrowserConfirm: '关闭此浏览器及其全部标签页？',
    columns: '显示列', resetColumns: '重置显示列', auto: '自动刷新', mcp: 'MCP 设置', mcpHelp: '在 MCP 设置中复制 browser_obscura 的连接配置，接入你的 AI 客户端。',
    detail: '浏览器详情', tabs: '标签页', noTabs: '暂无标签页，创建后即可浏览。', newTab: '创建标签页', initialUrl: '初始网址（可选）', url: '网址', openTab: '选择标签页', closeTab: '关闭标签页', closeTabConfirm: '关闭此标签页？', untitled: '未命名标签页',
    ready: '就绪', loading: '加载中', failed: '失败', navigate: '前往', screenshot: '截取屏幕', screenshotAlt: '浏览器标签页截图', preview: '截图预览', selected: '已选标签页', browser_id: '浏览器 ID',
    requestFailed: '操作未完成，请刷新后重试。', missingBrowser: '此浏览器已不可用，请刷新浏览器列表。', missingTab: '此标签页已不可用，请刷新浏览器详情。', invalidUrl: '请输入浏览器支持的有效网址。', timeout: '浏览器响应超时，请刷新查看当前状态。', unavailable: '浏览器服务暂不可用，请稍后重试。', forbidden: '你没有执行此操作的权限。', capacity: '浏览器服务已达容量上限，请关闭闲置浏览器后重试。',
    created: '浏览器已创建', closed: '浏览器已关闭', refreshed: '已刷新', tabCreated: '标签页已创建', tabClosed: '标签页已关闭', navigated: '已完成导航', captured: '已截取屏幕', required: '请填写此项',
  },
};
type Copy = typeof copy.en;
function errorText(code: string, t: Copy) {
  // Only stable business codes select product copy; host exceptions and error messages are never rendered.
  const messages: Record<string, string> = {
    browser_not_found: t.missingBrowser, browser_closed: t.missingBrowser,
    tab_not_found: t.missingTab, tab_closed: t.missingTab,
    invalid_url: t.invalidUrl, unsupported_url_scheme: t.invalidUrl, invalid_input: t.requestFailed,
    navigation_timeout: t.timeout, deadline_exceeded: t.timeout, wait_timeout: t.timeout,
    navigation_failed: t.requestFailed, page_failed: t.requestFailed, page_unavailable: t.requestFailed,
    snapshot_failed: t.requestFailed, screenshot_failed: t.requestFailed,
    browser_service_unavailable: t.unavailable, engine_unavailable: t.unavailable,
    forbidden: t.forbidden, permission_denied: t.forbidden, scope_mismatch: t.forbidden,
    browser_capacity_exceeded: t.capacity,
  };
  return messages[code] ?? t.requestFailed;
}
function call<T>(path: string, method: 'GET' | 'POST' | 'DELETE', signal: AbortSignal, body?: unknown) {
  return request<Result<T>>({ path: `${root}${path}`, method, body, signal });
}
const browserPath = (browser_id: string) => `/browsers/${encodeURIComponent(browser_id)}`;
const tabPath = (browser_id: string, tab_id: string) => `${browserPath(browser_id)}/tabs/${encodeURIComponent(tab_id)}`;

export default function BrowserObscuraSettingsPage({ ctx }: { ctx: { ui: { locale?: string } } }) {
  const locale = ctx.ui.locale?.toLowerCase().startsWith('zh') ? 'zh' : 'en';
  const t = copy[locale];
  const mobile = Grid.useBreakpoint().md === false;
  const [createForm] = Form.useForm();
  const [tabForm] = Form.useForm();
  const [navigateForm] = Form.useForm();
  const [list, setList] = useState<BrowserList>({ items: [], page: 1, page_size: 20, total: 0 });
  const [search, setSearch] = useState('');
  const [searchDraft, setSearchDraft] = useState('');
  const [createOpen, setCreateOpen] = useState(false);
  const [browser_id, setBrowserId] = useState<string>();
  const [detail, setDetail] = useState<Detail>();
  const [tab_id, setTabId] = useState<string>();
  const [screenshot, setScreenshot] = useState<Screenshot>();
  const [busy, setBusy] = useState(false);
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [error, setError] = useState<string>();
  const [notice, setNotice] = useState<keyof Copy>();
  const mounted = useRef(false);
  const controller = useRef<AbortController | null>(null);
  const actionOrigin = useRef<HTMLElement | null>(null);
  const previousMobile = useRef(mobile);

  async function run(action: (signal: AbortSignal) => Promise<Result>, success?: keyof Copy, quiet = false) {
    if (controller.current) return;
    const active = new AbortController();
    controller.current = active;
    setBusy(true);
    if (!quiet) { setError(undefined); setNotice(undefined); }
    try {
      const result = await action(active.signal);
      if (!mounted.current || active.signal.aborted) return;
      if (!result.success) { setError(result.error.code); setNotice(undefined); }
      else if (success) { setNotice(success); setError(undefined); }
    } catch {
      if (mounted.current && !active.signal.aborted) { setError('request_failed'); setNotice(undefined); }
    } finally {
      if (controller.current === active) {
        controller.current = null;
        if (mounted.current) setBusy(false);
      }
    }
  }
  async function loadList(signal: AbortSignal, page = list.page, term = search, append = false) {
    const query = new URLSearchParams({ page: String(page), page_size: String(list.page_size) });
    if (term) query.set('search', term);
    const result = await call<BrowserList>(`/browsers?${query}`, 'GET', signal);
    if (result.success && mounted.current && !signal.aborted) {
      setList(current => ({ ...result, items: append ? [...current.items, ...result.items] : result.items }));
    }
    return result;
  }
  async function loadDetail(signal: AbortSignal, id: string) {
    const result = await call<Detail>(browserPath(id), 'GET', signal);
    if (result.success && mounted.current && !signal.aborted) {
      setDetail(result);
      setTabId(current => result.tabs.some(tab => tab.tab_id === current) ? current : result.tabs[0]?.tab_id);
    }
    return result;
  }
  async function refresh(signal: AbortSignal) {
    const result = await loadList(signal, mobile ? 1 : list.page);
    if (!result.success || !browser_id) return result;
    return loadDetail(signal, browser_id);
  }
  useEffect(() => {
    mounted.current = true;
    void run(signal => loadList(signal, 1));
    return () => { mounted.current = false; controller.current?.abort(); controller.current = null; };
  }, []);
  useEffect(() => {
    if (!autoRefresh || createOpen) return;
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible' && !controller.current) void run(refresh, undefined, true);
    }, 15000);
    return () => clearInterval(timer);
  }, [autoRefresh, createOpen, browser_id, list.page, list.page_size, search, mobile]);
  useEffect(() => {
    if (previousMobile.current === mobile || busy) return;
    previousMobile.current = mobile;
    if (list.page > 1) void run(signal => loadList(signal, 1));
  }, [mobile, busy]);
  const selectedTab = detail?.tabs.find(tab => tab.tab_id === tab_id);
  useEffect(() => { setScreenshot(undefined); }, [tab_id]);
  useEffect(() => { navigateForm.setFieldsValue({ url: selectedTab?.url ?? '' }); }, [selectedTab?.url, tab_id]);
  function closeDetail() {
    if (controller.current) return;
    setBrowserId(undefined); setDetail(undefined); setTabId(undefined); setScreenshot(undefined); setError(undefined); setNotice(undefined);
    tabForm.resetFields();
  }
  async function updateTabs(signal: AbortSignal, action: () => Promise<Result>, successBrowserId: string) {
    const result = await action();
    if (!result.success) return result;
    if (mounted.current && !signal.aborted) setScreenshot(undefined);
    const updated = await loadDetail(signal, successBrowserId);
    if (!updated.success) return updated;
    return loadList(signal, mobile ? 1 : list.page);
  }
  const formatDate = (value: unknown) => new Date(Number(value)).toLocaleString(locale === 'zh' ? 'zh-CN' : 'en-US');
  const touchStyle = mobile ? { minHeight: 44 } : undefined;
  const columns = useMemo<Column[]>(() => [
    { key: 'name', title: t.name, dataIndex: 'name', width: 200, sizing: 'fill', mobileRole: 'title' },
    { key: 'status', title: t.status, dataIndex: 'status', width: 110, render: () => <Tag color="success">{t.running}</Tag> },
    { key: 'tab_count', title: t.tab_count, dataIndex: 'tab_count', width: 100 },
    { key: 'created_at_unix_ms', title: t.created_at_unix_ms, dataIndex: 'created_at_unix_ms', width: 200, render: formatDate },
    { key: 'last_activity_at_unix_ms', title: t.last_activity_at_unix_ms, dataIndex: 'last_activity_at_unix_ms', width: 200, render: formatDate },
    { key: 'actions', title: t.actions, width: 240, mobileRole: 'actions', render: (_, browser) => <Space wrap data-testid={`browser-actions-${browser.browser_id}`}>
      <Button disabled={busy} style={touchStyle} onClick={event => {
        actionOrigin.current = event.currentTarget;
        setBrowserId(browser.browser_id); setDetail(undefined); setTabId(undefined); setScreenshot(undefined);
        void run(signal => loadDetail(signal, browser.browser_id));
      }}>{t.details}</Button>
      <Popconfirm title={t.closeBrowserConfirm} okText={t.closeBrowser} cancelText={t.cancel} disabled={busy} onConfirm={() => run(async signal => {
        const result = await call<{ closed: true }>(browserPath(browser.browser_id), 'DELETE', signal);
        if (!result.success) return result;
        return loadList(signal, mobile ? 1 : (list.items.length === 1 && list.page > 1 ? list.page - 1 : list.page));
      }, 'closed')}><Button danger disabled={busy} style={touchStyle}>{t.closeBrowser}</Button></Popconfirm>
    </Space> },
  ], [t, locale, busy, mobile, list.page, list.items.length, search]);
  const configuration = usePersistedDataTableConfiguration({ columns, storageKey: '1flowbase.browser-obscura.browsers.columns.v1' });
  const feedback = <div aria-live="polite" style={{ marginBlock: 12 }}>
    {error && <Alert type="error" showIcon message={errorText(error, t)} />}
    {notice && <Alert type="success" showIcon message={t[notice]} />}
  </div>;
  return <section data-testid="browser-obscura-settings" style={{ width: '100%', minWidth: 0, padding: 16, boxSizing: 'border-box' }}>
    <Typography.Title level={3}>{t.title}</Typography.Title>
    <Typography.Paragraph type="secondary">{t.intro}</Typography.Paragraph>
    <Typography.Paragraph><Typography.Link href="/settings/mcp-management">{t.mcp}</Typography.Link> — {t.mcpHelp}</Typography.Paragraph>
    <DataTableLayout>
      <DataTable columns={columns} configuration={configuration} rowKey="browser_id" dataSource={list.items} loading={busy} emptyText={t.empty}
        page={list.page} pageSize={list.page_size} total={list.total} onPageChange={(page: number) => { if (!busy) void run(signal => loadList(signal, page)); }}
        mobileList={mobile ? { resetKey: `browser-obscura-${search}`, hasMore: list.page * list.page_size < list.total, loading: busy, failed: Boolean(error), onLoadMore: () => run(signal => loadList(signal, list.page + 1, search, true)) } : undefined}
        toolbar={<Space wrap style={{ width: '100%', justifyContent: 'space-between' }}>
          <Space wrap><Button type="primary" disabled={busy} style={touchStyle} onClick={() => { createForm.resetFields(); setError(undefined); setNotice(undefined); setCreateOpen(true); }}>{t.create}</Button>
            <Button disabled={busy} style={touchStyle} onClick={() => void run(refresh, 'refreshed')}>{t.refresh}</Button></Space>
          <Space wrap><Input.Search aria-label={t.search} placeholder={t.search} value={searchDraft} disabled={busy} allowClear style={{ width: mobile ? '100%' : 240 }} onChange={event => setSearchDraft(event.target.value)} onSearch={value => {
            if (controller.current) return;
            const term = value.trim(); setSearch(term); void run(signal => loadList(signal, 1, term));
          }} />
            <Switch checked={autoRefresh} onChange={setAutoRefresh} aria-label={t.auto} /><Typography.Text>{t.auto}</Typography.Text>
            <DataTableColumnSettings columns={columns} configuration={configuration} ariaLabel={t.columns} placeholder={t.columns} resetLabel={t.resetColumns} />
          </Space>
        </Space>} />
    </DataTableLayout>
    {!browser_id && !createOpen && feedback}
    <Modal open={createOpen} title={t.create} footer={null} onCancel={() => { if (!busy) { setCreateOpen(false); setError(undefined); setNotice(undefined); } }} closable={!busy} maskClosable={!busy} keyboard={!busy} getContainer={false}>
      <Form form={createForm} name="browser_obscura_create" layout="vertical" disabled={busy} onFinish={values => void run(async signal => {
        const body: { name: string; idle_timeout_ms?: number } = { name: values.name.trim() };
        if (values.idle_timeout_ms != null) body.idle_timeout_ms = values.idle_timeout_ms;
        const result = await call<{ browser: BrowserSummary }>('/browsers', 'POST', signal, body);
        if (!result.success) return result;
        if (mounted.current && !signal.aborted) { setCreateOpen(false); createForm.resetFields(); }
        return loadList(signal, 1);
      }, 'created')}>
        <Form.Item name="name" label={t.name} rules={[{ required: true, whitespace: true, message: t.required }]}><Input autoComplete="off" /></Form.Item>
        <Form.Item name="idle_timeout_ms" label={t.idle} extra={t.idleHelp}><InputNumber min={1} precision={0} style={{ width: '100%' }} /></Form.Item>
        <Space><Button type="primary" htmlType="submit" loading={busy} style={touchStyle}>{t.create}</Button><Button disabled={busy} style={touchStyle} onClick={() => { setCreateOpen(false); setError(undefined); setNotice(undefined); }}>{t.cancel}</Button></Space>
      </Form>
      {createOpen && feedback}
    </Modal>
    <Drawer open={Boolean(browser_id)} title={t.detail} width={mobile ? '100%' : 720} getContainer={false} rootStyle={{ position: 'fixed' }} maskClosable={!busy} keyboard={!busy} closable={!busy} onClose={closeDetail} afterOpenChange={open => { if (!open) actionOrigin.current?.focus(); }} extra={<Button disabled={busy} style={touchStyle} onClick={closeDetail}>{t.done}</Button>}>
      {feedback}
      {browser_id && <Button disabled={busy} style={touchStyle} onClick={() => void run(signal => loadDetail(signal, browser_id), 'refreshed')}>{t.refresh}</Button>}
      {detail && <>
        <Descriptions column={1} size="small" style={{ marginBlock: 16 }}>
          <Descriptions.Item label={t.name}>{detail.browser.name}</Descriptions.Item>
          <Descriptions.Item label={t.browser_id}><Typography.Text copyable style={{ overflowWrap: 'anywhere' }}>{detail.browser.browser_id}</Typography.Text></Descriptions.Item>
          <Descriptions.Item label={t.status}><Tag color="success">{t.running}</Tag></Descriptions.Item>
          <Descriptions.Item label={t.tab_count}>{detail.browser.tab_count}</Descriptions.Item>
          <Descriptions.Item label={t.idle}>{detail.browser.idle_timeout_ms}</Descriptions.Item>
        </Descriptions>
        <Divider orientation="left">{t.tabs}</Divider>
        <Form form={tabForm} name="browser_obscura_new_tab" layout="vertical" disabled={busy} onFinish={values => void run(signal => updateTabs(signal, async () => {
          const result = await call<{ tab: TabSummary }>(`${browserPath(detail.browser.browser_id)}/tabs`, 'POST', signal, values.url?.trim() ? { url: values.url.trim() } : {});
          if (result.success && mounted.current && !signal.aborted) { setTabId(result.tab.tab_id); tabForm.resetFields(); }
          return result;
        }, detail.browser.browser_id), 'tabCreated')}>
          <Form.Item name="url" label={t.initialUrl}><Input autoComplete="off" /></Form.Item>
          <Button htmlType="submit" disabled={busy} style={touchStyle}>{t.newTab}</Button>
        </Form>
        <List dataSource={detail.tabs} locale={{ emptyText: <Empty description={t.noTabs} image={Empty.PRESENTED_IMAGE_SIMPLE} /> }} style={{ marginBlock: 16 }} renderItem={tab => <List.Item>
          <div data-testid={`browser-tab-${tab.tab_id}`} style={{ width: '100%', minWidth: 0 }}>
            <Space wrap><Typography.Text strong>{tab.title || t.untitled}</Typography.Text><Tag color={tab.status === 'failed' ? 'error' : tab.status === 'ready' ? 'success' : 'default'}>{t[tab.status]}</Tag></Space>
            <Typography.Paragraph style={{ overflowWrap: 'anywhere', marginBlock: 8 }}>{tab.url}</Typography.Paragraph>
            <Space wrap><Button type={tab.tab_id === tab_id ? 'primary' : 'default'} disabled={busy} style={touchStyle} aria-pressed={tab.tab_id === tab_id} onClick={() => { setTabId(tab.tab_id); setScreenshot(undefined); }}>{t.openTab}</Button>
              <Popconfirm title={t.closeTabConfirm} okText={t.closeTab} cancelText={t.cancel} disabled={busy} onConfirm={() => run(signal => updateTabs(signal, () => call(tabPath(tab.browser_id, tab.tab_id), 'DELETE', signal), tab.browser_id), 'tabClosed')}><Button danger disabled={busy} style={touchStyle}>{t.closeTab}</Button></Popconfirm>
            </Space>
          </div>
        </List.Item>} />
        {selectedTab && <section aria-label={t.selected}>
          <Divider orientation="left">{t.selected}</Divider>
          <Typography.Paragraph strong>{selectedTab.title || t.untitled}</Typography.Paragraph>
          <Form form={navigateForm} name="browser_obscura_navigate" layout="vertical" disabled={busy} onFinish={values => void run(signal => updateTabs(signal, () => call(`${tabPath(selectedTab.browser_id, selectedTab.tab_id)}/navigate`, 'POST', signal, { url: values.url.trim() }), selectedTab.browser_id), 'navigated')}>
            <Form.Item name="url" label={t.url} rules={[{ required: true, whitespace: true, message: t.required }]}><Input autoComplete="off" /></Form.Item>
            <Space wrap><Button htmlType="submit" type="primary" disabled={busy} style={touchStyle}>{t.navigate}</Button>
              <Button disabled={busy} style={touchStyle} onClick={() => void run(async signal => {
                const result = await call<Screenshot>(`${tabPath(selectedTab.browser_id, selectedTab.tab_id)}/screenshot`, 'POST', signal, {});
                if (result.success && mounted.current && !signal.aborted) setScreenshot(result);
                return result;
              }, 'captured')}>{t.screenshot}</Button></Space>
          </Form>
          {screenshot && <figure data-testid="browser-screenshot" style={{ margin: '16px 0' }}>
            <Typography.Title level={5}>{t.preview}</Typography.Title>
            <img src={`data:${screenshot.mime_type};base64,${screenshot.data}`} alt={t.screenshotAlt} style={{ maxWidth: '100%', height: 'auto', display: 'block' }} />
            <figcaption style={{ overflowWrap: 'anywhere' }}>{screenshot.metadata.title} · {screenshot.metadata.url} · {screenshot.metadata.width} × {screenshot.metadata.height}</figcaption>
          </figure>}
        </section>}
      </>}
    </Drawer>
  </section>;
}
