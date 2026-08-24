import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..', '..');
const plugin = path.join(root, 'runtime-extensions', '@taichuy', 'clash-proxy');

function read(relative) {
  return fs.readFileSync(path.join(plugin, relative), 'utf8');
}

test('NC-06 declares the network egress worker ABI and subscription URL input', () => {
  const manifest = read('manifest.yaml');
  const contract = read('provider/clash-proxy.yaml');

  assert.doesNotMatch(manifest, /^plugin_type:/m);
  assert.match(manifest, /contract_version: 1flowbase\.network_egress_provider\/v1/);
  assert.match(manifest, /execution_mode: stateful_runtime_worker/);
  assert.match(manifest, /protocol: stdio_json_worker/);
  assert.match(contract, /carrier: json_string/);
  for (const forbidden of ['base64', 'provider', 'v2ray_json']) {
    assert.match(contract, new RegExp(`- ${forbidden}`));
  }
});

test('NC-06 keeps SS, VMess, VLESS, and Trojan fixtures while preserving unsupported-schema negatives', () => {
  const fixture = read('tests/fixtures/representative-v1.yaml');
  const unsupported = read('tests/fixtures/unsupported-inputs.txt');

  for (const type of ['ss', 'vmess', 'vless', 'trojan']) {
    assert.match(fixture, new RegExp(`type: ${type}`));
  }
  assert.match(unsupported, /ss:\/\//);
  assert.match(unsupported, /proxy-providers/);
  assert.match(unsupported, /type: direct/);
});

test('NC-06 release builds pinned Mihomo Alpha as a separately attested core and proves tamper rejection', () => {
  const workflow = fs.readFileSync(path.join(root, '.github/workflows/provider-release.yml'), 'utf8');

  assert.match(workflow, /repository: MetaCubeX\/mihomo/);
  assert.match(workflow, /dd26c52463d8e6cbb6bc33ad9e2b4a488824e6f4/);
  assert.match(workflow, /--runtime-core-binary/);
  assert.match(workflow, /--runtime-core-gpl-license-notice/);
  assert.match(workflow, /--runtime-core-corresponding-source/);
  assert.match(workflow, /verifySignedRuntimeCoreRelease/);
  assert.match(workflow, /appendFileSync\(core, 'tamper'\)/);
});
