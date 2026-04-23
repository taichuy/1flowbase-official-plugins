import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');

function readRepoJson(relativePath) {
  return JSON.parse(fs.readFileSync(path.join(repoRoot, relativePath), 'utf8'));
}

function readProviderManifestVersion(providerCode) {
  const manifest = fs.readFileSync(
    path.join(repoRoot, 'runtime-extensions', 'model-providers', providerCode, 'manifest.yaml'),
    'utf8'
  );
  const match = manifest.match(/^version:\s*(.+)$/m);
  assert.ok(match, `missing version in manifest for ${providerCode}`);
  return match[1].trim();
}

test('official-registry.json tracks the current openai_compatible manifest and six-target schema', () => {
  const registry = readRepoJson('official-registry.json');
  const entry = registry.plugins.find(
    (item) => item.provider_code === 'openai_compatible'
  );

  assert.ok(entry, 'missing openai_compatible entry in official-registry.json');
  assert.equal(entry.latest_version, readProviderManifestVersion('openai_compatible'));
  assert.equal(entry.plugin_type, 'model_provider');
  assert.deepEqual(entry.i18n_summary.available_locales, ['en_US', 'zh_Hans']);
  assert.equal(entry.i18n_summary.default_locale, 'en_US');
  assert.equal(
    entry.i18n_summary.bundles.en_US.plugin.label,
    readRepoJson('runtime-extensions/model-providers/openai_compatible/i18n/en_US.json').plugin
      .label
  );
  assert.equal(
    entry.i18n_summary.bundles.zh_Hans.plugin.label,
    readRepoJson('runtime-extensions/model-providers/openai_compatible/i18n/zh_Hans.json').plugin
      .label
  );
  assert.equal(
    entry.i18n_summary.bundles.en_US.plugin.description,
    readRepoJson('runtime-extensions/model-providers/openai_compatible/i18n/en_US.json').plugin
      .description
  );
  assert.equal(
    entry.i18n_summary.bundles.zh_Hans.plugin.description,
    readRepoJson('runtime-extensions/model-providers/openai_compatible/i18n/zh_Hans.json').plugin
      .description
  );
  assert.equal(
    entry.i18n_summary.bundles.zh_Hans.provider.label,
    readRepoJson('runtime-extensions/model-providers/openai_compatible/i18n/zh_Hans.json').provider
      .label
  );
  assert.equal(entry.artifacts.length, 6);
  assert.deepEqual(
    entry.artifacts.map((artifact) => [
      artifact.os,
      artifact.arch,
      artifact.libc ?? null,
    ]),
    [
      ['darwin', 'amd64', null],
      ['darwin', 'arm64', null],
      ['linux', 'amd64', 'musl'],
      ['linux', 'arm64', 'musl'],
      ['windows', 'amd64', 'msvc'],
      ['windows', 'arm64', 'msvc'],
    ]
  );
});
