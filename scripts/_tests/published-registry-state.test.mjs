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
    path.join(repoRoot, 'runtime-extensions', '@taichuy', providerCode, 'manifest.yaml'),
    'utf8'
  );
  const match = manifest.match(/^version:\s*(.+)$/m);
  assert.ok(match, `missing version in manifest for ${providerCode}`);
  return match[1].trim();
}

function readProviderManifestField(providerCode, fieldName) {
  const manifest = fs.readFileSync(
    path.join(repoRoot, 'runtime-extensions', '@taichuy', providerCode, 'manifest.yaml'),
    'utf8'
  );
  const match = manifest.match(new RegExp(`^${fieldName}:\\s*(.+)$`, 'm'));
  assert.ok(match, `missing ${fieldName} in manifest for ${providerCode}`);
  return match[1].trim();
}

function parseStableSemver(value) {
  const match = value.match(/^(\d+)\.(\d+)\.(\d+)$/);
  assert.ok(match, `invalid stable semver: ${value}`);
  return match.slice(1).map(Number);
}

function compareStableSemver(left, right) {
  const leftParts = parseStableSemver(left);
  const rightParts = parseStableSemver(right);

  for (let index = 0; index < leftParts.length; index += 1) {
    if (leftParts[index] !== rightParts[index]) {
      return leftParts[index] - rightParts[index];
    }
  }

  return 0;
}

test('official-registry.json keeps the published openai_compatible state consistent without source regression', () => {
  const registry = readRepoJson('official-registry.json');
  const entry = registry.plugins.find(
    (item) => item.provider_code === 'openai_compatible'
  );

  assert.ok(entry, 'missing openai_compatible entry in official-registry.json');
  assert.ok(
    compareStableSemver(
      readProviderManifestVersion('openai_compatible'),
      entry.latest_version
    ) >= 0,
    'openai_compatible source version must not precede the published version'
  );
  assert.equal(entry.plugin_id, '1flowbase.openai_compatible');
  assert.equal(entry.publisher_namespace, '1flowbase');
  assert.equal(entry.provider_code, 'openai_compatible');
  assert.equal(entry.plugin_type, 'model_provider');
  assert.deepEqual(entry.slot_codes, ['model_provider']);
  assert.equal(
    readProviderManifestField('openai_compatible', 'publisher_namespace'),
    '1flowbase'
  );
  assert.equal(
    entry.minimum_host_version,
    readProviderManifestField('openai_compatible', 'minimum_host_version')
  );
  assert.equal(
    readProviderManifestField('openai_compatible', 'contract_version'),
    '1flowbase.provider/v2'
  );
  assert.deepEqual(entry.i18n_summary.available_locales, ['en_US', 'zh_Hans']);
  assert.equal(entry.i18n_summary.default_locale, 'en_US');
  assert.equal(
    entry.i18n_summary.bundles.en_US.plugin.label,
    readRepoJson('runtime-extensions/@taichuy/openai_compatible/i18n/en_US.json').plugin
      .label
  );
  assert.equal(
    entry.i18n_summary.bundles.zh_Hans.plugin.label,
    readRepoJson('runtime-extensions/@taichuy/openai_compatible/i18n/zh_Hans.json').plugin
      .label
  );
  assert.equal(
    entry.i18n_summary.bundles.en_US.plugin.description,
    readRepoJson('runtime-extensions/@taichuy/openai_compatible/i18n/en_US.json').plugin
      .description
  );
  assert.equal(
    entry.i18n_summary.bundles.zh_Hans.plugin.description,
    readRepoJson('runtime-extensions/@taichuy/openai_compatible/i18n/zh_Hans.json').plugin
      .description
  );
  assert.equal(
    entry.i18n_summary.bundles.zh_Hans.provider.label,
    readRepoJson('runtime-extensions/@taichuy/openai_compatible/i18n/zh_Hans.json').provider
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

  for (const artifact of entry.artifacts) {
    assert.match(
      artifact.checksum,
      /^sha256:[0-9a-f]{64}$/,
      `${entry.provider_code} ${artifact.os}-${artifact.arch} has invalid checksum`
    );
    const checksumHex = artifact.checksum.slice('sha256:'.length);
    assert.equal(
      artifact.download_url,
      `https://github.com/taichuy/1flowbase-official-plugins/releases/download/${entry.provider_code}-v${entry.latest_version}/1flowbase@${entry.provider_code}@${entry.latest_version}@${artifact.os}-${artifact.arch}@${checksumHex}.1flowbasepkg`
    );
    assert.equal(artifact.signature_algorithm, 'ed25519');
    assert.equal(artifact.signing_key_id, 'official-key-2026-04');
  }
});

test('official-registry.json stores normalized sha256 checksums', () => {
  const registry = readRepoJson('official-registry.json');

  for (const plugin of registry.plugins) {
    for (const artifact of plugin.artifacts) {
      assert.match(
        artifact.checksum,
        /^sha256:[0-9a-f]{64}$/i,
        `${plugin.provider_code} ${artifact.os}-${artifact.arch} has invalid checksum`
      );
      assert.ok(
        artifact.download_url.includes(artifact.checksum.slice('sha256:'.length)),
        `${plugin.provider_code} ${artifact.os}-${artifact.arch} checksum does not match URL`
      );
    }
  }
});
