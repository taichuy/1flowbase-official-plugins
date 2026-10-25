import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import {
  listProviderPackageTargets,
  listProviderReleaseTargets,
  readProviderPackageTarget,
} from '../list-provider-package-targets.mjs';

function writeManifestV1(root, pluginDirName, { pluginId, entry, vendor = '1flowbase' }) {
  const pluginDir = path.join(root, 'runtime-extensions', '@taichuy', pluginDirName);
  fs.mkdirSync(pluginDir, { recursive: true });
  fs.writeFileSync(
    path.join(pluginDir, 'manifest.yaml'),
    [
      'manifest_version: 1',
      pluginId ? `plugin_id: ${pluginId}` : null,
      'version: 0.3.8',
      `vendor: ${vendor}`,
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
  const pluginDir = path.join(root, 'runtime-extensions', '@taichuy', providerCode);
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

test('JSON managed notification packages preserve manifest runtime binary and release identity', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'notification-package-target-'));
  const pluginDir = path.join(root, 'runtime-extensions', '@taichuy', 'wechat-notification');
  fs.mkdirSync(pluginDir, { recursive: true });
  fs.writeFileSync(path.join(pluginDir, 'manifest.yaml'), JSON.stringify({
    manifest_version: 2, plugin_id: 'wechat-notification', version: '1.0.0',
    vendor: '1flowbase', slot_codes: [], consumption_kind: 'runtime_extension',
    notification_types: [{ type_code: '1flowbase/wechat-notification/private' }],
    runtime: { protocol: 'stdio_json_multiplex_v1', entry: 'bin/wechat-notification' },
  }));
  assert.deepEqual(readProviderPackageTarget(pluginDir, root, { rustTargetTriple: 'x86_64-pc-windows-msvc' }), {
    provider_code: 'wechat-notification', plugin_dir: 'runtime-extensions/@taichuy/wechat-notification',
    binary_name: 'wechat-notification', runtime_binary_name: 'wechat-notification.exe',
  });
  assert.deepEqual(listProviderReleaseTargets(root), [{
    asset_plugin_code: 'wechat-notification', asset_vendor: '1flowbase',
    plugin_dir: 'runtime-extensions/@taichuy/wechat-notification', provider_code: 'wechat-notification',
    release_tag: 'wechat-notification-v1.0.0', version: '1.0.0',
  }]);
});

test('listProviderPackageTargets supports manifest v1 stable plugin_id and basename fallback', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'provider-targets-'));
  writeManifestV1(root, 'alpha_provider', {
    pluginId: 'alpha_provider',
    entry: 'bin/nested/alpha-runtime',
  });
  writeManifestV1(root, 'fallback-provider', {
    entry: 'bin/nested/fallback-provider',
  });

  const targets = listProviderPackageTargets(root);

  assert.deepEqual(targets, [
    {
      provider_code: 'alpha_provider',
      plugin_dir: 'runtime-extensions/@taichuy/alpha_provider',
      binary_name: 'alpha-runtime',
    },
    {
      provider_code: 'fallback-provider',
      plugin_dir: 'runtime-extensions/@taichuy/fallback-provider',
      binary_name: 'fallback-provider',
    },
  ]);
});

test('listProviderReleaseTargets plans every source provider at its manifest version', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'provider-releases-'));
  writeManifestV1(root, 'alpha_provider', {
    pluginId: 'alpha_provider',
    entry: 'bin/alpha-provider',
    vendor: 'Taichuy',
  });
  writeManifestV2(root, 'beta-provider', 'bin/beta-provider');

  assert.deepEqual(listProviderReleaseTargets(root), [
    {
      asset_plugin_code: 'alpha_provider',
      asset_vendor: 'Taichuy',
      plugin_dir: 'runtime-extensions/@taichuy/alpha_provider',
      provider_code: 'alpha_provider',
      release_tag: 'alpha_provider-v0.3.8',
      version: '0.3.8',
    },
    {
      asset_plugin_code: 'beta-provider',
      asset_vendor: '1flowbase',
      plugin_dir: 'runtime-extensions/@taichuy/beta-provider',
      provider_code: 'beta-provider',
      release_tag: 'beta-provider-v0.1.0',
      version: '0.1.0',
    },
  ]);
});

