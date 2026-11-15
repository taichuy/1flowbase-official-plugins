import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Alert, Button, Drawer, Dropdown, Form, Grid, Input, Modal, Space, Switch, Tag, Typography } from 'antd';
import { DataTable, DataTableColumnSettings, DataTableLayout, usePersistedDataTableConfiguration } from '@1flowbase/data-table';
import { draftBody, hostsApi, type HostDraft, type HostList, type HostSummary } from './api';
import { copy, errorText, type Copy } from './copy';

type Context = { ui: { locale?: string }; currentUser?: { isRoot?: boolean; permissions?: readonly string[] } };
type Column = { key: string; title: string; width: number; dataIndex?: keyof HostSummary; sizing?: 'fill'; mobileRole?: 'title' | 'actions'; render?: (value: unknown, host: HostSummary) => React.ReactNode };
type Feedback = { error?: string; notice?: keyof Copy };

export default function UmamiSettingsPage({ ctx }: { ctx: Context }) {
  const t = copy[ctx.ui.locale?.toLowerCase().startsWith('zh') ? 'zh' : 'en'];
  const can = (operation: string) => ctx.currentUser?.isRoot === true || ctx.currentUser?.permissions?.includes(`umami.hosts.${operation}`) === true;
  const canList = can('list');
  const mobile = Grid.useBreakpoint().md === false;
  const [form] = Form.useForm<HostDraft>();
  const [list, setList] = useState<HostList>({ items: [], total: 0, page: 1, page_size: 20 });
  const [listRevision, setListRevision] = useState(0);
  const [search, setSearch] = useState('');
  const [searchDraft, setSearchDraft] = useState('');
  const [editing, setEditing] = useState<HostSummary>();
  const [open, setOpen] = useState(false);
  const [deleting, setDeleting] = useState<HostSummary>();
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>({});
  const [draftFeedback, setDraftFeedback] = useState<Feedback>({});
  const mounted = useRef(false);
  const controller = useRef<AbortController | null>(null);
  const actionOrigin = useRef<HTMLElement | null>(null);
  const draftRevision = useRef(0);

  async function run(action: (signal: AbortSignal) => Promise<unknown>, success?: keyof Copy, draft = false) {
    if (controller.current) return;
    const active = new AbortController();
    controller.current = active;
    const revision = draftRevision.current;
    const publish = draft ? setDraftFeedback : setFeedback;
    setBusy(true); publish({});
    try {
      await action(active.signal);
      if (mounted.current && !active.signal.aborted && (!draft || revision === draftRevision.current) && success) publish({ notice: success });
    } catch (failure) {
      if (mounted.current && !active.signal.aborted && (!draft || revision === draftRevision.current)) {
        // Render only localized stable codes, never provider messages, response bodies, or credentials.
        const code = failure !== null && typeof failure === 'object' && 'code' in failure && typeof failure.code === 'string' ? failure.code : 'request_failed';
        publish({ error: code });
      }
    } finally {
      if (controller.current === active) {
        controller.current = null;
        if (mounted.current) setBusy(false);
      }
    }
  }
  async function loadList(signal: AbortSignal, page = list.page, term = search, append = false) {
    const result = await hostsApi.list(signal, page, list.page_size, term);
    if (mounted.current && !signal.aborted) {
      setList(current => ({ ...result, items: append ? [...current.items, ...result.items] : result.items }));
      if (!append) setListRevision(current => current + 1);
    }
  }
  useEffect(() => {
    mounted.current = true;
    if (canList) void run(signal => loadList(signal, 1));
    else { setList({ items: [], total: 0, page: 1, page_size: 20 }); setBusy(false); }
    return () => { mounted.current = false; controller.current?.abort(); controller.current = null; };
  }, [canList]);

  function openEditor(host?: HostSummary, origin?: HTMLElement) {
    if (busy) return;
    actionOrigin.current = origin ?? document.activeElement as HTMLElement;
    setEditing(host); form.resetFields();
    form.setFieldsValue(host ? { name: host.name, base_url: host.base_url, username: host.username, enabled: host.enabled, password: '' } : { enabled: true, password: '' });
    draftRevision.current++; setDraftFeedback({}); setFeedback({}); setOpen(true);
  }
  function closeEditor() {
    if (busy) return;
    draftRevision.current++; form.resetFields(); setOpen(false); setEditing(undefined); setDraftFeedback({});
  }
  async function testDraft() {
    if (!can('test_draft') || busy) return;
    try {
      const values = await form.validateFields();
      await run(signal => hostsApi.testDraft(signal, draftBody(values), editing?.host_id), 'tested', true);
    } catch {
      // Ant Design owns field-validation feedback; this branch makes no request.
    }
  }
  const touchStyle = mobile ? { minHeight: 44, minWidth: 44 } : undefined;
  const columns = useMemo<Column[]>(() => [
    { key: 'name', title: t.name, dataIndex: 'name', width: 180, sizing: 'fill', mobileRole: 'title' },
    { key: 'base_url', title: t.base_url, dataIndex: 'base_url', width: 260, render: value => <Typography.Text style={{ overflowWrap: 'anywhere' }}>{String(value)}</Typography.Text> },
    { key: 'username', title: t.username, dataIndex: 'username', width: 150 },
    { key: 'enabled', title: t.status, dataIndex: 'enabled', width: 110, render: value => <Tag color={value ? 'success' : 'default'}>{value ? t.enabled : t.disabled}</Tag> },
    { key: 'has_password', title: t.has_password, dataIndex: 'has_password', width: 140, render: value => value ? t.configured : t.missing },
    { key: 'actions', title: t.actions, width: 260, mobileRole: 'actions', render: (_, host) => {
      const actions = [
        ...(can('test') ? [{ key: 'test', label: t.test, disabled: busy || !host.enabled, onClick: () => void run(signal => hostsApi.test(signal, host.host_id), 'tested') }] : []),
        ...(can('update') ? [{ key: 'edit', label: t.editAction, disabled: busy, onClick: () => openEditor(host) }] : []),
        ...(can('delete') ? [{ key: 'delete', label: t.delete, disabled: busy, danger: true, onClick: () => { setFeedback({}); setDeleting(host); } }] : []),
      ];
      return <Space size={4} data-testid={`umami-actions-${host.host_id}`}>
        {actions.slice(0, 2).map(action => <Button key={action.key} type="link" size="small" style={touchStyle} disabled={action.disabled} danger={'danger' in action && action.danger} onClick={action.onClick}>{action.label}</Button>)}
        {actions.length > 2 && <Dropdown trigger={['click']} menu={{ items: actions.slice(2) }}><Button type="text" size="small" style={touchStyle} disabled={busy} aria-label={t.more}>{t.more}</Button></Dropdown>}
      </Space>;
    } },
  ], [t, busy, mobile, ctx.currentUser, list.page, list.items.length, search]);
  const configuration = usePersistedDataTableConfiguration({ columns, storageKey: '1flowbase.umami.hosts.columns.v1' });
  const feedbackView = (value: Feedback) => <div aria-live="polite" style={{ marginBlock: 12 }}>
    {value.error && <Alert type="error" showIcon message={errorText(value.error, t)} />}
    {value.notice && <Alert type="success" showIcon message={t[value.notice]} />}
  </div>;

  return <section data-testid="umami-settings" style={{ width: '100%', minWidth: 0, padding: 16, boxSizing: 'border-box' }}>
    <Typography.Title level={3}>{t.title}</Typography.Title>
    <Typography.Paragraph type="secondary">{t.intro}</Typography.Paragraph>
    <Typography.Paragraph><Typography.Link href="/settings/mcp-management">{t.mcp}</Typography.Link> — {t.mcpHelp}</Typography.Paragraph>
    {!canList ? <Alert type="warning" showIcon message={t.noList} /> : <DataTableLayout>
      <DataTable columns={columns} configuration={configuration} rowKey="host_id" dataSource={list.items} loading={busy} emptyText={feedback.error ? errorText(feedback.error, t) : t.empty}
        page={list.page} pageSize={list.page_size} total={list.total} onPageChange={(page: number) => { if (!busy) void run(signal => loadList(signal, page)); }}
        mobileList={mobile ? { resetKey: `umami-${search}-${listRevision}`, hasMore: list.page * list.page_size < list.total, loading: busy, failed: Boolean(feedback.error), onLoadMore: () => run(signal => loadList(signal, list.page + 1, search, true)) } : undefined}
        toolbar={<Space wrap style={{ width: '100%', justifyContent: 'space-between' }}>
          <Space wrap>{can('create') && <Button type="primary" disabled={busy} style={touchStyle} onClick={event => openEditor(undefined, event.currentTarget)}>{t.create}</Button>}
            <Button disabled={busy} style={touchStyle} onClick={() => void run(signal => loadList(signal, mobile ? 1 : list.page), 'refreshed')}>{t.refresh}</Button></Space>
          <Space wrap><Input.Search aria-label={t.search} placeholder={t.search} value={searchDraft} disabled={busy} allowClear style={{ width: mobile ? '100%' : 240 }} onChange={event => setSearchDraft(event.target.value)} onSearch={value => {
            if (busy) return;
            const term = value.trim(); setSearch(term); void run(signal => loadList(signal, 1, term));
          }} />
            <DataTableColumnSettings columns={columns} configuration={configuration} ariaLabel={t.columns} placeholder={t.columns} resetLabel={t.resetColumns} />
          </Space>
        </Space>} />
    </DataTableLayout>}
    {!open && !deleting && feedbackView(feedback)}
    <Drawer open={open} title={editing ? t.edit : t.create} width={mobile ? '100%' : 480} getContainer={false} rootStyle={{ position: 'fixed' }} maskClosable={!busy} keyboard={!busy} closable={!busy} onClose={closeEditor} afterOpenChange={visible => { if (!visible) actionOrigin.current?.focus(); }}>
      <Form form={form} name="umami_connection" layout="vertical" disabled={busy} onValuesChange={() => { draftRevision.current++; setDraftFeedback({}); setFeedback({}); }} onFinish={values => {
        if (!can(editing ? 'update' : 'create')) return;
        void run(async signal => {
          await hostsApi.save(signal, draftBody(values), editing?.host_id);
          if (!mounted.current || signal.aborted) return;
          form.resetFields(); setOpen(false); setEditing(undefined); setDraftFeedback({});
          await loadList(signal, 1);
        }, editing ? 'updated' : 'created');
      }}>
        <Form.Item name="name" label={t.name} rules={[{ required: true, whitespace: true, message: t.required }]}><Input autoComplete="off" /></Form.Item>
        <Form.Item name="base_url" label={t.base_url} rules={[{ required: true, whitespace: true, message: t.required }]}><Input autoComplete="url" inputMode="url" /></Form.Item>
        <Form.Item name="username" label={t.username} rules={[{ required: true, whitespace: true, message: t.required }]}><Input autoComplete="off" /></Form.Item>
        <Form.Item name="password" label={t.password} extra={editing ? t.passwordHelp : t.passwordCreateHelp} rules={editing ? [] : [{ required: true, message: t.passwordRequired }]}><Input.Password autoComplete="new-password" /></Form.Item>
        <Form.Item name="enabled" label={t.status} valuePropName="checked"><Switch aria-label={t.status} checkedChildren={t.enabled} unCheckedChildren={t.disabled} /></Form.Item>
        <Space wrap><Button type="primary" htmlType="submit" aria-label={t.save} loading={busy} disabled={!can(editing ? 'update' : 'create')} style={touchStyle}>{t.save}</Button>
          {can('test_draft') && <Button disabled={busy} style={touchStyle} onClick={() => void testDraft()}>{t.testDraft}</Button>}
          <Button disabled={busy} style={touchStyle} onClick={closeEditor}>{t.cancel}</Button></Space>
      </Form>
      {feedbackView(draftFeedback)}
      {open && feedbackView(feedback.error ? feedback : {})}
    </Drawer>
    <Modal open={Boolean(deleting)} title={t.deleteTitle} okText={t.delete} cancelText={t.cancel} okButtonProps={{ danger: true, disabled: busy || !can('delete') }} cancelButtonProps={{ disabled: busy }} confirmLoading={busy} closable={!busy} maskClosable={!busy} keyboard={!busy} onCancel={() => { if (!busy) setDeleting(undefined); }} onOk={() => {
      if (!deleting || !can('delete')) return;
      void run(async signal => {
        await hostsApi.delete(signal, deleting.host_id);
        if (mounted.current && !signal.aborted) setDeleting(undefined);
        await loadList(signal, mobile ? 1 : list.items.length === 1 && list.page > 1 ? list.page - 1 : list.page);
      }, 'deleted');
    }}>
      <Typography.Paragraph strong>{deleting?.name}</Typography.Paragraph><Typography.Paragraph>{t.deleteHelp}</Typography.Paragraph>
      {deleting && feedbackView(feedback)}
    </Modal>
  </section>;
}
