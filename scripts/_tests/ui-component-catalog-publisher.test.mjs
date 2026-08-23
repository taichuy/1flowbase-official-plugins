import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  buildUiComponentCatalog,
  discoverUiComponentRecords,
  stableJson,
  updateUiComponentCatalog,
  verifyUiComponentSeed,
} from '../ui-component-catalog-publisher.mjs';

function writeJson(repoRoot, relativePath, value) {
  const filePath = path.join(repoRoot, relativePath);
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, stableJson(value));
}

function sourceRecord(overrides = {}) {
  return {
    schema_version: '1flowbase.ui-component-source/v1',
    component_code: 'taichuy.ant-design-x.bubble',
    name: 'Bubble',
    description: 'Render a maintained conversational message sample.',
    import_code: "import { Bubble } from '@ant-design/x';",
    source_code: '<Bubble content="Hello" />',
    origin: 'official',
    source: 'taichuy',
    group: 'ant-design-x',
    upstream: { identity: '@ant-design/x', version: '2.9.0' },
    version: '1.0.0',
    keywords: ['message', 'chat'],
    updated_at: '2026-08-23T00:00:00.000Z',
    ...overrides,
  };
}

function makeFixture(records = [sourceRecord()]) {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'official-ui-component-catalog-'));
  writeJson(repoRoot, 'ui_components/catalog-source.json', {
    schema_version: '1flowbase.ui-component-catalog-source/v1',
    catalog_version: '1.0.0',
    generated_at: '2026-08-23T00:00:00.000Z',
    page_size: 1,
  });
  for (const [index, record] of records.entries()) {
    writeJson(
      repoRoot,
      `ui_components/@${record.source}/${record.group}/component-${index + 1}.json`,
      record,
    );
  }
  return repoRoot;
}

test('D1-AC-01/D1-AC-04 builds deterministic pages, search, download, update, and maintained inventory', () => {
  const sender = sourceRecord({
    component_code: 'taichuy.ant-design-x.sender',
    name: 'Sender',
    description: 'Render a maintained prompt sender sample.',
    import_code: "import { Sender } from '@ant-design/x';",
    source_code: '<Sender placeholder="Ask anything" />',
    keywords: ['input', 'prompt'],
  });
  const repoRoot = makeFixture([sender, sourceRecord()]);
  writeJson(repoRoot, 'runtime-extensions/@taichuy/not-a-source/component.json', sourceRecord({
    component_code: 'must.not.be.discovered',
  }));

  const records = discoverUiComponentRecords(repoRoot);
  assert.deepEqual(records.map((record) => record.component_code), [
    'taichuy.ant-design-x.bubble',
    'taichuy.ant-design-x.sender',
  ]);

  const first = buildUiComponentCatalog({
    repoRoot,
    rawBaseUrl: 'https://catalog.invalid/main',
    now: '2030-01-01T00:00:00.000Z',
  });
  assert.equal(first.index.total_components, 2);
  assert.equal(first.index.pages.length, 2);
  assert.equal(first.pages[0].document.cursor, 'start');
  assert.equal(first.pages[0].document.next_cursor, Buffer.from(
    'after:taichuy.ant-design-x.bubble',
    'utf8',
  ).toString('base64url'));
  assert.equal(first.search.entries[0].name, 'bubble');
  assert.equal(first.index.search_index.checksum, first.searchChecksum);
  assert.equal(first.index.download.checksum, first.seedChecksum);
  assert.equal(first.index.download.release_tag, 'ui-component-catalog-v1.0.0');
  assert.deepEqual(first.index.update, {
    strategy: 'authoritative_source_group_replace',
    identity_field: 'component_code',
    source_field: 'source',
    group_field: 'group',
    version_field: 'version',
  });
  assert.equal(verifyUiComponentSeed(first.seed), true);

  updateUiComponentCatalog({ repoRoot, rawBaseUrl: 'https://catalog.invalid/main' });
  const second = buildUiComponentCatalog({
    repoRoot,
    rawBaseUrl: 'https://catalog.invalid/main',
    now: '2040-01-01T00:00:00.000Z',
  });
  assert.equal(second.index.generated_at, '2026-08-23T00:00:00.000Z');
  assert.equal(stableJson(second.seed), stableJson(first.seed));
  assert.equal(updateUiComponentCatalog({
    repoRoot,
    rawBaseUrl: 'https://catalog.invalid/main',
    check: true,
  }).changed, false);
});

