import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  buildCatalogSeed,
  isCanonicalModuleId,
  publishCatalogSeed,
  stableJson,
  verifyCatalogSeed,
} from '../i18n-catalog-publisher.mjs';

const MODULE_ID = '@taichuy/platform/common';
const SOURCE_PATH = `i18n/${MODULE_ID}/en_US.json`;
const TARGET_PATH = `i18n/${MODULE_ID}/zh_Hans.json`;

test('canonical module identity uses lowercase scoped multi-level segments with dot underscore and hyphen punctuation', () => {
  for (const moduleId of [
    '@taichuy/platform/common',
    '@taichuy/platform/console/settings',
    '@org/group/module.v2',
    '@org/group/module_name',
    '@org/group/module-name',
  ]) {
    assert.equal(isCanonicalModuleId(moduleId), true, moduleId);
  }

  for (const moduleId of [
    '@org/messages',
    '@Org/group/module',
    '@org/Group/module',
    '@org/group/Module',
    '@org//group/module',
    '@org/group//module',
    '@org/group/module/',
    '@org/group/.module',
    '@org/group/module+variant',
  ]) {
    assert.equal(isCanonicalModuleId(moduleId), false, moduleId);
  }
});

function sha256(value) {
  return `sha256:${crypto.createHash('sha256').update(value).digest('hex')}`;
}

function writeJson(repoRoot, relativePath, value, raw = null) {
  const filePath = path.join(repoRoot, relativePath);
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, raw ?? stableJson(value));
}

