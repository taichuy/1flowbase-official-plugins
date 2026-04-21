import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import {
  listProviderPackageTargets,
  readProviderPackageTarget,
} from '../list-provider-package-targets.mjs';

function writeManifestV1(root, pluginDirName, { pluginId, entry }) {
  const pluginDir = path.join(root, 'runtime-extensions', 'model-providers', pluginDirName);
  fs.mkdirSync(pluginDir, { recursive: true });
  fs.writeFileSync(
    path.join(pluginDir, 'manifest.yaml'),
    [
      'manifest_version: 1',
      pluginId ? `plugin_id: ${pluginId}` : null,
      'version: 0.3.8',
      'vendor: 1flowbase',
      'display_name: OpenAI Compatible',
      'description: OpenAI-compatible provider runtime extension',
      'source_kind: official_registry',
      'trust_level: verified_official',
      'consumption_kind: runtime_extension',
      'execution_mode: process_per_call',
      'slot_codes:',
      '  - model_provider',
      'binding_targets:',
      '  - workspace',
      'selection_mode: assignment_then_select',
      'minimum_host_version: 0.1.0',
      'contract_version: 1flowbase.provider/v1',
      'schema_version: 1flowbase.plugin.manifest/v1',
      'permissions:',
      '  network: outbound_only',
      '  secrets: provider_instance_only',
      '  storage: none',
      '  mcp: none',
      '  subprocess: deny',
      'runtime:',
      '  protocol: stdio_json',
      `  entry: ${entry}`,
      '  limits:',
      '    timeout_ms: 30000',
      '    memory_bytes: 268435456',
      'node_contributions: []',
    ].join('\n')
  );
  fs.writeFileSync(path.join(pluginDir, 'Cargo.toml'), '[package]\nname = "fixture"\nversion = "0.0.0"\n');
  return pluginDir;
}

function writeManifestV2(root, providerCode, executablePath) {
  const pluginDir = path.join(root, 'runtime-extensions', 'model-providers', providerCode);
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

test('listProviderPackageTargets supports manifest v1 plugin_id prefix and basename fallback', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'provider-targets-'));
  writeManifestV1(root, 'alpha_provider', {
    pluginId: 'alpha_provider@0.3.8',
    entry: 'bin/nested/alpha-runtime',
  });
  writeManifestV1(root, 'fallback-provider', {
    entry: 'bin/nested/fallback-provider',
  });

  const targets = listProviderPackageTargets(root);

  assert.deepEqual(targets, [
    {
      provider_code: 'alpha_provider',
      plugin_dir: 'runtime-extensions/model-providers/alpha_provider',
      binary_name: 'alpha-runtime',
    },
    {
      provider_code: 'fallback-provider',
      plugin_dir: 'runtime-extensions/model-providers/fallback-provider',
      binary_name: 'fallback-provider',
    },
  ]);
});

test('readProviderPackageTarget keeps old schema v2 plugin_code and executable path support', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'provider-target-'));
  const pluginDir = writeManifestV2(root, 'gamma_provider', 'bin/custom/nested/gamma-provider.exe');

  assert.deepEqual(readProviderPackageTarget(pluginDir, root), {
    provider_code: 'gamma_provider',
    plugin_dir: 'runtime-extensions/model-providers/gamma_provider',
    binary_name: 'gamma-provider.exe',
  });
});
