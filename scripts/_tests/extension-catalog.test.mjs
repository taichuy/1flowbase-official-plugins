import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  CATALOG_CATEGORIES,
  discoverCatalogEntries,
  updateCategoryCatalog,
  updateExtensionCatalog,
} from '../extension-catalog.mjs';

const repositoryRoot = path.resolve(import.meta.dirname, '..', '..');

function fixtureRepo() {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'extension-catalog-'));
  for (const category of CATALOG_CATEGORIES) fs.mkdirSync(path.join(repoRoot, category), { recursive: true });
  return repoRoot;
}

function canonicalEntry(repoRoot, category, organization, artifact, overrides = {}) {
  const root = path.join(repoRoot, category, `@${organization}`, artifact);
  fs.mkdirSync(root, { recursive: true });
  fs.writeFileSync(path.join(root, 'catalog-entry.json'), `${JSON.stringify({
    name: artifact,
    version: '1.0.0',
    description: `${artifact} description`,
    host_version_requirement: '>=0.3.0',
    source: { kind: 'repository', locator: `${category}/@${organization}/${artifact}` },
    signature: null,
    checksum: `sha256:${'a'.repeat(64)}`,
    download_locator: { kind: 'repository_archive', locator: `https://example.test/${artifact}.zip` },
    ...overrides,
  }, null, 2)}\n`);
}

function read(repoRoot, relativePath) {
  return JSON.parse(fs.readFileSync(path.join(repoRoot, relativePath), 'utf8'));
}

test('AC-CAT-1 emits the same v1 index/page/state contract for all six categories', () => {
  const repoRoot = fixtureRepo();
  updateExtensionCatalog({ repoRoot, now: '2026-08-01T00:00:00.000Z' });

  for (const category of CATALOG_CATEGORIES) {
    const index = read(repoRoot, `${category}/catalog/v1/index.json`);
    const page = read(repoRoot, `${category}/catalog/v1/pages/1.json`);
    const state = read(repoRoot, `${category}/_maintenance/catalog-state.json`);
    assert.equal(index.schema_version, '1flowbase.extension-catalog/v1');
    assert.equal(page.schema_version, index.schema_version);
    assert.equal(index.category, category);
    assert.equal(page.category, category);
    assert.equal(index.first_page.cursor, 'start');
    assert.equal(state.schema_version, '1flowbase.extension-catalog-state/v1');
    assert.equal(Object.hasOwn(index, '_maintenance'), false);
    assert.equal(Object.hasOwn(page, '_maintenance'), false);
  }
});

test('AC-CAT-2 uses stable id order, repeatable cursors/pages, and incremental state positions', () => {
  const repoRoot = fixtureRepo();
  canonicalEntry(repoRoot, 'host-extensions', 'zeta', 'second');
  canonicalEntry(repoRoot, 'host-extensions', 'alpha', 'first');

  const first = updateCategoryCatalog({
    repoRoot,
    category: 'host-extensions',
    pageSize: 1,
    now: '2026-08-01T00:00:00.000Z',
  });
  const firstIndexBytes = fs.readFileSync(path.join(repoRoot, 'host-extensions/catalog/v1/index.json'), 'utf8');
  const pageOne = read(repoRoot, 'host-extensions/catalog/v1/pages/1.json');
  const pageTwo = read(repoRoot, 'host-extensions/catalog/v1/pages/2.json');
  const state = read(repoRoot, 'host-extensions/_maintenance/catalog-state.json');
  const second = updateCategoryCatalog({
    repoRoot,
    category: 'host-extensions',
    pageSize: 1,
    now: '2099-01-01T00:00:00.000Z',
  });

  assert.equal(first.pageCount, 2);
  assert.equal(pageOne.entries[0].id, 'host-extensions:alpha/first');
  assert.equal(pageTwo.entries[0].id, 'host-extensions:zeta/second');
  assert.equal(pageOne.next_cursor, pageTwo.cursor);
  assert.equal(state.entries['host-extensions:zeta/second'].page, 2);
  assert.match(state.entries['host-extensions:zeta/second'].source_fingerprint, /^sha256:[a-f0-9]{64}$/);
  assert.equal(second.changed, false);
  assert.equal(fs.readFileSync(path.join(repoRoot, 'host-extensions/catalog/v1/index.json'), 'utf8'), firstIndexBytes);
});

test('AC-CAT-3 exposes the Gateway field set with domain names and catalog page', () => {
  const repoRoot = fixtureRepo();
  canonicalEntry(repoRoot, 'capability-plugins', 'taichuy', 'nodes');
  updateCategoryCatalog({ repoRoot, category: 'capability-plugins', now: '2026-08-01T00:00:00.000Z' });
  const entry = read(repoRoot, 'capability-plugins/catalog/v1/pages/1.json').entries[0];

  assert.deepEqual(Object.keys(entry), [
    'id', 'name', 'category', 'organization', 'artifact', 'version', 'description',
    'host_version_requirement', 'source', 'signature', 'checksum', 'download_locator', 'catalog_page',
  ]);
  assert.equal(entry.organization, 'taichuy');
  assert.equal(entry.artifact, 'nodes');
  assert.equal(entry.catalog_page, 1);
});

test('AC-CAT-4 check mode reports catalog drift without rewriting generated files', () => {
  const repoRoot = fixtureRepo();
  canonicalEntry(repoRoot, 'capability-plugins', 'taichuy', 'nodes');
  updateCategoryCatalog({ repoRoot, category: 'capability-plugins', now: '2026-08-01T00:00:00.000Z' });
  const indexPath = path.join(repoRoot, 'capability-plugins/catalog/v1/index.json');
  fs.writeFileSync(indexPath, '{}\n');

  assert.throws(
    () => updateCategoryCatalog({ repoRoot, category: 'capability-plugins', check: true }),
    /catalog drift/,
  );
  assert.equal(fs.readFileSync(indexPath, 'utf8'), '{}\n');
});

test('AC-CAT-5 canonical source layout overrides a legacy publisher entry with the same identity', () => {
  const repoRoot = fixtureRepo();
  const workflowRoot = path.join(repoRoot, 'agent-flow/workflows/example');
  fs.mkdirSync(workflowRoot, { recursive: true });
  fs.writeFileSync(path.join(workflowRoot, 'template.json'), JSON.stringify({
    schema_version: '1flowbase.application-template/v1',
    application: { application_type: 'agent_flow', name: 'Legacy', description: '' },
  }));
  canonicalEntry(repoRoot, 'agent-flow', 'taichuy', 'example', { name: 'Canonical' });

  const entries = discoverCatalogEntries({ repoRoot, category: 'agent-flow' });
  assert.equal(entries.length, 1);
  assert.equal(entries[0].name, 'Canonical');
  assert.equal(entries[0].source.kind, 'repository');
});

test('AC-CAT-4 and AC-CAT-5 workflows detect drift, rebuild, and invoke publisher adapters', () => {
  const catalogWorkflow = fs.readFileSync(path.join(repositoryRoot, '.github/workflows/extension-catalog.yml'), 'utf8');
  const agentFlowWorkflow = fs.readFileSync(path.join(repositoryRoot, '.github/workflows/agent-flow-catalog.yml'), 'utf8');
  const mcpWorkflow = fs.readFileSync(path.join(repositoryRoot, '.github/workflows/mcp-bundle-release.yml'), 'utf8');
  const i18nWorkflow = fs.readFileSync(path.join(repositoryRoot, '.github/workflows/i18n-catalog-release.yml'), 'utf8');
  const providerWorkflow = fs.readFileSync(path.join(repositoryRoot, '.github/workflows/provider-release.yml'), 'utf8');

  assert.match(catalogWorkflow, /node scripts\/update-extension-catalog\.mjs/);
  assert.match(catalogWorkflow, /git diff --exit-code/);
  assert.match(catalogWorkflow, /git commit -m "chore: rebuild extension catalogs"/);
  assert.match(agentFlowWorkflow, /--category agent-flow/);
  assert.match(mcpWorkflow, /--category mcp/);
  assert.match(i18nWorkflow, /--category i18n --check/);
  assert.match(providerWorkflow, /--category runtime-extensions/);
});
