import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  MCP_CATALOG_SCHEMA,
  buildMcpBundleSource,
  signMcpArtifact,
  updateMcpReleaseCatalog,
  verifyMcpArtifact,
} from '../update-mcp-catalog.mjs';

const TEST_PRIVATE_KEY = `-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEIJc4ZPHgt74X/YRVKoTqHBTYTVWQs2he1XCxFc0SiD3I
-----END PRIVATE KEY-----
`;
const TEST_PUBLIC_KEY = `-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEA6s4oAVb83/zXHAYtpr0Sj53dmBwiLCFjrJ2Yb3VLzr4=
-----END PUBLIC KEY-----
`;

function fixtureRepository(version = '1.1.1') {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcp-release-'));
  const bundleRoot = path.join(root, 'mcp', '@taichuy', 'example');
  fs.mkdirSync(path.join(bundleRoot, 'tools'), { recursive: true });
  fs.mkdirSync(path.join(bundleRoot, 'instances'), { recursive: true });
  fs.writeFileSync(path.join(bundleRoot, 'manifest.json'), JSON.stringify({
    schema_version: '1flowbase.mcp.bundle/v2', organization: 'taichuy', bundle_id: 'example',
    bundle_version: version, locale: 'zh_Hans', minimum_host_version: '0.3.1',
    exported_from_system_version: '0.3.1', exported_at: '2026-08-02T00:00:00Z', files: [],
  }));
  fs.writeFileSync(path.join(bundleRoot, 'tools', 'tool.json'), JSON.stringify({
    tool_id: 'tool', execution_target: { kind: 'interface_wrapper', interface_id: 'get_tool' },
  }));
  fs.writeFileSync(path.join(bundleRoot, 'instances', 'instance.json'), JSON.stringify({
    instance_id: 'instance', bindings: [{ tool_id: 'tool' }],
  }));
  const manifestPath = path.join(bundleRoot, 'manifest.json');
  const manifest = JSON.parse(fs.readFileSync(manifestPath));
  manifest.files = [
    { path: 'tools/tool.json', kind: 'tool' },
    { path: 'instances/instance.json', kind: 'instance' },
  ].map((entry) => ({
    ...entry,
    sha256: `sha256:${createHash('sha256').update(fs.readFileSync(path.join(bundleRoot, entry.path))).digest('hex')}`,
  }));
  fs.writeFileSync(manifestPath, JSON.stringify(manifest));
  return { root, bundleRoot };
}

function signed(root, bundleRoot, version, bytes = Buffer.from(`exact zip bytes ${version}`)) {
  const artifactPath = path.join(root, `${version}.zip`);
  fs.writeFileSync(artifactPath, bytes);
  const record = signMcpArtifact({
    artifactPath, bundleRoot, privateKeyPem: TEST_PRIVATE_KEY, keyId: 'official-test-key',
    downloadUrl: `https://github.com/taichuy/1flowbase-official-plugins/releases/download/mcp-taichuy-example-v${version}/taichuy-example-v${version}.zip`,
  });
  return { artifactPath, bytes, record };
}

test('AC-001 source validation requires the exporting system as the host requirement', () => {
  const { bundleRoot } = fixtureRepository();
  assert.equal(buildMcpBundleSource(bundleRoot).manifest.files.length, 2);
  const manifestPath = path.join(bundleRoot, 'manifest.json');
  const manifest = JSON.parse(fs.readFileSync(manifestPath));
  manifest.minimum_host_version = '0.3.0';
  fs.writeFileSync(manifestPath, JSON.stringify(manifest));
  assert.throws(() => buildMcpBundleSource(bundleRoot), /must equal exported_from_system_version/);
});

test('AC-003 signs and checksums the exact ZIP bytes and rejects tampering', () => {
  const { root, bundleRoot } = fixtureRepository();
  const { bytes, record } = signed(root, bundleRoot, '1.1.1');
  assert.equal(record.checksum, `sha256:${createHash('sha256').update(bytes).digest('hex')}`);
  assert.equal(record.algorithm, 'ed25519');
  assert.equal(verifyMcpArtifact({ artifactBytes: bytes, record, publicKeyPem: TEST_PUBLIC_KEY }), true);
  assert.equal(verifyMcpArtifact({ artifactBytes: Buffer.concat([bytes, Buffer.from('tamper')]), record, publicKeyPem: TEST_PUBLIC_KEY }), false);
});

test('AC-003 retains immutable signed version history in maintenance state and catalog', () => {
  const { root, bundleRoot } = fixtureRepository('1.1.1');
  const first = signed(root, bundleRoot, '1.1.1').record;
  updateMcpReleaseCatalog({ repoRoot: root, records: [{ organization: 'taichuy', bundle_id: 'example', record: first }], generatedAt: '2026-08-02T01:00:00Z' });

  const manifestPath = path.join(bundleRoot, 'manifest.json');
  const manifest = JSON.parse(fs.readFileSync(manifestPath));
  manifest.bundle_version = '1.1.2';
  fs.writeFileSync(manifestPath, JSON.stringify(manifest));
  const second = signed(root, bundleRoot, '1.1.2').record;
  const catalog = updateMcpReleaseCatalog({ repoRoot: root, records: [{ organization: 'taichuy', bundle_id: 'example', record: second }], generatedAt: '2026-08-02T02:00:00Z' });

  assert.equal(catalog.schema_version, MCP_CATALOG_SCHEMA);
  assert.deepEqual(catalog.bundles[0].versions.map((entry) => entry.bundle_version), ['1.1.1', '1.1.2']);
  const history = JSON.parse(fs.readFileSync(path.join(root, 'mcp', '_maintenance', 'release-history.json')));
  assert.deepEqual(history.bundles[0].versions, catalog.bundles[0].versions);
});

test('AC-003 rejects content drift for the same immutable release tag', () => {
  const { root, bundleRoot } = fixtureRepository();
  const first = signed(root, bundleRoot, '1.1.1').record;
  updateMcpReleaseCatalog({ repoRoot: root, records: [{ organization: 'taichuy', bundle_id: 'example', record: first }] });
  const changed = signed(root, bundleRoot, '1.1.1', Buffer.from('changed exact zip bytes')).record;
  assert.throws(
    () => updateMcpReleaseCatalog({ repoRoot: root, records: [{ organization: 'taichuy', bundle_id: 'example', record: changed }] }),
    /immutable MCP release conflict/,
  );
});
