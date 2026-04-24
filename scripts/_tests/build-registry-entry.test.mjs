import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { buildRegistryEntry } from '../build-registry-entry.mjs';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');

test('buildRegistryEntry emits plugin_type and i18n_summary', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'official-registry-entry-'));
  mkdirSync(path.join(root, 'provider'), { recursive: true });
  mkdirSync(path.join(root, 'i18n'), { recursive: true });

  writeFileSync(path.join(root, 'manifest.yaml'), 'plugin_type: model_provider\nversion: 0.2.1\n');
  writeFileSync(
    path.join(root, 'provider', 'openai_compatible.yaml'),
    'provider_code: openai_compatible\nprotocol: openai_compatible\nmodel_discovery: hybrid\n'
  );
  writeFileSync(
    path.join(root, 'i18n', 'en_US.json'),
    JSON.stringify({
      plugin: {
        label: 'OpenAI-Compatible API Provider',
        description: 'English description',
      },
      provider: {
        label: 'OpenAI-Compatible API Provider',
      },
    })
  );
  writeFileSync(
    path.join(root, 'i18n', 'zh_Hans.json'),
    JSON.stringify({
      plugin: {
        label: 'OpenAI-Compatible API Provider',
        description: '中文描述',
      },
      provider: {
        label: 'OpenAI-Compatible API Provider',
      },
    })
  );

  const entry = buildRegistryEntry({
    pluginDir: root,
    providerCode: 'openai_compatible',
    version: '0.2.1',
    artifacts: [
      {
        os: 'linux',
        arch: 'amd64',
        libc: 'musl',
        rust_target: 'x86_64-unknown-linux-musl',
        download_url: 'https://example.test/linux',
        checksum: 'sha256:abc',
      },
    ],
  });

  assert.equal(entry.plugin_type, 'model_provider');
  assert.deepEqual(entry.i18n_summary.available_locales, ['en_US', 'zh_Hans']);
  assert.equal(entry.i18n_summary.default_locale, 'en_US');
  assert.equal(entry.i18n_summary.bundles.zh_Hans.plugin.description, '中文描述');
});

test('buildRegistryEntry prefers manifest metadata for plugin label and description', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'official-registry-entry-'));
  mkdirSync(path.join(root, 'provider'), { recursive: true });
  mkdirSync(path.join(root, 'i18n'), { recursive: true });

  writeFileSync(
    path.join(root, 'manifest.yaml'),
    [
      'plugin_type: model_provider',
      'version: 0.2.1',
      'metadata:',
      '  label:',
      '    en_US: Manifest Label',
      '    zh_Hans: 清单标题',
      '  description:',
      '    en_US: Manifest Description',
      '    zh_Hans: 清单描述',
      '',
    ].join('\n')
  );
  writeFileSync(
    path.join(root, 'provider', 'openai_compatible.yaml'),
    'provider_code: openai_compatible\nprotocol: openai_compatible\nmodel_discovery: hybrid\n'
  );
  writeFileSync(
    path.join(root, 'i18n', 'en_US.json'),
    JSON.stringify({
      plugin: {
        label: 'Old English Label',
        description: 'Old English Description',
      },
      provider: {
        label: 'Provider English Label',
      },
    })
  );
  writeFileSync(
    path.join(root, 'i18n', 'zh_Hans.json'),
    JSON.stringify({
      plugin: {
        label: '旧中文标题',
        description: '旧中文描述',
      },
      provider: {
        label: '供应商中文标题',
      },
    })
  );

  const entry = buildRegistryEntry({
    pluginDir: root,
    providerCode: 'openai_compatible',
    version: '0.2.1',
    artifacts: [],
  });

  assert.equal(entry.i18n_summary.bundles.en_US.plugin.label, 'Manifest Label');
  assert.equal(
    entry.i18n_summary.bundles.en_US.plugin.description,
    'Manifest Description'
  );
  assert.equal(entry.i18n_summary.bundles.zh_Hans.plugin.label, '清单标题');
  assert.equal(entry.i18n_summary.bundles.zh_Hans.plugin.description, '清单描述');
  assert.equal(entry.i18n_summary.bundles.en_US.provider.label, 'Provider English Label');
});

test('buildRegistryEntry emits a raw GitHub icon URL for repository asset icons', () => {
  const entry = buildRegistryEntry({
    pluginDir: path.join(
      repoRoot,
      'runtime-extensions',
      'model-providers',
      'openai_compatible'
    ),
    providerCode: 'openai_compatible',
    version: '0.3.17',
    artifacts: [],
  });

  assert.equal(
    entry.icon,
    'https://raw.githubusercontent.com/taichuy/1flowbase-official-plugins/main/runtime-extensions/model-providers/openai_compatible/_assets/icon.svg'
  );
});
