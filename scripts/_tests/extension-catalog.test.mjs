import assert from 'node:assert/strict';
import crypto from 'node:crypto';
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
    slot_codes: [],
    keywords: [],
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
    const searchIndex = read(repoRoot, `${category}/catalog/v1/search-index.json`);
    assert.equal(index.schema_version, '1flowbase.extension-catalog/v1');
    assert.equal(page.schema_version, index.schema_version);
    assert.equal(index.category, category);
    assert.equal(page.category, category);
    assert.equal(index.first_page.cursor, 'start');
    assert.equal(state.schema_version, '1flowbase.extension-catalog-state/v1');
    assert.equal(searchIndex.schema_version, '1flowbase.extension-catalog-search/v1');
    assert.equal(index.search_index.entry_count, 0);
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
    'slot_codes', 'keywords', 'host_version_requirement', 'source', 'signature', 'checksum',
    'download_locator', 'catalog_page',
  ]);
  assert.equal(entry.organization, 'taichuy');
  assert.equal(entry.artifact, 'nodes');
  assert.equal(entry.catalog_page, 1);
  assert.deepEqual(entry.slot_codes, []);
  assert.deepEqual(entry.keywords, []);
});

test('AC-CAT-SEARCH emits deterministic normalized metadata tied to verified catalog pages', () => {
  const repoRoot = fixtureRepo();
  canonicalEntry(repoRoot, 'host-extensions', 'Zeta', 'Second', {
    name: '  Mixed   CASE Name  ',
    description: 'Search\nDescription',
    slot_codes: ['data_source', 'model_provider', 'data_source'],
    keywords: ['AI Search', 'Provider'],
  });
  updateCategoryCatalog({
    repoRoot,
    category: 'host-extensions',
    pageSize: 1,
    now: '2026-08-01T00:00:00.000Z',
  });

  const index = read(repoRoot, 'host-extensions/catalog/v1/index.json');
  const search = read(repoRoot, 'host-extensions/catalog/v1/search-index.json');
  const searchBytes = fs.readFileSync(path.join(repoRoot, 'host-extensions/catalog/v1/search-index.json'));
  const pageBytes = fs.readFileSync(path.join(repoRoot, 'host-extensions/catalog/v1/pages/1.json'));
  const pageChecksum = `sha256:${crypto.createHash('sha256').update(pageBytes).digest('hex')}`;

  assert.equal(search.source_fingerprint, read(repoRoot, 'host-extensions/_maintenance/catalog-state.json').source_fingerprint);
  assert.equal(search.generated_at, index.generated_at);
  assert.equal(search.entries[0].name, 'mixed case name');
  assert.equal(search.entries[0].description, 'search description');
  assert.equal(search.entries[0].organization, 'zeta');
  assert.deepEqual(search.entries[0].slot_codes, ['data_source', 'model_provider']);
  assert.deepEqual(search.entries[0].keywords, ['ai search', 'provider']);
  assert.equal(search.entries[0].catalog_page.checksum, pageChecksum);
  assert.deepEqual(Object.keys(search.entries[0]), [
    'id', 'name', 'category', 'organization', 'artifact', 'version', 'description',
    'host_version_requirement', 'source', 'signature', 'checksum', 'slot_codes', 'keywords',
    'catalog_page',
  ]);
  assert.equal(index.search_index.schema_version, '1flowbase.extension-catalog-search/v1');
  assert.equal(index.search_index.entry_count, 1);
  assert.equal(index.search_index.checksum, `sha256:${crypto.createHash('sha256').update(searchBytes).digest('hex')}`);
  assert.equal(index.search_index.locator.endsWith('/host-extensions/catalog/v1/search-index.json'), true);

  updateCategoryCatalog({ repoRoot, category: 'host-extensions', pageSize: 1, now: '2099-01-01T00:00:00.000Z' });
  assert.deepEqual(fs.readFileSync(path.join(repoRoot, 'host-extensions/catalog/v1/search-index.json')), searchBytes);
});

