import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { listCollectors, PLATFORM_TARGETS, archiveName } from '../catalog.mjs';
import { listProviderPackageTargets } from '../../list-provider-package-targets.mjs';

const repoRoot = path.resolve(import.meta.dirname, '../../..');
test('client collectors do not enter existing host runtime packaging', () => {
  const collectors = listCollectors(repoRoot);
  assert.equal(collectors.length, 1);
  assert.equal(collectors[0].execution_target, 'client');
  assert.equal(collectors[0].protocol_version, '1flowbase.agent-logs/v1');
  assert.ok(!listProviderPackageTargets(repoRoot).some(item => item.provider_code === 'codex-logs-collector'));
  assert.deepEqual(new Set(PLATFORM_TARGETS.map(item => `${item.os}/${item.arch}`)), new Set([
    'linux/amd64', 'linux/arm64', 'darwin/amd64', 'darwin/arm64', 'windows/amd64', 'windows/arm64',
  ]));
  assert.ok(archiveName(collectors[0], PLATFORM_TARGETS[0]).endsWith('linux-amd64.tar.gz'));
});
test('a client package falsely declaring server runtime execution is rejected', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'collector-catalog-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const directory = path.join(root, 'runtime-extensions', '@taichuy', 'codex-logs-collector');
  fs.mkdirSync(directory, { recursive: true });
  const manifest = JSON.parse(fs.readFileSync(path.join(repoRoot, 'runtime-extensions/@taichuy/codex-logs-collector/collector-manifest.json'), 'utf8'));
  fs.writeFileSync(path.join(directory, 'collector-manifest.json'), JSON.stringify(manifest));
  assert.equal(listCollectors(root).length, 1);
  fs.writeFileSync(path.join(directory, 'manifest.yaml'), 'consumption_kind: runtime_extension');
  assert.throws(() => listCollectors(root), /server runtime discovery/);
  fs.unlinkSync(path.join(directory, 'manifest.yaml'));
  manifest.execution_target = 'server';
  fs.writeFileSync(path.join(directory, 'collector-manifest.json'), JSON.stringify(manifest));
  assert.throws(() => listCollectors(root), /Invalid client collector manifest/);
});
