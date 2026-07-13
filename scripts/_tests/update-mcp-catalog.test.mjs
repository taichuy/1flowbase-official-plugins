import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  buildMcpBundleSource,
  buildMcpCatalog
} from '../update-mcp-catalog.mjs';
import { setMcpArtifactChecksum } from '../set-mcp-artifact-checksum.mjs';

function fixtureRepository() {
  const root = mkdtempSync(path.join(tmpdir(), '1flowbase-mcp-catalog-'));
  const bundleRoot = path.join(root, 'mcp', 'taichuy', '1flowbase_zh_hans');
  mkdirSync(path.join(bundleRoot, 'tools'), { recursive: true });
  mkdirSync(path.join(bundleRoot, 'instances'), { recursive: true });
  writeFileSync(
    path.join(bundleRoot, 'manifest.json'),
    JSON.stringify({
      schema_version: '1flowbase.mcp.bundle/v1',
      organization: 'taichuy',
      bundle_id: '1flowbase_zh_hans',
      bundle_version: '1.0.0',
      locale: 'zh_Hans',
      minimum_host_version: '0.2.6',
      exported_from_system_version: '0.2.6',
      exported_at: '2026-07-13T10:00:00Z',
      files: []
    })
  );
  writeFileSync(
    path.join(bundleRoot, 'tools', 'runtime-profile.json'),
    JSON.stringify({ tool_id: 'runtime_profile', interface_id: 'get_runtime_profile' })
  );
  writeFileSync(
    path.join(bundleRoot, 'instances', 'system.json'),
    JSON.stringify({
      instance_id: 'system',
      bindings: [{ tool_id: 'runtime_profile' }]
    })
  );
  return { root, bundleRoot };
}

test('buildMcpBundleSource generates file hashes and validates stable bindings', () => {
  // AC-001 and AC-002.
  const { bundleRoot } = fixtureRepository();
  const source = buildMcpBundleSource(bundleRoot);

  assert.equal(source.manifest.files.length, 2);
  assert.deepEqual(
    source.manifest.files.map((entry) => entry.kind).sort(),
    ['instance', 'tool']
  );
  assert.ok(source.manifest.files.every((entry) => /^sha256:[a-f0-9]{64}$/.test(entry.sha256)));
});

test('buildMcpCatalog exposes the release artifact identity for each bundle', () => {
  // AC-001 and AC-002.
  const { root } = fixtureRepository();
  const catalog = buildMcpCatalog(root, {
    generatedAt: '2026-07-13T10:00:00Z'
  });

  assert.equal(catalog.version, 1);
  assert.equal(catalog.bundles.length, 1);
  assert.deepEqual(catalog.bundles[0], {
    organization: 'taichuy',
    bundle_id: '1flowbase_zh_hans',
    latest_version: '1.0.0',
    locale: 'zh_Hans',
    minimum_host_version: '0.2.6',
    exported_from_system_version: '0.2.6',
    release_tag: 'mcp-taichuy-1flowbase_zh_hans-v1.0.0',
    download_url:
      'https://github.com/taichuy/1flowbase-official-plugins/releases/download/mcp-taichuy-1flowbase_zh_hans-v1.0.0/taichuy-1flowbase_zh_hans-v1.0.0.zip'
  });
});

test('setMcpArtifactChecksum records the released ZIP digest', () => {
  const { root } = fixtureRepository();
  const catalog = buildMcpCatalog(root, { generatedAt: '2026-07-13T10:00:00Z' });
  mkdirSync(path.join(root, 'mcp'), { recursive: true });
  writeFileSync(path.join(root, 'mcp', 'catalog.json'), JSON.stringify(catalog));
  const checksum = `sha256:${'a'.repeat(64)}`;

  setMcpArtifactChecksum(
    root,
    'mcp-taichuy-1flowbase_zh_hans-v1.0.0',
    checksum
  );

  const updated = JSON.parse(readFileSync(path.join(root, 'mcp', 'catalog.json')));
  assert.equal(updated.bundles[0].artifact_sha256, checksum);
});
