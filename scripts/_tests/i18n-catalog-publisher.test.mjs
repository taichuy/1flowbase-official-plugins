import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  buildCatalogSeed,
  publishCatalogSeed,
  stableJson,
  verifyCatalogSeed,
} from '../i18n-catalog-publisher.mjs';

const COMMON_EN = 'i18n/@taichuy/platform/common/en_US.json';
const COMMON_ZH = 'i18n/@taichuy/platform/common/zh_Hans.json';
const SETTINGS_EN = 'i18n/@taichuy/platform/console/settings/en_US.json';
const SETTINGS_ZH = 'i18n/@taichuy/platform/console/settings/zh_Hans.json';

function sha256(value) {
  return `sha256:${crypto.createHash('sha256').update(value).digest('hex')}`;
}

function writeJson(repoRoot, relativePath, value, raw = null) {
  const filePath = path.join(repoRoot, relativePath);
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, raw ?? stableJson(value));
}

function fileEntry(locale, path, document) {
  return { locale, path, sha256: sha256(stableJson(document)) };
}

function makeFixture({ duplicateValue = '取消' } = {}) {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'official-i18n-catalog-'));
  const commonEn = { Cancel: 'Cancel', 'Save {name}': 'Save {name}' };
  const commonZh = { Cancel: '取消', 'Save {name}': '保存{name}' };
  const settingsEn = { Cancel: 'Cancel', Settings: 'Settings' };
  const settingsZh = { Cancel: duplicateValue, Settings: '设置' };
  for (const [relativePath, document] of [
    [COMMON_EN, commonEn],
    [COMMON_ZH, commonZh],
    [SETTINGS_EN, settingsEn],
    [SETTINGS_ZH, settingsZh],
  ]) writeJson(repoRoot, relativePath, document);
  writeJson(repoRoot, 'i18n/catalog.json', {
    schema_version: '1flowbase.i18n-catalog-source/v2',
    catalog_version: '2.0.0',
    source_locale: 'en_US',
    locales: ['zh_Hans', 'en_US'],
    files: [
      fileEntry('zh_Hans', SETTINGS_ZH, settingsZh),
      fileEntry('en_US', COMMON_EN, commonEn),
      fileEntry('en_US', SETTINGS_EN, settingsEn),
      fileEntry('zh_Hans', COMMON_ZH, commonZh),
    ],
    generated_at: '2026-07-30T00:00:00.000Z',
  });
  return repoRoot;
}

function readCatalog(repoRoot) {
  return JSON.parse(fs.readFileSync(path.join(repoRoot, 'i18n/catalog.json'), 'utf8'));
}

function updateCatalog(repoRoot, change) {
  const catalog = readCatalog(repoRoot);
  change(catalog);
  writeJson(repoRoot, 'i18n/catalog.json', catalog);
}

function updateLocaleFile(repoRoot, relativePath, change) {
  const document = JSON.parse(fs.readFileSync(path.join(repoRoot, relativePath), 'utf8'));
  change(document);
  writeJson(repoRoot, relativePath, document);
  updateCatalog(repoRoot, (catalog) => {
    catalog.files.find((file) => file.path === relativePath).sha256 = sha256(stableJson(document));
  });
}

function expectInvalid(repoRoot, pattern) {
  assert.throws(() => buildCatalogSeed({ repoRoot }), pattern);
}

test('AC-001 publishes module-free global keys with every supported locale explicit', () => {
  const repoRoot = makeFixture();
  const outputPath = path.join(repoRoot, 'fixture-output', 'catalog-seed.json');
  const result = publishCatalogSeed({ repoRoot, outputPath });

  assert.equal(result.seed.manifest.schema_version, '1flowbase.i18n-catalog-seed/v2');
  assert.equal(result.seed.manifest.catalog_version, '2.0.0');
  assert.deepEqual(result.seed.manifest.locales, ['en_US', 'zh_Hans']);
  assert.equal(Object.hasOwn(result.seed.manifest, 'modules'), false);
  assert.equal(Object.hasOwn(result.seed, 'modules'), false);
  assert.equal(result.seed.manifest.files.length, 4);
  assert.match(result.seed.manifest.semantic_sha256, /^sha256:[a-f0-9]{64}$/);
  assert.deepEqual(result.seed.messages, [
    { key: 'Cancel', translations: { en_US: 'Cancel', zh_Hans: '取消' } },
    { key: 'Save {name}', translations: { en_US: 'Save {name}', zh_Hans: '保存{name}' } },
    { key: 'Settings', translations: { en_US: 'Settings', zh_Hans: '设置' } },
  ]);
  assert.equal(verifyCatalogSeed(result.seed), true);
  assert.equal(fs.readFileSync(outputPath, 'utf8'), result.bytes);
});

test('AC-002 merges equal global key translations across source directories and preserves provenance', () => {
  const seed = buildCatalogSeed({ repoRoot: makeFixture() });
  assert.equal(seed.messages.filter((message) => message.key === 'Cancel').length, 1);
  const cancelFiles = seed.manifest.files.filter((file) => file.keys.includes('Cancel'));
  assert.equal(cancelFiles.length, 4);
  assert.deepEqual(cancelFiles.map((file) => file.path), [COMMON_EN, COMMON_ZH, SETTINGS_EN, SETTINGS_ZH]);
});

