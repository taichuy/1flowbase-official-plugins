import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Alert, Button, Descriptions, Divider, Drawer, Form, Grid, Input, Popconfirm, Select, Space, Switch, Typography } from 'antd';
import { DataTable, DataTableColumnSettings, DataTableLayout, usePersistedDataTableConfiguration } from '@1flowbase/data-table';
import { request } from '@1flowbase/plugin-settings';

type Instance = { instance_id: string; name: string; base_url: string; credential_configured: boolean };
type Application = { application_id: string; name: string; application_type: string };
type Receipt = { exported_records?: number; exported_spans?: number; trace_ids?: string[]; imported_traces?: number; partial_committed_records?: number; accepted_events?: number; duplicate_events?: number; record_ids?: string[] };
type StepDetails = Receipt & { committed_records?: number; rejected_spans?: number; acknowledgement_warning?: boolean };
type Result = { success: boolean; receipt?: Receipt; error?: { code: string; status?: number; details?: { progress?: Receipt; cause_details?: StepDetails } } };
type InstanceList = Result & { items: Instance[]; page: number; page_size: number; total: number };
type ApplicationList = Result & { items: Application[] };
type DrawerState = { kind: 'instance'; instance?: Instance } | { kind: 'export' | 'import'; instance: Instance };
type Column = { key: string; title: string; width: number; dataIndex?: keyof Instance; sizing?: 'fill'; mobileRole?: 'title' | 'actions'; render?: (value: unknown, record: Instance) => React.ReactNode };
const root = '/api/console/managed-services/langfuse';
const copy = {
  en: { title:'Langfuse', intro:'Manage project connections and exchange complete application logs with each Langfuse instance.', instance:'Instance', add:'Add instance', edit:'Edit', name:'Name', address:'Service URL', public:'Public key', secret:'Secret key', credentials:'Credentials configured', yes:'Yes', no:'No', configured:'Credentials are stored by the host. Leave both key fields empty to keep this instance’s keys.', missing:'Enter both project API keys.', save:'Save', cancel:'Cancel', reload:'Reload', test:'Test connection', connected:'Connected', export:'Export', exportTitle:'Export application logs', source:'Source application', from:'Started at or after (RFC3339)', to:'Started before (RFC3339)', exportAction:'Export complete records', import:'Import', importTitle:'Import into AgentLogs', target:'Target AgentLogs application', byDate:'Select traces by date', traces:'Trace IDs (one per line or comma-separated)', importAction:'Import complete traces', failed:'Operation failed', progress:'Confirmed progress', range:'Enter a valid date range with timezone; the end is exclusive.', select:'Select an application', refreshed:'Saved instances reloaded', saved:'Connection saved', deleted:'Instance deleted', delete:'Delete', deleteConfirm:'Delete this instance and its saved credentials?', empty:'No instances', actions:'Actions', columns:'Columns', resetColumns:'Reset columns', counts:'Receipt', step:'Current step', exported_records:'Exported records', exported_spans:'Exported spans', imported_traces:'Imported traces', partial_committed_records:'Records committed in the partial step', accepted_events:'Accepted events', duplicate_events:'Duplicate events', committed_records:'Committed records', rejected_spans:'Rejected spans', acknowledgement_warning:'Acknowledgement warning', records:'Record IDs', traceLabel:'Trace IDs', selectTrace:'Enter at least one trace ID', incomplete:'Earlier steps may have committed. Review the confirmed progress and retry; repeated imports are deduplicated.' },
  zh: { title:'Langfuse', intro:'管理项目连接，与各 Langfuse 实例交换完整应用日志。', instance:'实例', add:'添加实例', edit:'编辑', name:'名称', address:'服务地址', public:'Public key', secret:'Secret key', credentials:'已配置凭据', yes:'是', no:'否', configured:'凭据由宿主保存；两个密钥框均留空可保留此实例的密钥。', missing:'请输入项目的两个 API 密钥。', save:'保存', cancel:'取消', reload:'重新加载', test:'测试连接', connected:'连接成功', export:'导出', exportTitle:'导出应用日志', source:'来源应用', from:'开始时间下界（含，RFC3339）', to:'开始时间上界（不含，RFC3339）', exportAction:'导出完整记录', import:'导入', importTitle:'导入到 AgentLogs', target:'目标 AgentLogs 应用', byDate:'按日期选择 Trace', traces:'Trace ID（换行或逗号分隔）', importAction:'导入完整 Trace', failed:'操作失败', progress:'已确认进度', range:'请输入含时区的有效日期范围；结束时间不包含在内。', select:'请选择应用', refreshed:'已重新加载保存的实例', saved:'连接已保存', deleted:'实例已删除', delete:'删除', deleteConfirm:'删除此实例及其保存的凭据？', empty:'暂无实例', actions:'操作', columns:'显示列', resetColumns:'重置显示列', counts:'实际回执', step:'当前步骤', exported_records:'已导出记录', exported_spans:'已导出 Span', imported_traces:'已导入 Trace', partial_committed_records:'部分失败步骤已提交记录', accepted_events:'已接收事件', duplicate_events:'重复事件', committed_records:'已提交记录', rejected_spans:'已拒绝 Span', acknowledgement_warning:'确认回执警告', records:'记录 ID', traceLabel:'Trace ID', selectTrace:'请至少输入一个 Trace ID', incomplete:'前序步骤可能已经提交。请查看已确认进度后重试；重复导入会去重。' }
};
type Copy = typeof copy.en;
const receiptKeys = ['exported_records','exported_spans','imported_traces','partial_committed_records','accepted_events','duplicate_events'] as const;
const stepKeys = ['committed_records','rejected_spans','acknowledgement_warning'] as const;
function ReceiptFields({ receipt, t }: { receipt: Receipt; t: Copy }) {
  return <>
    {receiptKeys.filter(key => receipt[key] !== undefined).map(key => <Typography.Paragraph key={key}>{t[key]}: <strong>{receipt[key]}</strong></Typography.Paragraph>)}
    {receipt.trace_ids?.length ? <Typography.Paragraph style={{overflowWrap:'anywhere'}}>{t.traceLabel}: {receipt.trace_ids.join(', ')}</Typography.Paragraph> : null}
    {receipt.record_ids?.length ? <Typography.Paragraph style={{overflowWrap:'anywhere'}}>{t.records}: {receipt.record_ids.join(', ')}</Typography.Paragraph> : null}
  </>;
}
function OperationFeedback({ result, notice, exchange, t }: { result?: Result; notice?: string; exchange: boolean; t: Copy }) {
  const receipt = result?.receipt || result?.error?.details?.progress;
  const step = result?.error?.details?.cause_details;
  const showStep = step && ([...receiptKeys,...stepKeys,'trace_ids','record_ids'] as const).some(key => step[key] !== undefined);
  return <>
    {notice && <Alert type="success" showIcon message={notice}/>}
    {result && !result.success && <Alert type="error" showIcon message={t.failed} description={<><Typography.Text>{result.error?.code}</Typography.Text>{exchange && <p>{t.incomplete}</p>}</>}/>}
    {receipt && <section data-testid="langfuse-receipt" aria-label={result?.success ? t.counts : t.progress}>
      <Divider orientation="left">{result?.success ? t.counts : t.progress}</Divider>
      <ReceiptFields receipt={receipt} t={t}/>
      {showStep && step && <>
        <Divider orientation="left">{t.step}</Divider>
        <ReceiptFields receipt={step} t={t}/>
        {stepKeys.filter(key => step[key] !== undefined).map(key => <Typography.Paragraph key={key}>{t[key]}: <strong>{typeof step[key] === 'boolean' ? (step[key] ? t.yes : t.no) : step[key]}</strong></Typography.Paragraph>)}
      </>}
    </section>}
  </>;
}
export default function LangfuseSettingsPage({ ctx }: { ctx: { ui: { locale?: string } } }) {
  const t = copy[ctx.ui.locale?.toLowerCase().startsWith('zh') ? 'zh' : 'en'];
  const screens = Grid.useBreakpoint();
  const mobile = screens.md === false;
  const [configForm] = Form.useForm();
  const [exportForm] = Form.useForm();
  const [importForm] = Form.useForm();
  const [list, setList] = useState<InstanceList>({success:true,items:[],page:1,page_size:50,total:0});
  const [applications, setApplications] = useState<Application[]>([]);
  const [drawer, setDrawer] = useState<DrawerState>();
  const [byDate, setByDate] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<'saved' | 'refreshed' | 'connected' | 'deleted'>();
  const [result, setResult] = useState<Result>();
  const controller = useRef<AbortController | null>(null);
  const mounted = useRef(true);
  const previousMobile = useRef(mobile);
  async function call<T extends Result = Result>(path: string, method: 'GET' | 'PUT' | 'POST' | 'DELETE', body?: unknown, signal?: AbortSignal) {
    return request<T>({path:`${root}/${path}`,method,body,signal});
  }
  async function run(action: (signal: AbortSignal) => Promise<Result>, success?: typeof notice) {
    if (controller.current) return;
    const active = new AbortController();
    controller.current = active;
    setBusy(true); setNotice(undefined); setResult(undefined);
    try {
      const response = await action(active.signal);
      if (!mounted.current || active.signal.aborted) return;
      setResult(response);
      if (response.success && success) setNotice(success);
    } catch (error) {
      if (mounted.current && !active.signal.aborted) {
        // The request adapter provides safe host errors. Never display submitted bodies.
        setResult({success:false,error:{code:error instanceof Error ? error.message : 'request_failed'}});
      }
    } finally {
      if (controller.current === active) {
        controller.current = null;
        if (mounted.current) setBusy(false);
      }
    }
  }
  async function load(signal: AbortSignal, page = 1, append = false) {
    const response = await call<InstanceList>(`instances?page=${page}&page_size=50`,'GET',undefined,signal);
    if (response.success && mounted.current && !signal.aborted) {
      setList(current => ({...response,items:append ? [...current.items,...response.items] : response.items}));
    }
    return response;
  }
  useEffect(() => {
    mounted.current = true;
    void run(async signal => {
      const instances = await load(signal);
      if (!instances.success) return instances;
      const apps = await call<ApplicationList>('applications','GET',undefined,signal);
      if (apps.success && mounted.current && !signal.aborted) setApplications(apps.items);
      return apps;
    });
    return () => { mounted.current = false; controller.current?.abort(); controller.current = null; };
  }, []);
  useEffect(() => {
    if (previousMobile.current === mobile || busy || drawer) return;
    previousMobile.current = mobile;
    if (list.page > 1) void run(signal => load(signal));
  }, [mobile, busy, drawer]);
  function open(next: DrawerState) {
    if (controller.current) return;
    setResult(undefined); setNotice(undefined); setByDate(false);
    if (next.kind === 'instance') {
      configForm.resetFields();
      configForm.setFieldsValue({name:next.instance?.name ?? '',base_url:next.instance?.base_url ?? '',public_key:'',secret_key:''});
    } else if (next.kind === 'export') exportForm.resetFields();
    else importForm.resetFields();
    setDrawer(next);
  }
  function close() {
    if (controller.current) return;
    if (drawer?.kind === 'instance') configForm.resetFields();
    setDrawer(undefined); setResult(undefined); setNotice(undefined);
  }
  function range(values: { started_at_from: string; started_at_to: string }) {
    const from = values.started_at_from?.trim(); const to = values.started_at_to?.trim();
    const timezone = /(Z|[+-]\d{2}:\d{2})$/i;
    if (!timezone.test(from || '') || !timezone.test(to || '') || !Number.isFinite(Date.parse(from)) || !Number.isFinite(Date.parse(to)) || Date.parse(from) >= Date.parse(to)) throw new Error(t.range);
    return {started_at_from:from,started_at_to:to};
  }
  const columns = useMemo<Column[]>(() => [
    {key:'name',title:t.name,dataIndex:'name',width:200,mobileRole:'title'},
    {key:'base_url',title:t.address,dataIndex:'base_url',width:300,sizing:'fill',render:value => <Typography.Text style={{overflowWrap:'anywhere'}}>{String(value)}</Typography.Text>},
    {key:'credential_configured',title:t.credentials,dataIndex:'credential_configured',width:180,render:value => value ? t.yes : t.no},
    {key:'actions',title:t.actions,width:500,mobileRole:'actions',render:(_, instance) => <Space wrap data-testid={`langfuse-instance-actions-${instance.instance_id}`}>
      <Button size="small" disabled={busy} onClick={() => open({kind:'instance',instance})}>{t.edit}</Button>
      <Button size="small" disabled={busy || !instance.credential_configured} onClick={() => void run(signal => call(`instances/${encodeURIComponent(instance.instance_id)}/test`,'POST',{},signal),'connected')}>{t.test}</Button>
      <Button size="small" disabled={busy || !instance.credential_configured} onClick={() => open({kind:'export',instance})}>{t.export}</Button>
      <Button size="small" disabled={busy || !instance.credential_configured} onClick={() => open({kind:'import',instance})}>{t.import}</Button>
      <Popconfirm title={t.deleteConfirm} okText={t.delete} cancelText={t.cancel} disabled={busy} onConfirm={() => run(async signal => {
        const deleted = await call(`instances/${encodeURIComponent(instance.instance_id)}`,'DELETE',undefined,signal);
        if (!deleted.success) return deleted;
        return load(signal,mobile ? 1 : (list.items.length === 1 && list.page > 1 ? list.page - 1 : list.page));
      },'deleted')}><Button size="small" danger disabled={busy}>{t.delete}</Button></Popconfirm>
    </Space>}
  ], [t, busy, mobile, list.page, list.items.length]);
  const configuration = usePersistedDataTableConfiguration({columns,storageKey:'1flowbase.langfuse.instances.columns.v1'});
  const applicationOptions = applications.map(app => ({value:app.application_id,label:`${app.name} (${app.application_type})`}));
  const targetOptions = applications.filter(app => app.application_type === 'agent_logs').map(app => ({value:app.application_id,label:app.name}));
  const dateFields = <><Form.Item name="started_at_from" label={t.from} rules={[{required:true}]}><Input placeholder="2026-10-01T00:00:00+08:00" /></Form.Item><Form.Item name="started_at_to" label={t.to} rules={[{required:true}]}><Input placeholder="2026-10-16T00:00:00+08:00" /></Form.Item></>;
  const exchange = drawer?.kind === 'export' || drawer?.kind === 'import';
  return <section style={{width:'100%',minWidth:0,padding:16,boxSizing:'border-box'}} data-testid="langfuse-settings">
    <Typography.Title level={3}>{t.title}</Typography.Title>
    <Typography.Paragraph type="secondary">{t.intro}</Typography.Paragraph>
    <div data-testid="langfuse-instances">
      <DataTableLayout>
        <DataTable columns={columns} configuration={configuration} rowKey="instance_id" dataSource={list.items} loading={busy} emptyText={t.empty}
          page={list.page} pageSize={list.page_size} total={list.total} onPageChange={(page: number) => { if (!busy) void run(signal => load(signal,page)); }}
          mobileList={mobile ? {resetKey:'langfuse-instances',hasMore:list.page * list.page_size < list.total,loading:busy,failed:result?.success === false,onLoadMore:() => run(signal => load(signal,list.page + 1,true))} : undefined}
          toolbar={<Space wrap style={{width:'100%',justifyContent:'space-between'}}>
            <Space wrap><Button type="primary" disabled={busy} onClick={() => open({kind:'instance'})}>{t.add}</Button><Button disabled={busy} onClick={() => void run(signal => load(signal,mobile ? 1 : list.page),'refreshed')}>{t.reload}</Button></Space>
            <DataTableColumnSettings columns={columns} configuration={configuration} ariaLabel={t.columns} placeholder={t.columns} resetLabel={t.resetColumns}/>
          </Space>}/>
      </DataTableLayout>
    </div>
    {!drawer && <OperationFeedback result={result} notice={notice && t[notice]} exchange={false} t={t}/>}
    <Drawer open={Boolean(drawer)} title={drawer?.kind === 'instance' ? t.instance : drawer?.kind === 'export' ? t.exportTitle : t.importTitle}
      width={mobile ? '100%' : 560} getContainer={false} rootStyle={{position:'fixed'}} maskClosable={!busy} keyboard={!busy} closable={!busy} onClose={close}
      extra={<Button disabled={busy} onClick={close}>{t.cancel}</Button>}>
      {drawer?.kind === 'instance' && <Form name="langfuse_instance" form={configForm} layout="vertical" disabled={busy} onFinish={values => run(async signal => {
        const body: {name:string;base_url:string;public_key?:string;secret_key?:string} = {name:values.name,base_url:values.base_url};
        if (values.public_key || values.secret_key || !drawer.instance) { body.public_key=values.public_key;body.secret_key=values.secret_key; }
        const saved = await call(drawer.instance ? `instances/${encodeURIComponent(drawer.instance.instance_id)}` : 'instances',drawer.instance ? 'PUT' : 'POST',body,signal);
        if (!saved.success) return saved;
        if (mounted.current && !signal.aborted) { configForm.setFieldsValue({public_key:'',secret_key:''}); setDrawer(undefined); }
        return load(signal,mobile ? 1 : list.page);
      },'saved')}>
        <Form.Item name="name" label={t.name} rules={[{required:true,whitespace:true}]}><Input autoComplete="off" /></Form.Item>
        <Form.Item name="base_url" label={t.address} rules={[{required:true}]}><Input placeholder="https://cloud.langfuse.com/" autoComplete="off" /></Form.Item>
        <Form.Item name="public_key" label={t.public} rules={[{required:!drawer.instance?.credential_configured}]}><Input.Password autoComplete="new-password" visibilityToggle={false} /></Form.Item>
        <Form.Item name="secret_key" label={t.secret} rules={[{required:!drawer.instance?.credential_configured}]}><Input.Password autoComplete="new-password" visibilityToggle={false} /></Form.Item>
        <Typography.Paragraph type="secondary">{drawer.instance?.credential_configured ? t.configured : t.missing}</Typography.Paragraph>
        <Button type="primary" htmlType="submit" loading={busy}>{t.save}</Button>
      </Form>}
      {drawer && drawer.kind !== 'instance' && <Descriptions size="small" column={1}><Descriptions.Item label={t.instance}>{drawer.instance.name}</Descriptions.Item><Descriptions.Item label={t.address}>{drawer.instance.base_url}</Descriptions.Item></Descriptions>}
      {drawer?.kind === 'export' && <Form name="langfuse_export" form={exportForm} layout="vertical" disabled={busy} onFinish={values => run(signal => call('export','POST',{instance_id:drawer.instance.instance_id,application_id:values.application_id,...range(values)},signal))}>
        <Form.Item name="application_id" label={t.source} rules={[{required:true}]}><Select options={applicationOptions} placeholder={t.select} /></Form.Item>
        {dateFields}<Button type="primary" htmlType="submit" disabled={busy} loading={busy}>{t.exportAction}</Button>
      </Form>}
      {drawer?.kind === 'import' && <Form name="langfuse_import" form={importForm} layout="vertical" disabled={busy} onFinish={values => run(signal => {
        const selector = byDate ? range(values) : {trace_ids:String(values.trace_ids || '').split(/[\s,]+/).filter(Boolean)};
        if ('trace_ids' in selector && !selector.trace_ids.length) throw new Error(t.selectTrace);
        return call('import','POST',{instance_id:drawer.instance.instance_id,application_id:values.application_id,...selector},signal);
      })}>
        <Form.Item name="application_id" label={t.target} rules={[{required:true}]}><Select options={targetOptions} placeholder={t.select} /></Form.Item>
        <Space style={{marginBottom:16}}><Switch checked={byDate} disabled={busy} onChange={setByDate} aria-label={t.byDate}/><Typography.Text>{t.byDate}</Typography.Text></Space>
        {byDate ? dateFields : <Form.Item name="trace_ids" label={t.traces} rules={[{required:true}]}><Input.TextArea rows={3}/></Form.Item>}
        <Button type="primary" htmlType="submit" disabled={busy} loading={busy}>{t.importAction}</Button>
      </Form>}
      {drawer && <OperationFeedback result={result} notice={notice && t[notice]} exchange={exchange} t={t}/>}
    </Drawer>
  </section>;
}
