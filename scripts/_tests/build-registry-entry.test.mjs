import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { buildRegistryEntry } from '../build-registry-entry.mjs';

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