test('AC-003 deterministically rejects a global key with conflicting same-locale translations', () => {
  const repoRoot = makeFixture({ duplicateValue: '撤销' });
  expectInvalid(repoRoot, /conflicting global key "Cancel" for zh_Hans: .*common\/zh_Hans\.json and .*settings\/zh_Hans\.json/);
});

test('AC-004 keeps output byte-equal for semantically identical sources and preserves generated_at', () => {
  const repoRoot = makeFixture();
  const outputPath = path.join(repoRoot, 'fixture-output', 'catalog-seed.json');
  const first = publishCatalogSeed({ repoRoot, outputPath });

  writeJson(repoRoot, COMMON_EN, { 'Save {name}': 'Save {name}', Cancel: 'Cancel' }, '{"Save {name}":"Save {name}","Cancel":"Cancel"}\n');
  writeJson(repoRoot, COMMON_ZH, { 'Save {name}': '保存{name}', Cancel: '取消' }, '{"Save {name}":"保存{name}","Cancel":"取消"}\n');
  updateCatalog(repoRoot, (catalog) => {
    catalog.generated_at = '2099-01-01T00:00:00.000Z';
    catalog.locales.reverse();
    catalog.files.reverse();
  });
  const second = publishCatalogSeed({ repoRoot, outputPath });

  assert.equal(second.changed, false);
  assert.equal(second.bytes, first.bytes);
  assert.equal(second.seed.manifest.generated_at, '2026-07-30T00:00:00.000Z');
  assert.doesNotThrow(() => publishCatalogSeed({ repoRoot, outputPath, check: true }));
});

test('generated extension catalog JSON is not treated as an i18n locale source', () => {
  const repoRoot = makeFixture();
  writeJson(repoRoot, 'i18n/_maintenance/catalog-state.json', { schema_version: 'test' });
  writeJson(repoRoot, 'i18n/catalog/v1/pages/1.json', { items: [] });

  assert.doesNotThrow(() => buildCatalogSeed({ repoRoot }));
});

test('controlled negative validates schema, locale paths, source groups, and exact registry', async (context) => {
  await context.test('schema', () => {
    const repoRoot = makeFixture();
    updateCatalog(repoRoot, (catalog) => { catalog.schema_version = 'unknown/v1'; });
    expectInvalid(repoRoot, /unsupported schema_version/);
  });
  await context.test('module field is not part of schema', () => {
    const repoRoot = makeFixture();
    updateCatalog(repoRoot, (catalog) => { catalog.files[0].module = '@taichuy/platform/common'; });
    expectInvalid(repoRoot, /fields must be exactly/);
  });
  await context.test('locale path', () => {
    const repoRoot = makeFixture();
    updateCatalog(repoRoot, (catalog) => { catalog.files[0].path = 'i18n/settings/en_US.json'; });
    expectInvalid(repoRoot, /file path locale must be zh_Hans/);
  });
  await context.test('missing locale in source group', () => {
    const repoRoot = makeFixture();
    updateCatalog(repoRoot, (catalog) => { catalog.files = catalog.files.filter((file) => file.path !== SETTINGS_ZH); });
    fs.rmSync(path.join(repoRoot, SETTINGS_ZH));
    expectInvalid(repoRoot, /missing locale zh_Hans/);
  });
  await context.test('unregistered source file', () => {
    const repoRoot = makeFixture();
    writeJson(repoRoot, 'i18n/extra/en_US.json', { Extra: 'Extra' });
    expectInvalid(repoRoot, /source tree locale files must exactly match/);
  });
});

test('controlled negative rejects locale key-set mismatch', () => {
  const repoRoot = makeFixture();
  updateLocaleFile(repoRoot, COMMON_ZH, (document) => { delete document.Cancel; });
  expectInvalid(repoRoot, /keys must exactly match en_US/);
});

test('controlled negative rejects placeholder-set mismatch', () => {
  const repoRoot = makeFixture();
  updateLocaleFile(repoRoot, COMMON_ZH, (document) => { document['Save {name}'] = '保存{label}'; });
  expectInvalid(repoRoot, /placeholder set mismatch/);
});

test('controlled negative rejects source checksum mismatch and a tampered Seed', () => {
  const repoRoot = makeFixture();
  updateCatalog(repoRoot, (catalog) => { catalog.files[0].sha256 = `sha256:${'0'.repeat(64)}`; });
  expectInvalid(repoRoot, /checksum mismatch/);

  const seed = buildCatalogSeed({ repoRoot: makeFixture() });
  seed.messages[0].translations.zh_Hans = '已篡改';
  assert.throws(() => verifyCatalogSeed(seed), /tampered seed or checksum mismatch/);
});

test('controlled negative rejects HTML, JavaScript, and rich-text payloads', async (context) => {
  for (const [label, unsafeTranslation] of [
    ['HTML', '<strong>保存</strong>'],
    ['JavaScript', '${alert(1)}'],
    ['rich text', '[保存](https://example.test)'],
  ]) {
    await context.test(label, () => {
      const repoRoot = makeFixture();
      updateLocaleFile(repoRoot, COMMON_ZH, (document) => { document['Save {name}'] = unsafeTranslation; });
      expectInvalid(repoRoot, /plain text without HTML, JavaScript, or rich text/);
    });
  }
});
