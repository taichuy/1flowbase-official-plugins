import test from 'node:test';
import assert from 'node:assert/strict';

import { upsertRegistryEntry } from '../update-official-registry.mjs';

test('upsertRegistryEntry writes latest release metadata for openai_compatible', () => {
  const registry = { version: 1, generated_at: null, plugins: [] };

  const next = upsertRegistryEntry(registry, {
    plugin_id: '1flowse.openai_compatible',
    provider_code: 'openai_compatible',
    display_name: 'OpenAI Compatible',
    protocol: 'openai_compatible',
    latest_version: '0.1.0',
    release_tag: 'openai_compatible-v0.1.0',
    download_url:
      'https://github.com/taichuy/1flowse-official-plugins/releases/download/openai_compatible-v0.1.0/pkg.1flowsepkg',
    checksum: 'sha256:abc123',
    signature_status: 'unsigned',
    help_url: 'https://github.com/taichuy/1flowse-official-plugins/tree/main/models/openai_compatible',
    model_discovery_mode: 'hybrid',
  });

  assert.equal(next.plugins.length, 1);
  assert.equal(next.plugins[0].release_tag, 'openai_compatible-v0.1.0');
});

test('upsertRegistryEntry replaces an existing plugin entry and keeps plugin_id ordering', () => {
  const registry = {
    version: 1,
    generated_at: '2026-04-18T00:00:00.000Z',
    plugins: [
      {
        plugin_id: '1flowse.z_provider',
        provider_code: 'z_provider',
        latest_version: '0.0.1',
        release_tag: 'z_provider-v0.0.1',
      },
      {
        plugin_id: '1flowse.openai_compatible',
        provider_code: 'openai_compatible',
        latest_version: '0.0.1',
        release_tag: 'openai_compatible-v0.0.1',
      },
    ],
  };

  const next = upsertRegistryEntry(registry, {
    plugin_id: '1flowse.openai_compatible',
    provider_code: 'openai_compatible',
    display_name: 'OpenAI Compatible',
    protocol: 'openai_compatible',
    latest_version: '0.1.0',
    release_tag: 'openai_compatible-v0.1.0',
    download_url:
      'https://github.com/taichuy/1flowse-official-plugins/releases/download/openai_compatible-v0.1.0/pkg.1flowsepkg',
    checksum: 'sha256:def456',
    signature_status: 'unsigned',
    help_url: 'https://github.com/taichuy/1flowse-official-plugins/tree/main/models/openai_compatible',
    model_discovery_mode: 'hybrid',
  });

  assert.equal(next.plugins.length, 2);
  assert.deepEqual(
    next.plugins.map((item) => item.plugin_id),
    ['1flowse.openai_compatible', '1flowse.z_provider']
  );
  assert.equal(next.plugins[0].latest_version, '0.1.0');
  assert.equal(next.plugins[0].checksum, 'sha256:def456');
});
