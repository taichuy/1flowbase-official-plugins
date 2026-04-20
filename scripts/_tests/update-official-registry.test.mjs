import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

import { upsertRegistryEntry } from '../update-official-registry.mjs';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');

function readRepoJson(relativePath) {
  return JSON.parse(fs.readFileSync(path.join(repoRoot, relativePath), 'utf8'));
}

function readProviderManifestVersion(providerCode) {
  const manifest = fs.readFileSync(
    path.join(repoRoot, 'models', providerCode, 'manifest.yaml'),
    'utf8'
  );
  const match = manifest.match(/^version:\s*(.+)$/m);
  assert.ok(match, `missing version in manifest for ${providerCode}`);
  return match[1].trim();
}

function readManifestMetadataField(providerCode, section, locale) {
  const manifest = fs.readFileSync(
    path.join(repoRoot, 'models', providerCode, 'manifest.yaml'),
    'utf8'
  );
  const metadataMatch = manifest.match(
    new RegExp(`^\\s{2}${section}:\\s*$([\\s\\S]*?)(?=^\\S|^\\s{2}[a-z_]+:\\s*$)`, 'm')
  );

  assert.ok(metadataMatch, `missing metadata.${section} in manifest for ${providerCode}`);

  const localeMatch = metadataMatch[1].match(
    new RegExp(`^\\s{4}${locale}:\\s*(.+)$`, 'm')
  );
  assert.ok(
    localeMatch,
    `missing metadata.${section}.${locale} in manifest for ${providerCode}`
  );
  return localeMatch[1].trim();
}

test('upsertRegistryEntry writes latest release metadata for openai_compatible', () => {
  const registry = { version: 1, generated_at: null, plugins: [] };

  const next = upsertRegistryEntry(registry, {
    plugin_id: '1flowbase.openai_compatible',
    plugin_type: 'model_provider',
    provider_code: 'openai_compatible',
    display_name: 'OpenAI-Compatible API Provider',
    protocol: 'openai_compatible',
    latest_version: '0.1.0',
    help_url: 'https://github.com/taichuy/1flowbase-official-plugins/tree/main/models/openai_compatible',
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
  assert.equal(next.plugins[0].latest_version, '0.1.0');
  assert.equal(next.plugins[0].i18n_summary.default_locale, 'en_US');
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
    plugin_type: 'model_provider',
    provider_code: 'openai_compatible',
    display_name: 'OpenAI-Compatible API Provider',
    protocol: 'openai_compatible',
    latest_version: '0.2.1',
    help_url: 'https://github.com/taichuy/1flowbase-official-plugins/tree/main/models/openai_compatible',
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
  assert.equal(next.plugins[0].latest_version, '0.2.1');
  assert.equal(next.plugins[0].i18n_summary.default_locale, 'en_US');
  assert.equal(next.plugins[0].artifacts.length, 2);
});

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
    readManifestMetadataField('openai_compatible', 'label', 'en_US')
  );
  assert.equal(
    entry.i18n_summary.bundles.zh_Hans.plugin.label,
    readManifestMetadataField('openai_compatible', 'label', 'zh_Hans')
  );
  assert.equal(
    entry.i18n_summary.bundles.en_US.plugin.description,
    readManifestMetadataField('openai_compatible', 'description', 'en_US')
  );
  assert.equal(
    entry.i18n_summary.bundles.zh_Hans.plugin.description,
    readManifestMetadataField('openai_compatible', 'description', 'zh_Hans')
  );
  assert.equal(
    entry.i18n_summary.bundles.zh_Hans.provider.label,
    readManifestMetadataField('openai_compatible', 'label', 'zh_Hans')
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
