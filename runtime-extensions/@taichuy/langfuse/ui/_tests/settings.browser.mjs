import assert from 'node:assert/strict';

/** Central QA injects an authenticated page and owns exact user-locale restoration.
 * No login, server lifecycle, or real Langfuse requests belong to this consumer fixture.
 * setHostLocale may use the central runner's globalLanguageChromeMenu helper.
 */
export async function exerciseLangfuseSettings(page, {setHostLocale} = {}) {
  const root = '/api/console/managed-services/langfuse/';
  const ids = ['aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa','bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb'];
  const instances = new Map([['default',{instance_id:'default',name:'Legacy project',base_url:'https://legacy.example/',credential_configured:true}]]);
  const credentials = new Map([['default',{public_key:'legacy-public',secret_key:'legacy-secret'}]]);
  const bodies = [];
  let nextId = 0;
  let explicitImports = 0;
  const globalLanguageChromeMenu = page.locator('.app-shell-language-menu');
  const originalLocale = (await globalLanguageChromeMenu.locator('.app-shell-language-label').innerText()) === '中' ? 'zh_Hans' : 'en_US';
  const switchLocale = setHostLocale ?? (async locale => {
    await globalLanguageChromeMenu.getByRole('menuitem').first().hover();
    await page.getByRole('menuitem',{name:locale === 'en_US' ? 'English' : /^(Chinese|简体中文)$/,exact:true}).click();
  });
  await page.route(`**${root}**`, async route => {
    const request = route.request();
    const url = new URL(request.url());
    const operation = url.pathname.slice(root.length);
    const method = request.method();
    const body = method === 'GET' || method === 'DELETE' ? undefined : request.postDataJSON();
    bodies.push({operation,method,body});
    let result;
    if (operation === 'instances' && method === 'GET') {
      assert.equal(url.searchParams.get('page_size'),'50');
      const pageNumber = Number(url.searchParams.get('page'));
      assert.ok(pageNumber >= 1);
      const all = [...instances.values()];
      result = {success:true,items:all.slice((pageNumber - 1) * 50,pageNumber * 50),page:pageNumber,page_size:50,total:all.length};
    } else if (operation === 'instances' && method === 'POST') {
      const instance_id = ids[nextId++];
      assert.ok(instance_id,'Only the two authorized fixture instances may be created');
      assert.equal(body.public_key,`fixture-public-${nextId}`);
      assert.equal(body.secret_key,`fixture-secret-${nextId}`);
      const instance = {instance_id,name:body.name,base_url:body.base_url,credential_configured:true};
      instances.set(instance_id,instance);
      credentials.set(instance_id,{public_key:body.public_key,secret_key:body.secret_key});
      result = {success:true,instance};
    } else if (/^instances\/[^/]+\/test$/.test(operation)) {
      const id = operation.split('/')[1];
      assert.equal(method,'POST');
      assert.ok(credentials.has(id));
      result = {success:true,connected:true};
    } else if (operation.startsWith('instances/')) {
      const id = operation.split('/')[1];
      assert.ok(instances.has(id));
      if (method === 'PUT') {
        assert.equal(id,ids[0]);
        assert.deepEqual(body,{name:'Renamed project',base_url:'https://langfuse.example/prefix/'});
        const instance = {...instances.get(id),name:body.name,base_url:body.base_url};
        instances.set(id,instance);
        assert.deepEqual(credentials.get(id),{public_key:'fixture-public-1',secret_key:'fixture-secret-1'});
        result = {success:true,instance};
      } else {
        assert.equal(method,'DELETE');
        assert.equal(id,ids[0]);
        instances.delete(id); credentials.delete(id);
        result = {success:true};
      }
    } else if (operation === 'applications') {
      result = {success:true,items:[{application_id:'native',name:'Native source',application_type:'agent_flow'},{application_id:'target',name:'Historical logs',application_type:'agent_logs'}]};
    } else if (operation === 'export') {
      assert.deepEqual(body,{instance_id:ids[0],application_id:'native',started_at_from:'2026-10-01T00:00:00Z',started_at_to:'2026-11-01T00:00:00Z'});
      result = {success:true,receipt:{exported_records:2,exported_spans:9,trace_ids:['trace-a','trace-b']}};
    } else if (operation === 'import') {
      assert.equal(body.instance_id,ids[1]);
      assert.equal(body.application_id,'target');
      if (body.trace_ids) {
        assert.deepEqual(body.trace_ids,['trace-a','trace-b']);
        explicitImports++;
        result = explicitImports === 1
          ? {success:false,error:{code:'agent_logs_partial_import',status:409,details:{progress:{imported_traces:1,accepted_events:3,duplicate_events:4,partial_committed_records:1,record_ids:['record-a']},cause_details:{accepted_events:1,committed_records:1,record_ids:['record-b']}}}}
          : {success:true,receipt:{imported_traces:2,accepted_events:0,duplicate_events:7,record_ids:['record-a','record-b']}};
      } else {
        assert.deepEqual(body,{instance_id:ids[1],application_id:'target',started_at_from:'2026-10-01T00:00:00Z',started_at_to:'2026-11-01T00:00:00Z'});
        result = {success:true,receipt:{imported_traces:2,accepted_events:0,duplicate_events:7,record_ids:['record-a','record-b']}};
      }
    } else throw new Error(`Unexpected managed operation: ${method} ${operation}`);
    await route.fulfill({contentType:'application/json',body:JSON.stringify({data:result})});
  });
  const actions = id => page.getByTestId(`langfuse-instance-actions-${id}`);
  const dialog = name => page.getByRole('dialog',{name,exact:true});
  const dates = async form => {
    await form.getByLabel('Started at or after (RFC3339)',{exact:true}).fill('2026-10-01T00:00:00Z');
    await form.getByLabel('Started before (RFC3339)',{exact:true}).fill('2026-11-01T00:00:00Z');
  };
  const dismiss = async form => {
    await form.getByRole('button',{name:'Cancel',exact:true}).click();
    await form.waitFor({state:'hidden'});
  };
  try {
    await switchLocale('en_US');
    await page.goto(new URL('/settings/langfuse',page.url()).toString());
    const settings = page.getByTestId('langfuse-settings');
    await settings.waitFor();
    await actions('default').waitFor();
    assert.equal(await settings.getByRole('button',{name:/^(English|中文)$/}).count(),0);
    assert.ok(await settings.locator('.data-table').count(),'Use the actual public shared table');
    for (let index = 0; index < ids.length; index++) {
      await settings.getByRole('button',{name:'Add instance',exact:true}).click();
      const editor = dialog('Instance');
      await editor.getByLabel('Name',{exact:true}).fill(`Project ${index + 1}`);
      await editor.getByLabel('Service URL',{exact:true}).fill('https://langfuse.example/prefix/');
      await editor.getByLabel('Public key',{exact:true}).fill(`fixture-public-${index + 1}`);
      await editor.getByLabel('Secret key',{exact:true}).fill(`fixture-secret-${index + 1}`);
      await editor.getByRole('button',{name:'Save',exact:true}).click();
      await editor.waitFor({state:'hidden'});
      await actions(ids[index]).waitFor();
      await actions(ids[index]).getByRole('button',{name:'Edit',exact:true}).click();
      assert.equal(await editor.getByLabel('Public key',{exact:true}).inputValue(),'');
      assert.equal(await editor.getByLabel('Secret key',{exact:true}).inputValue(),'');
      await dismiss(editor);
    }
    await actions(ids[0]).getByRole('button',{name:'Edit',exact:true}).click();
    const editor = dialog('Instance');
    await editor.getByLabel('Name',{exact:true}).fill('Renamed project');
    await editor.getByRole('button',{name:'Save',exact:true}).click();
    await editor.waitFor({state:'hidden'});
    await settings.getByRole('button',{name:'Reload',exact:true}).click();
    await settings.getByText('Saved instances reloaded',{exact:true}).waitFor();
    await actions(ids[0]).getByRole('button',{name:'Edit',exact:true}).click();
    assert.equal(await editor.getByLabel('Name',{exact:true}).inputValue(),'Renamed project');
    assert.equal(await editor.getByLabel('Public key',{exact:true}).inputValue(),'');
    assert.equal(await editor.getByLabel('Secret key',{exact:true}).inputValue(),'');
    await dismiss(editor);
    for (const id of ids) {
      await actions(id).getByRole('button',{name:'Test connection',exact:true}).click();
      await settings.getByText('Connected',{exact:true}).waitFor();
    }
    await actions(ids[0]).getByRole('button',{name:'Export',exact:true}).click();
    const source = dialog('Export application logs');
    await source.getByLabel('Source application',{exact:true}).click();
    await page.getByText('Native source (agent_flow)',{exact:true}).last().click();
    await dates(source);
    await source.getByRole('button',{name:'Export complete records',exact:true}).click();
    await source.getByTestId('langfuse-receipt').waitFor();
    assert.match(await source.getByTestId('langfuse-receipt').innerText(),/Exported spans: 9/);
    await dismiss(source);
    await actions(ids[1]).getByRole('button',{name:'Import',exact:true}).click();
    const target = dialog('Import into AgentLogs');
    await target.getByLabel('Target AgentLogs application',{exact:true}).click();
    await page.getByText('Historical logs',{exact:true}).last().click();
    await target.getByLabel('Trace IDs (one per line or comma-separated)',{exact:true}).fill('trace-a,\ntrace-b');
    await target.getByRole('button',{name:'Import complete traces',exact:true}).click();
    await target.getByText('agent_logs_partial_import',{exact:true}).waitFor();
    const receipt = target.getByTestId('langfuse-receipt');
    assert.match(await receipt.innerText(),/Accepted events: 3/);
    assert.match(await receipt.innerText(),/Duplicate events: 4/);
    assert.match(await receipt.innerText(),/Records committed in the partial step: 1/);
    assert.match(await receipt.innerText(),/record-a/);
    assert.match(await receipt.innerText(),/record-b/);
    assert.equal(await receipt.locator('pre').count(),0,'Receipts must not render raw debug JSON');
    await target.getByRole('button',{name:'Import complete traces',exact:true}).click();
    await target.getByText('Receipt',{exact:true}).waitFor();
    assert.match(await receipt.innerText(),/Duplicate events: 7/);
    await target.getByRole('switch',{name:'Select traces by date',exact:true}).click();
    await dates(target);
    await Promise.all([
      page.waitForResponse(response => response.url().endsWith('/import') && response.request().postDataJSON()?.started_at_from !== undefined),
      target.getByRole('button',{name:'Import complete traces',exact:true}).click()
    ]);
    await target.getByRole('button',{name:'Import complete traces',exact:true}).waitFor({state:'visible'});
    await dismiss(target);
    await actions(ids[0]).getByRole('button',{name:'Delete',exact:true}).click();
    await page.getByRole('tooltip').getByRole('button',{name:'Delete',exact:true}).click();
    await settings.getByText('Instance deleted',{exact:true}).waitFor();
    assert.equal(await actions(ids[0]).count(),0);
    await actions(ids[1]).waitFor();
    await actions('default').waitFor();
    assert.deepEqual(credentials.get(ids[1]),{public_key:'fixture-public-2',secret_key:'fixture-secret-2'});
    assert.deepEqual(credentials.get('default'),{public_key:'legacy-public',secret_key:'legacy-secret'});
    await switchLocale('zh_Hans');
    await settings.getByRole('button',{name:'添加实例',exact:true}).waitFor();
    await actions(ids[1]).getByRole('button',{name:'导入',exact:true}).click();
    await dialog('导入到 AgentLogs').getByRole('button',{name:'导入完整 Trace',exact:true}).waitFor();
    await switchLocale('en_US');
    await dialog('Import into AgentLogs').getByRole('button',{name:'Import complete traces',exact:true}).waitFor();
    await dismiss(dialog('Import into AgentLogs'));
    assert.ok(bodies.some(item => item.operation === `instances/${ids[0]}` && item.method === 'PUT'));
    return {saved:nextId === 2,originalLocale,operations:bodies.map(({operation,method})=>({operation,method}))};
  } finally {
    await page.unroute(`**${root}**`);
    await switchLocale(originalLocale);
  }
}