test('readProviderPackageTarget keeps old schema v2 plugin_code and executable path support', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'provider-target-'));
  const pluginDir = writeManifestV2(root, 'gamma_provider', 'bin/custom/nested/gamma-provider.exe');

  assert.deepEqual(readProviderPackageTarget(pluginDir, root), {
    provider_code: 'gamma_provider',
    plugin_dir: 'runtime-extensions/@taichuy/gamma_provider',
    binary_name: 'gamma-provider.exe',
  });
});

test('readProviderPackageTarget derives a windows runtime binary name from rust target triples', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'provider-target-windows-'));
  const pluginDir = writeManifestV2(
    root,
    'openai_compatible',
    'bin/openai_compatible-provider'
  );

  assert.deepEqual(
    readProviderPackageTarget(pluginDir, root, {
      rustTargetTriple: 'aarch64-pc-windows-msvc',
    }),
    {
      provider_code: 'openai_compatible',
      plugin_dir: 'runtime-extensions/@taichuy/openai_compatible',
      binary_name: 'openai_compatible-provider',
      runtime_binary_name: 'openai_compatible-provider.exe',
    }
  );
});

test('readProviderPackageTarget keeps distribution release identity distinct from manifest plugin id', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'distribution-target-'));
  const pluginDir = path.join(root, 'runtime-extensions', '@taichuy', 'session-retry-distribution');
  fs.mkdirSync(pluginDir, { recursive: true });
  fs.writeFileSync(
    path.join(pluginDir, 'manifest.yaml'),
    [
      'manifest_version: 1',
      'plugin_id: session_retry_distribution',
      'version: 1.0.0',
      'slot_codes: [provider_distribution_rule]',
      'runtime:',
      '  entry: bin/session-retry-distribution',
      '',
    ].join('\n')
  );

  assert.deepEqual(readProviderPackageTarget(pluginDir, root), {
    provider_code: 'session-retry-distribution',
    plugin_dir: 'runtime-extensions/@taichuy/session-retry-distribution',
    binary_name: 'session-retry-distribution',
  });
});

test('managed-only browser package identity and executable need no synthetic slot', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'browser-package-target-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const pluginDir = path.join(root, 'runtime-extensions/@taichuy/browser-obscura');
  fs.mkdirSync(pluginDir, { recursive: true });
  fs.writeFileSync(path.join(pluginDir, 'manifest.yaml'), JSON.stringify({
    manifest_version: 2, plugin_id: 'browser-obscura', version: '0.1.0', vendor: '1flowbase',
    slot_codes: [], consumption_kind: 'runtime_extension',
    managed: { module: { contributions: [] }, execution_bindings: [] },
    runtime: { protocol: 'stdio_json_multiplex_v1', entry: 'bin/browser-obscura' },
  }));
  assert.deepEqual(readProviderPackageTarget(pluginDir, root, { rustTargetTriple: 'x86_64-unknown-linux-gnu' }), {
    provider_code: 'browser-obscura', plugin_dir: 'runtime-extensions/@taichuy/browser-obscura',
    binary_name: 'browser-obscura', runtime_binary_name: 'browser-obscura',
  });
  assert.deepEqual(listProviderReleaseTargets(root), [{
    asset_plugin_code: 'browser-obscura', asset_vendor: '1flowbase',
    plugin_dir: 'runtime-extensions/@taichuy/browser-obscura', provider_code: 'browser-obscura',
    release_tag: 'browser-obscura-v0.1.0', version: '0.1.0',
  }]);
});
