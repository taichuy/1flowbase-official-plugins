import test from 'node:test';
import assert from 'node:assert/strict';

import { upsertRegistryEntry } from '../update-official-registry.mjs';

test('upsertRegistryEntry writes latest release metadata for openai_compatible', () => {
  const registry = { version: 1, generated_at: null, plugins: [] };

  const next = upsertRegistryEntry(registry, {
    plugin_id: '1flowbase.openai_compatible',
    provider_code: 'openai_compatible',
    display_name: 'OpenAI-Compatible API Provider',
    protocol: 'openai_compatible',
    latest_version: '0.1.0',
    help_url: 'https://github.com/taichuy/1flowbase-official-plugins/tree/main/models/openai_compatible',
    model_discovery_mode: 'hybrid',
    artifacts: [
      {
        os: 'linux',
        arch: 'amd64',
        libc: 'musl',
        rust_target: 'x86_64-unknown-linux-musl',
        download_url:
          'https://github.com/taichuy/1flowbase-official-plugins/releases/download/openai_compatible-v0.1.0/linux-amd64.1flowbasepkg',
        checksum: 'sha256:abc123',
      },
    ],
  });

  assert.equal(next.plugins.length, 1);
  assert.equal(next.plugins[0].latest_version, '0.1.0');
  assert.equal(next.plugins[0].artifacts.length, 1);
});

test('upsertRegistryEntry replaces one provider entry and preserves artifacts array', () => {
  const registry = {
    version: 1,
    generated_at: '2026-04-19T00:00:00Z',
    plugins: [
      {
        plugin_id: 'legacy.openai_compatible',
        provider_code: 'openai_compatible',
        latest_version: '0.2.0',
        artifacts: [
          {
            os: 'linux',
            arch: 'amd64',
            libc: 'musl',
            download_url: 'old',
            checksum: 'sha256:old',
          },
        ],
      },
    ],
  };

  const next = upsertRegistryEntry(registry, {
    plugin_id: '1flowbase.openai_compatible',
    provider_code: 'openai_compatible',
    display_name: 'OpenAI-Compatible API Provider',
    protocol: 'openai_compatible',
    latest_version: '0.2.1',
    help_url: 'https://github.com/taichuy/1flowbase-official-plugins/tree/main/models/openai_compatible',
    model_discovery_mode: 'hybrid',
    artifacts: [
      {
        os: 'linux',
        arch: 'amd64',
        libc: 'musl',
        rust_target: 'x86_64-unknown-linux-musl',
        download_url: 'amd64',
        checksum: 'sha256:amd64',
      },
      {
        os: 'linux',
        arch: 'arm64',
        libc: 'musl',
        rust_target: 'aarch64-unknown-linux-musl',
        download_url: 'arm64',
        checksum: 'sha256:arm64',
      },
    ],
  });

  assert.equal(next.plugins.length, 1);
  assert.equal(next.plugins[0].latest_version, '0.2.1');
  assert.equal(next.plugins[0].artifacts.length, 2);
});
