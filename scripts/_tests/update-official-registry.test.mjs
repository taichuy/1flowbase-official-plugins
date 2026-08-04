import test from 'node:test';
import assert from 'node:assert/strict';
import { upsertRegistryEntry } from '../update-official-registry.mjs';

test('upsertRegistryEntry writes latest release metadata for openai_compatible', () => {
  const registry = { version: 1, generated_at: null, plugins: [] };

  const next = upsertRegistryEntry(registry, {
    plugin_id: '1flowbase.openai_compatible',
    plugin_type: 'model_provider',
    publisher_namespace: '1flowbase',
    manifest_locator: 'runtime-extensions/@source-owner/openai_compatible/manifest.yaml',
    provider_code: 'openai_compatible',
    slot_codes: ['model_provider'],
    keywords: ['openai compatible', 'ai', 'openai compatible'],
    display_name: 'OpenAI-Compatible API Provider',
    icon:
      'https://raw.githubusercontent.com/taichuy/1flowbase-official-plugins/main/runtime-extensions/@taichuy/openai_compatible/_assets/icon.svg',
    protocol: 'openai_compatible',
    latest_version: '0.1.0',
    help_url:
      'https://github.com/taichuy/1flowbase-official-plugins/tree/main/runtime-extensions/@taichuy/openai_compatible',
    model_discovery_mode: 'hybrid',
    i18n_summary: {
      default_locale: 'en_US',
      available_locales: ['en_US', 'zh_Hans'],
      bundles: {
        en_US: {
          plugin: {
            label: 'OpenAI-Compatible API Provider',
          },
        },
      },
    },
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
  assert.equal(next.plugins[0].plugin_type, 'model_provider');
  assert.equal(next.plugins[0].publisher_namespace, '1flowbase');
  assert.equal(
    next.plugins[0].manifest_locator,
    'runtime-extensions/@source-owner/openai_compatible/manifest.yaml'
  );
  assert.deepEqual(next.plugins[0].slot_codes, ['model_provider']);
  assert.deepEqual(next.plugins[0].keywords, ['ai', 'openai compatible']);
  assert.equal(
    next.plugins[0].icon,
    'https://raw.githubusercontent.com/taichuy/1flowbase-official-plugins/main/runtime-extensions/@taichuy/openai_compatible/_assets/icon.svg'
  );
  assert.equal(next.plugins[0].latest_version, '0.1.0');
  assert.equal(next.plugins[0].i18n_summary.default_locale, 'en_US');
  assert.equal(next.plugins[0].artifacts.length, 1);
});

test('upsertRegistryEntry replaces one publisher-scoped provider entry and preserves artifacts array', () => {
  const registry = {
    version: 1,
    generated_at: '2026-04-19T00:00:00Z',
    plugins: [
      {
        plugin_id: '1flowbase.openai_compatible',
        publisher_namespace: '1flowbase',
        manifest_locator: 'runtime-extensions/@old-source/openai_compatible/manifest.yaml',
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
    plugin_type: 'model_provider',
    publisher_namespace: '1flowbase',
    manifest_locator: 'runtime-extensions/@source-owner/openai_compatible/manifest.yaml',
    provider_code: 'openai_compatible',
    slot_codes: ['model_provider'],
    keywords: [],
    display_name: 'OpenAI-Compatible API Provider',
    icon:
      'https://raw.githubusercontent.com/taichuy/1flowbase-official-plugins/main/runtime-extensions/@taichuy/openai_compatible/_assets/icon.svg',
    protocol: 'openai_compatible',
    latest_version: '0.2.1',
    help_url:
      'https://github.com/taichuy/1flowbase-official-plugins/tree/main/runtime-extensions/@taichuy/openai_compatible',
    model_discovery_mode: 'hybrid',
    i18n_summary: {
      default_locale: 'en_US',
      available_locales: ['en_US', 'zh_Hans'],
      bundles: {
        en_US: {
          plugin: {
            label: 'OpenAI-Compatible API Provider',
          },
        },
      },
    },
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
  assert.equal(next.plugins[0].plugin_type, 'model_provider');
  assert.equal(
    next.plugins[0].icon,
    'https://raw.githubusercontent.com/taichuy/1flowbase-official-plugins/main/runtime-extensions/@taichuy/openai_compatible/_assets/icon.svg'
  );
  assert.equal(next.plugins[0].latest_version, '0.2.1');
  assert.equal(next.plugins[0].i18n_summary.default_locale, 'en_US');
  assert.equal(next.plugins[0].artifacts.length, 2);
});

test('upsertRegistryEntry preserves two publishers sharing the same provider code', () => {
  const registry = {
    version: 1,
    generated_at: '2026-08-04T00:00:00Z',
    plugins: [
      {
        plugin_id: 'acme.shared',
        publisher_namespace: 'acme',
        manifest_locator: 'runtime-extensions/@acme-source/shared/manifest.yaml',
        provider_code: 'shared',
        latest_version: '1.0.0',
      },
      {
        plugin_id: '1flowbase.shared',
        publisher_namespace: '1flowbase',
        manifest_locator: 'runtime-extensions/@official-source/shared/manifest.yaml',
        provider_code: 'shared',
        latest_version: '1.0.0',
      },
    ],
  };

  const next = upsertRegistryEntry(registry, {
    plugin_id: 'acme.shared',
    publisher_namespace: 'acme',
    manifest_locator: 'runtime-extensions/@acme-source/shared/manifest.yaml',
    provider_code: 'shared',
    latest_version: '2.0.0',
  });

  assert.deepEqual(
    next.plugins.map((entry) => [entry.publisher_namespace, entry.provider_code, entry.latest_version]),
    [
      ['1flowbase', 'shared', '1.0.0'],
      ['acme', 'shared', '2.0.0'],
    ],
  );
});