test('D1-AC-03 rejects duplicate component identities', () => {
  const repoRoot = makeFixture();
  writeJson(repoRoot, 'ui_components/@taichuy/other-group/duplicate.json', sourceRecord({
    group: 'other-group',
  }));
  assert.throws(() => discoverUiComponentRecords(repoRoot), /duplicate component_code/);
});

test('D1-AC-03 rejects missing raw import and source code without evaluating either', async (t) => {
  for (const field of ['import_code', 'source_code']) {
    await t.test(field, () => {
      const record = sourceRecord();
      delete record[field];
      const repoRoot = makeFixture([record]);
      assert.throws(() => discoverUiComponentRecords(repoRoot), /fields must be exactly/);
    });
  }

  const deliberatelyUnresolvable = sourceRecord({
    import_code: "import Widget from '@package/that-does-not-exist';",
    source_code: '<Widget definitelyNotARealProp={() => ???} />',
  });
  assert.equal(discoverUiComponentRecords(makeFixture([deliberatelyUnresolvable])).length, 1);
});

test('D1-AC-03 rejects unsupported or invalid source fields', async (t) => {
  await t.test('extra field', () => {
    const repoRoot = makeFixture([sourceRecord({ executable: true })]);
    assert.throws(() => discoverUiComponentRecords(repoRoot), /fields must be exactly/);
  });
  await t.test('invalid origin', () => {
    const repoRoot = makeFixture([sourceRecord({ origin: 'custom' })]);
    assert.throws(() => discoverUiComponentRecords(repoRoot), /origin must be official/);
  });
  await t.test('source and group must match their canonical path', () => {
    const repoRoot = makeFixture();
    const record = sourceRecord({ source: 'another-publisher' });
    writeJson(repoRoot, 'ui_components/@taichuy/ant-design-x/mismatch.json', record);
    assert.throws(() => discoverUiComponentRecords(repoRoot), /source must match canonical path/);
  });
});

test('D1-AC-03 detects a tampered Seed', () => {
  const catalog = buildUiComponentCatalog({ repoRoot: makeFixture() });
  catalog.seed.components[0].source_code = '<Tampered />';
  assert.throws(() => verifyUiComponentSeed(catalog.seed), /tampered Seed/);
});

test('D1-AC-03 --check rejects stale page and search outputs', async (t) => {
  await t.test('stale page', () => {
    const repoRoot = makeFixture();
    updateUiComponentCatalog({ repoRoot });
    writeJson(repoRoot, 'ui_components/catalog/v1/pages/99.json', { stale: true });
    assert.throws(
      () => updateUiComponentCatalog({ repoRoot, check: true }),
      /catalog drift: .*pages\/99\.json/,
    );
  });
  await t.test('stale search index', () => {
    const repoRoot = makeFixture();
    updateUiComponentCatalog({ repoRoot });
    writeJson(repoRoot, 'ui_components/catalog/v1/search-index.json', { stale: true });
    assert.throws(
      () => updateUiComponentCatalog({ repoRoot, check: true }),
      /catalog drift: .*search-index\.json/,
    );
  });
});

test('publisher workflow covers source, schema, scripts, fixtures, and version tags', () => {
  const repoRoot = path.resolve(import.meta.dirname, '..', '..');
  const workflow = fs.readFileSync(
    path.join(repoRoot, '.github/workflows/ui-component-catalog-release.yml'),
    'utf8',
  );
  assert.match(workflow, /ui_components\/\*\*/);
  assert.match(workflow, /schemas\/ui-component\/\*\*/);
  assert.match(workflow, /node --test scripts\/_tests\/ui-component-\*\.test\.mjs/);
  assert.match(workflow, /ui-component-catalog-publisher\.mjs --check/);
  assert.match(workflow, /tags: \['ui-component-catalog-v\*\.\*\.\*'\]/);
});
