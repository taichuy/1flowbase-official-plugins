import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readPackage } from '../archive.mjs';

const directory = process.env.GATEWAY_DEMO_DIRECTORY || fileURLToPath(new URL('../../../applications-demo/@taichuy/gateway-demo/', import.meta.url));

test('Gateway workflow drafts and publications preserve Start inputs without duplicate outputs', async () => {
  const template = await readPackage(directory);
  for (const application of template.applications) {
    for (const document of [application.flow_document, application.published?.flow_document].filter(Boolean)) {
      for (const start of document.graph.nodes.filter(node => ['start', 'workflow_start'].includes(node.type))) {
        assert.deepEqual(start.outputs, [], `${application.name}: ${start.id}`);
      }
    }
  }
  const summary = template.applications.find(application => application.id === '01a08e7b-4bd9-71d0-9a44-a43f845f68cd');
  assert.ok(summary?.published);
  for (const document of [summary.flow_document, summary.published.flow_document]) {
    const start = document.graph.nodes.find(node => node.type === 'workflow_start');
    assert.deepEqual(start.config.input_fields.map(field => [field.key, field.defaultValue]), [['sort_by', 'created_at'], ['sort_order', 'desc']]);
    const sort = document.graph.nodes.find(node => node.id === 'node-output');
    for (const key of ['sort_by', 'sort_order']) assert.deepEqual(sort.bindings[key].value, [start.id, key]);
  }
  const instructions = template.mcp_bundle.tools.find(tool => tool.tool_id === 'application_create_workflow').full_description;
  assert.ok(instructions.includes('Workflow Start的outputs必须为空数组[]'));
  assert.ok(!instructions.includes('不要照搬Agent Flow start的空outputs规则'));
});

test('Gateway bootstrap retains page workflows without publishing an LLM dependency', async () => {
  const template = await readPackage(directory);
  const workflows = new Map(template.applications.map(application => [application.name, application]));
  assert.deepEqual([...workflows.keys()].sort(), ['应用 API Key 汇总', '模型使用统计', '每日日报'].sort());
  for (const app of template.applications) {
    assert.ok(!app.published?.flow_document.graph.nodes.some(node => node.type === 'llm'), app.name);
  }
  for (const name of ['应用 API Key 汇总', '模型使用统计']) {
    assert.ok(workflows.get(name).published, name);
  }
  const dailyReport = workflows.get('每日日报');
  assert.equal(dailyReport.id, '01a0d95f-0701-7c40-8078-164e95a5663e');
  assert.ok(dailyReport.flow_document.graph.nodes.some(node => node.type === 'llm'));
  assert.equal(dailyReport.schedule.enabled, false);
  assert.ok(!dailyReport.published);
  const config = JSON.parse(await readFile(join(directory, 'export.config.json'), 'utf8'));
  assert.ok(!config.selection.application_ids.includes('01a08968-5ea8-7e63-b70e-3fd5bd9522b4'));
});

test('Gateway conversation retains its published AgentFlow in a separate catalog template', async () => {
  const optional = await readPackage(join(directory, '..', 'gateway-demo-conversation'));
  assert.equal(optional.release.template_id, '@taichuy/gateway-demo-conversation');
  assert.deepEqual(optional.pages, []);
  assert.equal(optional.applications.length, 1);
  const [app] = optional.applications;
  assert.equal(app.id, '01a08968-5ea8-7e63-b70e-3fd5bd9522b4');
  assert.equal(app.name, '1flowbase');
  assert.equal(app.application_type, 'AgentFlow');
  assert.ok(app.published.flow_document.graph.nodes.some(node => node.config.model_provider?.provider_code === 'anthropic'));
});
