import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import { updateAgentFlowCatalog } from '../update-agent-flow-catalog.mjs';

test('AC-CAT-5 keeps the existing AgentFlow publisher entry point as an adapter', () => {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'agent-flow-catalog-adapter-'));
  const workflowRoot = path.join(repoRoot, 'agent-flow', '@taichuy', 'example');
  fs.mkdirSync(workflowRoot, { recursive: true });
  fs.writeFileSync(path.join(workflowRoot, 'template.json'), JSON.stringify({
    schema_version: '1flowbase.application-template/v1',
    application: { application_type: 'agent_flow', name: 'Example', description: '' },
  }));

  const result = updateAgentFlowCatalog({ repoRoot, now: '2026-08-01T00:00:00.000Z' });
  const page = JSON.parse(fs.readFileSync(path.join(repoRoot, 'agent-flow/catalog/v1/pages/1.json')));

  assert.equal(result.totalEntries, 1);
  assert.equal(page.entries[0].source.kind, 'agent_flow_template');
  assert.equal(page.entries[0].id, 'agent-flow:taichuy/example');
});
