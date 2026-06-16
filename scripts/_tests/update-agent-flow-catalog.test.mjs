import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { updateAgentFlowCatalog } from '../update-agent-flow-catalog.mjs';

function makeTempRepo() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'agent-flow-catalog-'));
}

function writeTemplate(repoRoot, workflowId, application = {}) {
  const workflowRoot = path.join(repoRoot, 'agent-flow', 'workflows', workflowId);
  fs.mkdirSync(workflowRoot, { recursive: true });
  fs.writeFileSync(
    path.join(workflowRoot, 'template.json'),
    JSON.stringify(
      {
        schema_version: '1flowbase.application-template/v1',
        application: {
          application_type: 'agent_flow',
          name: application.name || workflowId,
          description: application.description || '',
          icon: application.icon ?? null,
          icon_type: application.icon_type ?? null,
          icon_background: application.icon_background ?? null,
        },
        flow_document: {
          schemaVersion: '1flowbase.flow/v1',
          graph: { nodes: [], edges: [] },
        },
        dependencies: [],
      },
      null,
      2
    )
  );
}

function readJson(repoRoot, relativePath) {
  return JSON.parse(fs.readFileSync(path.join(repoRoot, relativePath), 'utf8'));
}

test('updateAgentFlowCatalog exposes only template identity and application metadata', () => {
  const repoRoot = makeTempRepo();
  writeTemplate(repoRoot, 'multimodal-mount-test', {
    name: 'Multimodal Mount Test',
    icon: 'RobotOutlined',
    icon_type: 'iconfont',
    icon_background: '#E6F7F2',
  });

  const result = updateAgentFlowCatalog({
    repoRoot,
    now: '2026-06-16T00:00:00.000Z',
    rawBaseUrl: 'https://example.test/repo',
  });

  assert.equal(result.changed, true);
  assert.equal(result.totalEntries, 1);

  const page = readJson(repoRoot, 'agent-flow/catalog/v1/pages/1.json');
  const entry = page.entries[0];

  assert.equal(entry.workflow_id, 'multimodal-mount-test');
  assert.equal(entry.schema_version, '1flowbase.application-template/v1');
  assert.deepEqual(entry.application, {
    application_type: 'agent_flow',
    name: 'Multimodal Mount Test',
    description: '',
    icon: 'RobotOutlined',
    icon_type: 'iconfont',
    icon_background: '#E6F7F2',
  });
  assert.equal(
    entry.template_url,
    'https://example.test/repo/agent-flow/workflows/multimodal-mount-test/template.json'
  );
  assert.match(entry.template_sha256, /^sha256:[a-f0-9]{64}$/);
  assert.equal(entry.updated_at, '2026-06-16T00:00:00.000Z');
  assert.equal(Object.hasOwn(entry, 'tags'), false);
  assert.equal(Object.hasOwn(entry, 'author'), false);
  assert.equal(Object.hasOwn(entry, 'status'), false);
  assert.equal(Object.hasOwn(entry, 'dependency_summary'), false);
});

test('updateAgentFlowCatalog keeps generated files unchanged when hashes are unchanged', () => {
  const repoRoot = makeTempRepo();
  writeTemplate(repoRoot, 'stable-flow', { name: 'Stable Flow' });

  updateAgentFlowCatalog({
    repoRoot,
    now: '2026-06-16T00:00:00.000Z',
    rawBaseUrl: 'https://example.test/repo',
  });
  const firstIndex = readJson(repoRoot, 'agent-flow/catalog/v1/index.json');
  const firstPage = readJson(repoRoot, 'agent-flow/catalog/v1/pages/1.json');

  const second = updateAgentFlowCatalog({
    repoRoot,
    now: '2026-06-17T00:00:00.000Z',
    rawBaseUrl: 'https://example.test/repo',
  });
  const secondIndex = readJson(repoRoot, 'agent-flow/catalog/v1/index.json');
  const secondPage = readJson(repoRoot, 'agent-flow/catalog/v1/pages/1.json');

  assert.equal(second.changed, false);
  assert.equal(secondIndex.generated_at, firstIndex.generated_at);
  assert.equal(secondPage.entries[0].updated_at, firstPage.entries[0].updated_at);
});

test('updateAgentFlowCatalog paginates generated entries at one hundred workflows', () => {
  const repoRoot = makeTempRepo();
  for (let index = 0; index < 101; index += 1) {
    writeTemplate(repoRoot, `flow-${String(index).padStart(3, '0')}`);
  }

  updateAgentFlowCatalog({
    repoRoot,
    now: '2026-06-16T00:00:00.000Z',
    rawBaseUrl: 'https://example.test/repo',
  });

  const index = readJson(repoRoot, 'agent-flow/catalog/v1/index.json');
  const firstPage = readJson(repoRoot, 'agent-flow/catalog/v1/pages/1.json');
  const secondPage = readJson(repoRoot, 'agent-flow/catalog/v1/pages/2.json');

  assert.equal(index.total_entries, 101);
  assert.equal(index.pages.length, 2);
  assert.equal(firstPage.entries.length, 100);
  assert.equal(secondPage.entries.length, 1);
  assert.equal(
    firstPage.next_page_url,
    'https://example.test/repo/agent-flow/catalog/v1/pages/2.json'
  );
  assert.equal(secondPage.next_page_url, null);
});
