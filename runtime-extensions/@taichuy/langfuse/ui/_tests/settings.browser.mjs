import assert from 'node:assert/strict';

/** Central QA injects an authenticated Playwright page. No login or server lifecycle here. */
export async function exerciseLangfuseSettings(page) {
  const root = '/api/console/managed-services/langfuse/';
  let saved = false;
  const bodies = [];
  await page.route(`**${root}**`, async route => {
    const request = route.request();
    const operation = new URL(request.url()).pathname.slice(root.length);
    const body = request.method() === 'GET' ? {} : request.postDataJSON();
    bodies.push({operation, method:request.method(), body});
    let result;
    if (operation === 'config') {
      if (request.method() === 'PUT') {
        assert.equal(body.public_key, 'fixture-public');
        assert.equal(body.secret_key, 'fixture-secret');
        saved = true;
      }
      result = {success:true,base_url:saved ? 'https://langfuse.example/' : '',credential_configured:saved};
    } else if (operation === 'applications') {
      result = {success:true,items:[{application_id:'native',name:'Native source',application_type:'agent_flow'},{application_id:'target',name:'Historical logs',application_type:'agent_logs'}]};
    } else if (operation === 'test') result = {success:true,connected:true};
    else if (operation === 'export') {
      assert.deepEqual(body,{application_id:'native',started_at_from:'2026-10-01T00:00:00Z',started_at_to:'2026-11-01T00:00:00Z'});
      result = {success:true,receipt:{exported_records:2,exported_spans:9,trace_ids:['trace-a','trace-b']}};
    } else if (operation === 'import') {
      assert.equal(body.application_id,'target');
      assert.deepEqual(body.trace_ids,['trace-a','trace-b']);
      result = {success:false,error:{code:'source_partial',status:409,details:{progress:{imported_traces:1,accepted_events:3,duplicate_events:4,record_ids:['record-a']},cause_details:{accepted_events:1,record_ids:['record-b']}}}};
    } else throw new Error(`Unexpected managed operation: ${operation}`);
    await route.fulfill({contentType:'application/json',body:JSON.stringify({data:result})});
  });
  try {
    await page.goto(new URL('/settings/langfuse', page.url()).toString());
    const settings = page.getByTestId('langfuse-settings');
    await settings.waitFor();
    // Select the English fixture language while retaining the Chinese consumer path.
    if (await settings.getByRole('button',{name:'English',exact:true}).count()) await settings.getByRole('button',{name:'English',exact:true}).click();
    await settings.getByLabel('Service URL', {exact:true}).fill('https://langfuse.example/');
    await settings.getByLabel('Public key',{exact:true}).fill('fixture-public');
    await settings.getByLabel('Secret key',{exact:true}).fill('fixture-secret');
    await settings.getByRole('button',{name:'Save',exact:true}).click();
    await page.getByText('Connection saved',{exact:true}).waitFor();
    assert.equal(await settings.getByLabel('Public key',{exact:true}).inputValue(),'');
    assert.equal(await settings.getByLabel('Secret key',{exact:true}).inputValue(),'');
    await settings.getByRole('button',{name:'Reload',exact:true}).click();
    await page.getByText('Saved configuration reloaded',{exact:true}).waitFor();
    await settings.getByRole('button',{name:'Test connection',exact:true}).click();
    await page.getByText('Connected',{exact:true}).waitFor();
    await settings.getByLabel('Source application',{exact:true}).click();
    await page.getByText('Native source (agent_flow)',{exact:true}).last().click();
    const sourceForm = settings.locator('form').nth(1);
    await sourceForm.getByLabel('Started at or after (RFC3339)',{exact:true}).fill('2026-10-01T00:00:00Z');
    await sourceForm.getByLabel('Started before (RFC3339)',{exact:true}).fill('2026-11-01T00:00:00Z');
    await settings.getByRole('button',{name:'Export complete records',exact:true}).click();
    await settings.getByTestId('langfuse-receipt').getByText('Exported spans:',{exact:false}).waitFor();
    assert.match(await settings.getByTestId('langfuse-receipt').innerText(),/Exported spans: 9/);
    await settings.getByLabel('Target AgentLogs application',{exact:true}).click();
    await page.getByText('Historical logs',{exact:true}).last().click();
    await settings.getByLabel('Trace IDs (one per line or comma-separated)',{exact:true}).fill('trace-a,\ntrace-b');
    await settings.getByRole('button',{name:'Import complete traces',exact:true}).click();
    await settings.getByText('source_partial',{exact:true}).waitFor();
    assert.match(await settings.getByTestId('langfuse-receipt').innerText(),/Accepted events: 3/);
    assert.match(await settings.getByTestId('langfuse-receipt').innerText(),/Duplicate events: 4/);
    await settings.getByRole('button',{name:'中文',exact:true}).click();
    await settings.getByRole('button',{name:'导入完整 Trace',exact:true}).waitFor();
    assert.ok(bodies.some(item=>item.operation==='config' && item.method==='GET'));
    return {saved,operations:bodies.map(({operation,method})=>({operation,method}))};
  } finally { await page.unroute(`**${root}**`); }
}
