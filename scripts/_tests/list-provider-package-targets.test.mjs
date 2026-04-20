import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import {
  listProviderPackageTargets,
  readProviderPackageTarget,
} from '../list-provider-package-targets.mjs';

function writeManifest(root, providerCode, executablePath) {
  const pluginDir = path.join(root, 'models', providerCode);
  fs.mkdirSync(pluginDir, { recursive: true });
  fs.writeFileSync(
    path.join(pluginDir, 'manifest.yaml'),
    [
      'schema_version: 2',
      'plugin_type: model_provider',
      `plugin_code: ${providerCode}`,
      'version: 0.1.0',
      'contract_version: 1flowbase.provider/v1',
      'provider:',
      `  definition: provider/${providerCode}.yaml`,
      'runtime:',
      '  kind: executable',
      '  protocol: stdio-json',
      '  executable:',
      `    path: ${executablePath}`,
      '',
    ].join('\n')
  );
  fs.writeFileSync(path.join(pluginDir, 'Cargo.toml'), '[package]\nname = "fixture"\nversion = "0.0.0"\n');
  return pluginDir;
}

test('listProviderPackageTargets discovers every packaged provider from models directory', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'provider-targets-'));
  writeManifest(root, 'alpha_provider', 'bin/alpha-runtime');
  writeManifest(root, 'beta_provider', 'bin/beta-provider.exe');

  const targets = listProviderPackageTargets(root);

  assert.deepEqual(targets, [
    {
      provider_code: 'alpha_provider',
      plugin_dir: 'models/alpha_provider',
      binary_name: 'alpha-runtime',
    },
    {
      provider_code: 'beta_provider',
      plugin_dir: 'models/beta_provider',
      binary_name: 'beta-provider.exe',
    },
  ]);
});

test('readProviderPackageTarget returns the runtime binary basename from manifest executable path', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'provider-target-'));
  const pluginDir = writeManifest(root, 'gamma_provider', 'bin/custom/nested/gamma-provider.exe');

  assert.deepEqual(readProviderPackageTarget(pluginDir, root), {
    provider_code: 'gamma_provider',
    plugin_dir: 'models/gamma_provider',
    binary_name: 'gamma-provider.exe',
  });
});