function makeFixture() {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'official-i18n-catalog-'));
  const source = ['Cancel', 'Save {name}'];
  const target = { Cancel: '取消', 'Save {name}': '保存{name}' };
  writeJson(repoRoot, SOURCE_PATH, source);
  writeJson(repoRoot, TARGET_PATH, target);
  writeJson(repoRoot, 'i18n/catalog.json', {
    schema_version: '1flowbase.i18n-catalog-source/v1',
    catalog_version: '1.0.0',
    source_locale: 'en_US',
    locales: ['zh_Hans', 'en_US'],
    modules: [MODULE_ID],
    files: [
      { module: MODULE_ID, locale: 'zh_Hans', path: TARGET_PATH, sha256: sha256(stableJson(target)) },
      { module: MODULE_ID, locale: 'en_US', path: SOURCE_PATH, sha256: sha256(stableJson(source)) },
    ],
    generated_at: '2026-07-28T00:00:00.000Z',
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

function expectInvalid(repoRoot, pattern) {
  assert.throws(() => buildCatalogSeed({ repoRoot }), pattern);
}

test('AC-001 publishes a canonical single-file Seed with English identity fallback', () => {
  const repoRoot = makeFixture();
  const outputPath = path.join(repoRoot, 'fixture-output', 'catalog-seed.json');
  const result = publishCatalogSeed({ repoRoot, outputPath });

  assert.equal(result.seed.manifest.schema_version, '1flowbase.i18n-catalog-seed/v1');
  assert.equal(result.seed.manifest.catalog_version, '1.0.0');
  assert.deepEqual(result.seed.manifest.locales, ['en_US', 'zh_Hans']);
  assert.deepEqual(result.seed.manifest.modules, [MODULE_ID]);
  assert.equal(result.seed.manifest.files.length, 2);
  assert.match(result.seed.manifest.semantic_sha256, /^sha256:[a-f0-9]{64}$/);
  assert.deepEqual(result.seed.modules, [{
    id: MODULE_ID,
    messages: [
      { msgid: 'Cancel', translations: { zh_Hans: '取消' } },
      { msgid: 'Save {name}', translations: { zh_Hans: '保存{name}' } },
    ],
  }]);
  assert.equal(Object.hasOwn(result.seed.modules[0].messages[0].translations, 'en_US'), false);
  assert.equal(verifyCatalogSeed(result.seed), true);
  assert.equal(fs.readFileSync(outputPath, 'utf8'), result.bytes);
});

test('AC-002 keeps output byte-equal for semantically identical source and preserves generated_at', () => {
  const repoRoot = makeFixture();
  const outputPath = path.join(repoRoot, 'fixture-output', 'catalog-seed.json');
  const first = publishCatalogSeed({ repoRoot, outputPath });

  writeJson(repoRoot, SOURCE_PATH, ['Save {name}', 'Cancel'], '[\n "Save {name}",\n "Cancel"\n]\n');
  writeJson(
    repoRoot,
    TARGET_PATH,
    { 'Save {name}': '保存{name}', Cancel: '取消' },
    '{"Save {name}":"保存{name}","Cancel":"取消"}\n'
  );
  updateCatalog(repoRoot, (catalog) => {
    catalog.generated_at = '2099-01-01T00:00:00.000Z';
    catalog.locales.reverse();
    catalog.files.reverse();
  });
  const second = publishCatalogSeed({ repoRoot, outputPath });

  assert.equal(second.changed, false);
  assert.equal(second.bytes, first.bytes);
  assert.equal(second.seed.manifest.generated_at, '2026-07-28T00:00:00.000Z');
  assert.doesNotThrow(() => publishCatalogSeed({ repoRoot, outputPath, check: true }));
});

test('controlled negative validates schema, normalized multi-level module identity, and exact paths', async (context) => {
  await context.test('schema', () => {
    const repoRoot = makeFixture();
    updateCatalog(repoRoot, (catalog) => { catalog.schema_version = 'unknown/v1'; });
    expectInvalid(repoRoot, /unsupported schema_version/);
  });
  await context.test('module identity', () => {
    const repoRoot = makeFixture();
    updateCatalog(repoRoot, (catalog) => { catalog.modules = ['@taichuy/common']; });
    expectInvalid(repoRoot, /multi-level\/module/);
  });
  await context.test('path', () => {
    const repoRoot = makeFixture();
    updateCatalog(repoRoot, (catalog) => { catalog.files[0].path = 'i18n/translated.json'; });
    expectInvalid(repoRoot, /file path must be/);
  });
  await context.test('unregistered source file', () => {
    const repoRoot = makeFixture();
    writeJson(repoRoot, `i18n/${MODULE_ID}/fr_FR.json`, { Cancel: 'Annuler', 'Save {name}': 'Enregistrer {name}' });
    expectInvalid(repoRoot, /source tree locale files must exactly match/);
  });
});

test('controlled negative rejects a duplicate English msgid within one module', () => {
  const repoRoot = makeFixture();
  const duplicated = ['Cancel', 'Cancel'];
  writeJson(repoRoot, SOURCE_PATH, duplicated);
  updateCatalog(repoRoot, (catalog) => {
    catalog.files.find((file) => file.locale === 'en_US').sha256 = sha256(stableJson(duplicated));
  });
  expectInvalid(repoRoot, /duplicate English msgid/);
});

test('controlled negative rejects placeholder-set mismatch', () => {
  const repoRoot = makeFixture();
  const target = { Cancel: '取消', 'Save {name}': '保存{label}' };
  writeJson(repoRoot, TARGET_PATH, target);
  updateCatalog(repoRoot, (catalog) => {
    catalog.files.find((file) => file.locale === 'zh_Hans').sha256 = sha256(stableJson(target));
  });
  expectInvalid(repoRoot, /placeholder set mismatch/);
});

test('controlled negative rejects source checksum mismatch and a tampered Seed', () => {
  const repoRoot = makeFixture();
  updateCatalog(repoRoot, (catalog) => {
    catalog.files.find((file) => file.locale === 'zh_Hans').sha256 = `sha256:${'0'.repeat(64)}`;
  });
  expectInvalid(repoRoot, /checksum mismatch/);

  const cleanRepoRoot = makeFixture();
  const seed = buildCatalogSeed({ repoRoot: cleanRepoRoot });
  seed.modules[0].messages[0].translations.zh_Hans = '已篡改';
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
      const target = { Cancel: '取消', 'Save {name}': unsafeTranslation };
      writeJson(repoRoot, TARGET_PATH, target);
      updateCatalog(repoRoot, (catalog) => {
        catalog.files.find((file) => file.locale === 'zh_Hans').sha256 = sha256(stableJson(target));
      });
      expectInvalid(repoRoot, /plain text without HTML, JavaScript, or rich text/);
    });
  }
});