test('AC-CAT-RUNTIME derives identity from publisher namespace and slot classification from metadata', () => {
  const repoRoot = fixtureRepo();
  for (const providerCode of ['alpha', 'beta']) {
    const root = path.join(repoRoot, 'runtime-extensions', '@taichuy', providerCode);
    fs.mkdirSync(root, { recursive: true });
    fs.writeFileSync(path.join(root, 'manifest.yaml'), 'manifest fixture\n');
  }
  fs.writeFileSync(path.join(repoRoot, 'official-registry.json'), JSON.stringify({
    version: 1,
    plugins: [
      {
        plugin_id: 'acme.alpha', publisher_namespace: 'acme', provider_code: 'alpha',
        plugin_type: 'model_provider', display_name: 'Alpha', latest_version: '1.0.0',
        minimum_host_version: '0.3.0', protocol: 'alpha', model_discovery_mode: 'hybrid',
        slot_codes: ['data_source'], keywords: ['warehouse'], artifacts: [],
      },
      {
        plugin_id: '1flowbase.beta', publisher_namespace: '1flowbase', provider_code: 'beta',
        plugin_type: 'model_provider', display_name: 'Beta', latest_version: '1.0.0',
        minimum_host_version: '0.3.0', protocol: 'beta', model_discovery_mode: 'hybrid',
        slot_codes: ['model_provider'], keywords: [], artifacts: [],
      },
    ],
  }));

  const entries = discoverCatalogEntries({ repoRoot, category: 'runtime-extensions' });
  assert.deepEqual(entries.map(({ id, organization, slot_codes }) => ({ id, organization, slot_codes })), [
    { id: 'runtime-extensions:1flowbase/beta', organization: '1flowbase', slot_codes: ['model_provider'] },
    { id: 'runtime-extensions:acme/alpha', organization: 'acme', slot_codes: ['data_source'] },
  ]);
  assert.equal(entries[1].source.locator, 'runtime-extensions/@taichuy/alpha/manifest.yaml');
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

test('AC-CAT-5 explicit canonical metadata overrides derived publisher metadata', () => {
  const repoRoot = fixtureRepo();
  const workflowRoot = path.join(repoRoot, 'agent-flow/@taichuy/example');
  fs.mkdirSync(workflowRoot, { recursive: true });
  fs.writeFileSync(path.join(workflowRoot, 'template.json'), JSON.stringify({
    schema_version: '1flowbase.application-template/v1',
    application: { application_type: 'agent_flow', name: 'Derived', description: '' },
  }));
  canonicalEntry(repoRoot, 'agent-flow', 'taichuy', 'example', { name: 'Canonical' });

  const entries = discoverCatalogEntries({ repoRoot, category: 'agent-flow' });
  assert.equal(entries.length, 1);
  assert.equal(entries[0].name, 'Canonical');
  assert.equal(entries[0].source.kind, 'repository');
});

test('AC-CAT-6 Agent Flow discovery projects the latest signed release record', () => {
  const repoRoot = fixtureRepo();
  const catalogRoot = path.join(repoRoot, 'agent-flow/releases/v1');
  fs.mkdirSync(catalogRoot, { recursive: true });
  fs.writeFileSync(path.join(catalogRoot, 'catalog.json'), JSON.stringify({
    schema_version: '1flowbase.agent-flow-catalog/v1',
    templates: [{
      template_id: '019f5443-5b8e-74b2-90e3-c867dbddd37b',
      organization: 'taichuy',
      artifact: 'multimodal-mount-test',
      source_path: 'agent-flow/@taichuy/multimodal-mount-test/template.json',
      versions: [{
        template_id: '019f5443-5b8e-74b2-90e3-c867dbddd37b',
        release_version: 2,
        exported_from_system_version: '0.3.1',
        exported_at: '2026-08-02T00:00:00Z',
        application: { name: 'Multimodal', description: 'Signed template' },
        download_url: 'https://github.com/taichuy/1flowbase-official-plugins/releases/download/template-v2/template-v2.json',
        checksum: `sha256:${'a'.repeat(64)}`,
        algorithm: 'ed25519',
        key_id: 'official-key-2026-04',
        signature: 'fixture-signature',
      }],
    }],
  }));

  const [entry] = discoverCatalogEntries({ repoRoot, category: 'agent-flow' });
  assert.equal(entry.version, '2');
  assert.equal(entry.host_version_requirement, '0.3.1');
  assert.equal(entry.source.kind, 'agent_flow_release');
  assert.equal(entry.download_locator.kind, 'https');
  assert.equal(entry.signature.key_id, 'official-key-2026-04');
});

test('AC-003 MCP discovery projects the latest signed v2 history record', () => {
  const repoRoot = fixtureRepo();
  const sourceRoot = path.join(repoRoot, 'mcp', '@taichuy', 'example');
  fs.mkdirSync(sourceRoot, { recursive: true });
  fs.writeFileSync(path.join(sourceRoot, 'manifest.json'), '{}');
  fs.writeFileSync(path.join(repoRoot, 'mcp', 'catalog.json'), JSON.stringify({
    schema_version: '1flowbase.mcp-catalog/v2',
    bundles: [{
      organization: 'taichuy', bundle_id: 'example', source_path: 'mcp/@taichuy/example/manifest.json',
      versions: [
        { bundle_version: '1.0.0', locale: 'zh_Hans', minimum_host_version: '0.3.0', exported_from_system_version: '0.3.0', release_tag: 'old', download_url: 'https://example.test/old.zip', checksum: `sha256:${'a'.repeat(64)}`, algorithm: 'ed25519', key_id: 'key', signature: 'old-signature' },
        { bundle_version: '1.1.0', locale: 'zh_Hans', minimum_host_version: '0.3.1', exported_from_system_version: '0.3.1', release_tag: 'new', download_url: 'https://example.test/new.zip', checksum: `sha256:${'b'.repeat(64)}`, algorithm: 'ed25519', key_id: 'key', signature: 'new-signature' },
      ],
    }],
  }));
  const [entry] = discoverCatalogEntries({ repoRoot, category: 'mcp' });
  assert.equal(entry.version, '1.1.0');
  assert.equal(entry.checksum, `sha256:${'b'.repeat(64)}`);
  assert.equal(entry.signature.signature, 'new-signature');
  assert.equal(entry.source.release_tag, 'new');
  assert.equal(entry.source.locale, 'zh_Hans');
  assert.equal(entry.source.exported_from_system_version, '0.3.1');
});

test('AC-CAT-1 repository sources and repository downloads resolve through canonical artifact paths', () => {
  const expectedCounts = new Map([
    ['agent-flow', 2],
    ['i18n', 1],
    ['mcp', 1],
    ['runtime-extensions', 6],
  ]);

  for (const [category, expectedCount] of expectedCounts) {
    const entries = discoverCatalogEntries({ repoRoot: repositoryRoot, category });
    assert.equal(entries.length, expectedCount);
    for (const entry of entries) {
      const artifactRoot = `${category}/@${entry.organization}/${entry.artifact}`;
      assert.ok(entry.source.locator.startsWith(artifactRoot), `${entry.id} source must use ${artifactRoot}`);
      if (entry.download_locator.kind === 'repository_file') {
        assert.ok(entry.download_locator.locator.includes(artifactRoot));
      }
    }
  }

  for (const entry of discoverCatalogEntries({ repoRoot: repositoryRoot, category: 'runtime-extensions' })) {
    assert.equal(entry.source.plugin_type, 'model_provider');
    assert.equal(entry.source.provider_code, entry.artifact);
    assert.equal(typeof entry.source.protocol, 'string');
    assert.ok(entry.source.protocol.length > 0);
    assert.equal(typeof entry.source.model_discovery_mode, 'string');
    assert.ok(entry.source.model_discovery_mode.length > 0);
  }

  for (const removedPath of [
    ['agent-flow', 'workflows'].join('/'),
    ['capability-plugins', 'nodes'].join('/'),
    ['mcp', 'taichuy'].join('/'),
    ['runtime-extensions', ['model', 'providers'].join('-')].join('/'),
  ]) {
    assert.equal(fs.existsSync(path.join(repositoryRoot, removedPath)), false);
  }
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
