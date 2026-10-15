import React, { useEffect, useRef, useState } from 'react';
import { Alert, Button, Card, Form, Input, Select, Space, Switch, Typography } from 'antd';
import { request } from '@1flowbase/plugin-settings';

type Application = { application_id: string; name: string; application_type: string };
type Receipt = { exported_records?: number; exported_spans?: number; trace_ids?: string[]; imported_traces?: number; partial_committed_records?: number; accepted_events?: number; duplicate_events?: number; record_ids?: string[] };
type Result = { success: boolean; base_url?: string; credential_configured?: boolean; items?: Application[]; receipt?: Receipt; connected?: boolean; error?: { code: string; status?: number; details?: { progress?: Receipt; cause_details?: unknown } } };
const root = '/api/console/managed-services/langfuse';
const copy = {
  en: { title:'Langfuse', intro:'Save the project connection, export application logs, or import complete Langfuse traces into AgentLogs.', language:'中文', connection:'1. Project connection', address:'Service URL', public:'Public key', secret:'Secret key', configured:'Credentials are stored by the host. Leave both key fields empty to keep them.', missing:'Enter both project API keys.', save:'Save', reload:'Reload', test:'Test connection', connected:'Connected', export:'2. Export application logs', source:'Source application', from:'Started at or after (RFC3339)', to:'Started before (RFC3339)', exportAction:'Export complete records', import:'3. Import into AgentLogs', target:'Target AgentLogs application', byDate:'Select traces by date', traces:'Trace IDs (one per line or comma-separated)', importAction:'Import complete traces', done:'Completed', failed:'Operation failed', progress:'Confirmed progress', busy:'Working…', range:'Enter a valid date range with timezone; the end is exclusive.', select:'Select an application', refreshed:'Saved configuration reloaded', saved:'Connection saved', none:'No matching records or traces', counts:'Receipt', exported_records:'Exported records', exported_spans:'Exported spans', imported_traces:'Imported traces', partial_committed_records:'Records committed in the partial step', accepted_events:'Accepted events', duplicate_events:'Duplicate events', records:'Record IDs', traceLabel:'Trace IDs', selectTrace:'Enter at least one trace ID', incomplete:'Earlier steps may have committed. Review the confirmed progress and retry; repeated imports are deduplicated.' },
  zh: { title:'Langfuse', intro:'保存项目连接，导出应用日志，或将完整 Langfuse Trace 导入 AgentLogs。', language:'English', connection:'1. 项目连接', address:'服务地址', public:'Public key', secret:'Secret key', configured:'凭据由宿主保存；两个密钥框均留空可保留现有凭据。', missing:'请输入项目的两个 API 密钥。', save:'保存', reload:'重新加载', test:'测试连接', connected:'连接成功', export:'2. 导出应用日志', source:'来源应用', from:'开始时间下界（含，RFC3339）', to:'开始时间上界（不含，RFC3339）', exportAction:'导出完整记录', import:'3. 导入到 AgentLogs', target:'目标 AgentLogs 应用', byDate:'按日期选择 Trace', traces:'Trace ID（换行或逗号分隔）', importAction:'导入完整 Trace', done:'已完成', failed:'操作失败', progress:'已确认进度', busy:'处理中…', range:'请输入含时区的有效日期范围；结束时间不包含在内。', select:'请选择应用', refreshed:'已重新加载保存的配置', saved:'连接已保存', none:'没有匹配的记录或 Trace', counts:'实际回执', exported_records:'已导出记录', exported_spans:'已导出 Span', imported_traces:'已导入 Trace', partial_committed_records:'部分失败步骤已提交记录', accepted_events:'已接收事件', duplicate_events:'重复事件', records:'记录 ID', traceLabel:'Trace ID', selectTrace:'请至少输入一个 Trace ID', incomplete:'前序步骤可能已经提交。请查看已确认进度后重试；重复导入会去重。' }
};
export default function LangfuseSettingsPage() {
  const [language, setLanguage] = useState<'en'|'zh'>(() => (document.documentElement.lang || navigator.language).toLowerCase().startsWith('zh') ? 'zh' : 'en');
  const t = copy[language];
  const [configForm] = Form.useForm();
  const [exportForm] = Form.useForm();
  const [importForm] = Form.useForm();
  const [configured, setConfigured] = useState(false);
  const [applications, setApplications] = useState<Application[]>([]);
  const [byDate, setByDate] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState('');
  const [result, setResult] = useState<Result>();
  const controller = useRef<AbortController | null>(null);
  const mounted = useRef(true);
  async function call(path: string, method: 'GET'|'PUT'|'POST', body?: unknown, signal?: AbortSignal) {
    return request<Result>({path:`${root}/${path}`,method,body,signal});
  }
  async function run(action: (signal: AbortSignal) => Promise<Result>, success?: string) {
    if (controller.current) return;
    const active = new AbortController();
    controller.current = active;
    setBusy(true); setNotice(''); setResult(undefined);
    try {
      const response = await action(active.signal);
      if (!mounted.current) return;
      setResult(response);
      if (response.success && success) setNotice(success);
    } catch (error) {
      if (mounted.current && !active.signal.aborted) {
        // The request adapter provides safe host errors. Do not expose submitted bodies.
        setResult({success:false,error:{code:error instanceof Error ? error.message : 'request_failed'}});
      }
    } finally {
      if (controller.current === active) {
        controller.current = null;
        if (mounted.current) setBusy(false);
      }
    }
  }
  async function load(signal: AbortSignal) {
    const config = await call('config','GET',undefined,signal);
    if (!config.success) return config;
    if (mounted.current) {
      setConfigured(Boolean(config.credential_configured));
      configForm.setFieldsValue({base_url:config.base_url, public_key:'',secret_key:''});
    }
    const apps = await call('applications','GET',undefined,signal);
    if (apps.success && mounted.current) setApplications(apps.items || []);
    return apps;
  }
  useEffect(() => {
    mounted.current = true;
    run(load);
    return () => { mounted.current = false; controller.current?.abort(); controller.current = null; };
  }, []);
  function range(values: { started_at_from: string; started_at_to: string }) {
    const from = values.started_at_from?.trim(); const to = values.started_at_to?.trim();
    const timezone = /(Z|[+-]\d{2}:\d{2})$/i;
    if (!timezone.test(from || '') || !timezone.test(to || '') || !Number.isFinite(Date.parse(from)) || !Number.isFinite(Date.parse(to)) || Date.parse(from) >= Date.parse(to)) throw new Error(t.range);
    return {started_at_from:from,started_at_to:to};
  }
  const applicationOptions = applications.map(app => ({value:app.application_id,label:`${app.name} (${app.application_type})`}));
  const targetOptions = applications.filter(app => app.application_type === 'agent_logs').map(app => ({value:app.application_id,label:app.name}));
  const receipt = result?.receipt || result?.error?.details?.progress;
  const dateFields = <><Form.Item name="started_at_from" label={t.from} rules={[{required:true}]}><Input placeholder="2026-10-01T00:00:00+08:00" /></Form.Item><Form.Item name="started_at_to" label={t.to} rules={[{required:true}]}><Input placeholder="2026-10-16T00:00:00+08:00" /></Form.Item></>;
  return <section style={{maxWidth:960, margin:'0 auto',padding:16}} data-testid="langfuse-settings">
    <Space style={{width:'100%',justifyContent:'space-between'}}><Typography.Title level={3}>{t.title}</Typography.Title><Button size="small" onClick={() => setLanguage(language === 'en' ? 'zh' : 'en')}>{t.language}</Button></Space>
    <Typography.Paragraph type="secondary">{t.intro}</Typography.Paragraph>
    <Space direction="vertical" size="middle" style={{width:'100%'}}>
      <Card title={t.connection} size="small">
        <Form name="langfuse_config" form={configForm} layout="vertical" disabled={busy} onFinish={values => run(async signal => {
          const body: {base_url:string;public_key?:string;secret_key?:string} = {base_url:values.base_url};
          if (values.public_key || values.secret_key) { body.public_key=values.public_key;body.secret_key=values.secret_key; }
          const saved = await call('config','PUT',body,signal);
          if (saved.success && mounted.current) { setConfigured(Boolean(saved.credential_configured));configForm.setFieldsValue({public_key:'',secret_key:''}); }
          return saved;
        },t.saved)}>
          <Form.Item name="base_url" label={t.address} rules={[{required:true}]}><Input placeholder="https://cloud.langfuse.com/" autoComplete="off" /></Form.Item>
          <Form.Item name="public_key" label={t.public}><Input.Password autoComplete="new-password" visibilityToggle={false} /></Form.Item>
          <Form.Item name="secret_key" label={t.secret}><Input.Password autoComplete="new-password" visibilityToggle={false} /></Form.Item>
          <Typography.Paragraph type="secondary">{configured ? t.configured : t.missing}</Typography.Paragraph>
          <Space wrap><Button type="primary" htmlType="submit" loading={busy}>{t.save}</Button><Button onClick={() => run(load,t.refreshed)}>{t.reload}</Button><Button disabled={!configured} onClick={() => run(signal => call('test','POST',{},signal),t.connected)}>{t.test}</Button></Space>
        </Form>
      </Card>
      <Card title={t.export} size="small">
        <Form name="langfuse_export" form={exportForm} layout="vertical" disabled={busy} onFinish={values => run(signal => call('export','POST',{application_id:values.application_id,...range(values)},signal))}>
          <Form.Item name="application_id" label={t.source} rules={[{required:true}]}><Select options={applicationOptions} placeholder={t.select} /></Form.Item>
          {dateFields}<Button type="primary" htmlType="submit" disabled={!configured || busy} loading={busy}>{t.exportAction}</Button>
        </Form>
      </Card>
      <Card title={t.import} size="small">
        <Form name="langfuse_import" form={importForm} layout="vertical" disabled={busy} onFinish={values => run(signal => {
          const selector = byDate ? range(values) : {trace_ids:String(values.trace_ids || '').split(/[\s,]+/).filter(Boolean)};
          if ('trace_ids' in selector && !selector.trace_ids.length) throw new Error(t.selectTrace);
          return call('import','POST',{application_id:values.application_id,...selector},signal);
        })}>
          <Form.Item name="application_id" label={t.target} rules={[{required:true}]}><Select options={targetOptions} placeholder={t.select} /></Form.Item>
          <Space style={{marginBottom:16}}><Switch checked={byDate} disabled={busy} onChange={setByDate} aria-label={t.byDate}/><Typography.Text>{t.byDate}</Typography.Text></Space>
          {byDate ? dateFields : <Form.Item name="trace_ids" label={t.traces} rules={[{required:true}]}><Input.TextArea rows={3}/></Form.Item>}
          <Button type="primary" htmlType="submit" disabled={!configured || busy} loading={busy}>{t.importAction}</Button>
        </Form>
      </Card>
      {notice && <Alert type="success" showIcon message={notice}/>}
      {result && !result.success && <Alert type="error" showIcon message={t.failed} description={<><Typography.Text code>{result.error?.code}</Typography.Text><p>{t.incomplete}</p></>}/>}
      {receipt && <Card size="small" title={result?.success ? t.counts : t.progress} data-testid="langfuse-receipt">
        {(['exported_records','exported_spans','imported_traces','partial_committed_records','accepted_events','duplicate_events'] as const).filter(key => receipt[key] !== undefined).map(key => <Typography.Paragraph key={key}>{t[key]}: <strong>{receipt[key]}</strong></Typography.Paragraph>)}
        {receipt.trace_ids?.length ? <Typography.Paragraph style={{overflowWrap:'anywhere'}}>{t.traceLabel}: {receipt.trace_ids.join(', ')}</Typography.Paragraph> : null}
        {receipt.record_ids?.length ? <Typography.Paragraph style={{overflowWrap:'anywhere'}}>{t.records}: {receipt.record_ids.join(', ')}</Typography.Paragraph> : null}
        {Boolean(result?.error?.details?.cause_details) && <pre style={{whiteSpace:'pre-wrap',overflowWrap:'anywhere'}}>{JSON.stringify(result.error.details.cause_details,null,2)}</pre>}
      </Card>}
    </Space>
  </section>;
}
