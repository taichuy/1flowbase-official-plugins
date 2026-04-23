import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import {
  syncProviderManifestFile,
  syncProviderManifestVersions,
} from '../sync-provider-manifest-versions.mjs';

function makeProviderRoot() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'provider-manifest-sync-'));
}

function writeManifest(rootDir, providerCode, content) {
  const pluginDir = path.join(rootDir, 'runtime-extensions', 'model-providers', providerCode);
  fs.mkdirSync(pluginDir, { recursive: true });
  const manifestPath = path.join(pluginDir, 'manifest.yaml');
  fs.writeFileSync(manifestPath, content);
  return manifestPath;
}

test('syncProviderManifestFile aligns manifest v1 plugin_id suffix with version', () => {
  const rootDir = makeProviderRoot();
  const manifestPath = writeManifest(
    rootDir,
    'openai_compatible',
    [
      'manifest_version: 1',
      'plugin_id: openai_compatible@0.3.11',
      'version: 0.3.12',
      'runtime:',
      '  entry: bin/openai_compatible-provider',
      '',
    ].join('\n')
  );

  const result = syncProviderManifestFile(manifestPath);

  assert.equal(result.changed, true);
  assert.equal(result.providerCode, 'openai_compatible');
  assert.equal(result.version, '0.3.12');
  assert.equal(result.nextPluginId, 'openai_compatible@0.3.12');
  assert.match(fs.readFileSync(manifestPath, 'utf8'), /^plugin_id:\s*openai_compatible@0\.3\.12$/m);
});

test('syncProviderManifestVersions leaves aligned manifests unchanged', () => {
  const rootDir = makeProviderRoot();
  writeManifest(
    rootDir,
    'openai_compatible',
    [
      'manifest_version: 1',
      'plugin_id: openai_compatible@0.3.12',
      'version: 0.3.12',
      'runtime:',
      '  entry: bin/openai_compatible-provider',
      '',
    ].join('\n')
  );

  const result = syncProviderManifestVersions(rootDir);

  assert.equal(result.changedFiles.length, 0);
  assert.equal(result.scannedFiles.length, 1);
});

test('syncProviderManifestVersions ignores non-v1 manifests', () => {
  const rootDir = makeProviderRoot();
  const manifestPath = writeManifest(
    rootDir,
    'legacy_provider',
    [
      'schema_version: 2',
      'plugin_code: legacy_provider',
      'version: 1.2.3',
      'runtime:',
      '  executable:',
      '    path: bin/legacy-provider',
      '',
    ].join('\n')
  );

  const result = syncProviderManifestVersions(rootDir);

  assert.equal(result.changedFiles.length, 0);
  assert.equal(result.scannedFiles.length, 1);
  assert.doesNotMatch(fs.readFileSync(manifestPath, 'utf8'), /^plugin_id:/m);
});
