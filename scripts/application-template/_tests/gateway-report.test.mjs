import test from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { readPackage } from '../archive.mjs';

const directory = fileURLToPath(new URL('../../../applications-demo/@taichuy/gateway-demo/', import.meta.url));

test('Gateway report release includes the shared snapshot contract for all three retained blocks', async () => {
  const template = await readPackage(directory);
  assert.equal(template.release.template_id, '@taichuy/gateway-demo');
  const page = template.pages.find(p => p.id === '01a073f3-03e9-7030-86a4-371f80ebf522');
  const blocks = page.tabs.flatMap(tab => tab.blocks);
  const owner = blocks.find(b => b.description === 'model-usage-report:overview');
  const consumers = ['trend', 'users'].map(name => blocks.find(b => b.description === `model-usage-report:${name}`));
  assert.ok(owner);
  assert.ok(consumers.every(Boolean));
  assert.equal(blocks.length, 3);

  for (const block of [owner, ...consumers]) {
    assert.equal(block.input_mapping.timeRange, 'usage.timeRange');
    assert.equal(block.input_mapping.reportState, 'usage.reportState');
    const inputs = block.runtime_descriptor.ports.inputs;
    assert.equal(inputs.filter(p => p.name === 'reportState').length, 1);
    const schema = inputs.find(p => p.name === 'reportState').schema;
    assert.equal(schema.type, 'object');
    assert.deepEqual(new Set(schema.required), new Set(['report', 'busy', 'error']));
    assert.equal(schema.properties.busy.type, 'boolean');
    assert.equal(schema.properties.error.type, 'boolean');
  }

  assert.equal(owner.output_mapping.reportState, 'usage.reportState');
  const outputs = owner.runtime_descriptor.ports.outputs;
  assert.equal(outputs.filter(p => p.name === 'reportState').length, 1);
  const outputSchema = outputs.find(p => p.name === 'reportState').schema;
  for (const block of [owner, ...consumers]) {
    assert.deepEqual(block.runtime_descriptor.ports.inputs.find(p => p.name === 'reportState').schema, outputSchema);
  }
  for (const block of consumers) {
    assert.ok(!Object.hasOwn(block.output_mapping, 'reportState'));
    assert.ok(!block.runtime_descriptor.ports.outputs.some(p => p.name === 'reportState'));
  }
  assert.ok(template.release.release_version >= 3);
});
