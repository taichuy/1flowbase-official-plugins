import test from 'node:test';
import assert from 'node:assert/strict';
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
