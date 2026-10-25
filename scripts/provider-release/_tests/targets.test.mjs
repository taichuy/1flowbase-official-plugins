import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { DEFAULT_RUST_TARGETS, buildPackageMatrix, readReleaseTargets } from '../targets.mjs';

function fixture(t, sidecar) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'release-targets-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const plugin_dir = 'runtime-extensions/@taichuy/fixture';
  const plugin = path.join(root, plugin_dir);
  fs.mkdirSync(plugin, { recursive: true });
  if (sidecar !== undefined) fs.writeFileSync(path.join(plugin, 'release-targets.json'), JSON.stringify(sidecar));
  return { root, plugin, plugin_dir };
}
test('undeclared plugins retain all six existing targets and platform values', t => {
  const { root, plugin_dir } = fixture(t);
  const selected = readReleaseTargets(plugin_dir, root);
  assert.deepEqual(selected.map(target => target.rust_target), DEFAULT_RUST_TARGETS);
  assert.deepEqual(selected, [
    { runs_on: 'ubuntu-latest', os: 'linux', arch: 'amd64', libc: 'musl', rust_target: 'x86_64-unknown-linux-musl' },
    { runs_on: 'ubuntu-latest', os: 'linux', arch: 'arm64', libc: 'musl', rust_target: 'aarch64-unknown-linux-musl' },
    { runs_on: 'macos-latest', os: 'darwin', arch: 'amd64', libc: '', rust_target: 'x86_64-apple-darwin' },
    { runs_on: 'macos-latest', os: 'darwin', arch: 'arm64', libc: '', rust_target: 'aarch64-apple-darwin' },
    { runs_on: 'windows-latest', os: 'windows', arch: 'amd64', libc: 'msvc', rust_target: 'x86_64-pc-windows-msvc' },
    { runs_on: 'windows-latest', os: 'windows', arch: 'arm64', libc: 'msvc', rust_target: 'aarch64-pc-windows-msvc' },
  ]);
});
test('explicit GNU-only source selects one native runner and preserves release identity', t => {
  const { root, plugin_dir } = fixture(t, { schema_version: 1, rust_targets: ['x86_64-unknown-linux-gnu'] });
  const release = { plugin_dir, provider_code: 'fixture', version: '0.1.0', release_tag: 'fixture-v0.1.0' };
  assert.deepEqual(buildPackageMatrix([release], root), { include: [{ ...release, runs_on: 'ubuntu-22.04', os: 'linux', arch: 'amd64', libc: 'gnu', rust_target: 'x86_64-unknown-linux-gnu' }] });
});
for (const [name, value] of Object.entries({
  empty: { schema_version: 1, rust_targets: [] },
  unknown: { schema_version: 1, rust_targets: ['wasm32-wasi'] },
  prototype: { schema_version: 1, rust_targets: ['__proto__'] },
  duplicate: { schema_version: 1, rust_targets: ['x86_64-unknown-linux-gnu', 'x86_64-unknown-linux-gnu'] },
  version: { schema_version: 2, rust_targets: ['x86_64-unknown-linux-gnu'] },
  extra: { schema_version: 1, rust_targets: ['x86_64-unknown-linux-gnu'], script: 'custom.sh' },
  scalar: ['x86_64-unknown-linux-gnu'],
  null: null,
})) test(`reject invalid sidecar: ${name}`, t => {
  const { root, plugin_dir } = fixture(t, value);
  assert.throws(() => readReleaseTargets(plugin_dir, root), /Invalid release-targets/);
});
test('reject malformed JSON rather than silently restoring broad defaults', t => {
  const { root, plugin, plugin_dir } = fixture(t);
  fs.writeFileSync(path.join(plugin, 'release-targets.json'), '{');
  assert.throws(() => readReleaseTargets(plugin_dir, root), SyntaxError);
});
test('reject directory traversal and absolute plugin paths', t => {
  const { root } = fixture(t);
  for (const name of ['../fixture', '/tmp/fixture', 'runtime-extensions/@taichuy/../fixture']) assert.throws(() => readReleaseTargets(name, root), /Invalid release plugin directory/);
});
test('reject symlinked sidecars and plugin-directory aliases', t => {
  const { root, plugin, plugin_dir } = fixture(t);
  const external = path.join(root, 'external.json');
  fs.writeFileSync(external, JSON.stringify({ schema_version: 1, rust_targets: ['x86_64-unknown-linux-gnu'] }));
  fs.symlinkSync(external, path.join(plugin, 'release-targets.json'));
  assert.throws(() => readReleaseTargets(plugin_dir, root), /symlink/);
  fs.symlinkSync(plugin, path.join(root, 'runtime-extensions/@taichuy/alias'), 'dir');
  assert.throws(() => readReleaseTargets('runtime-extensions/@taichuy/alias', root), /escape or alias/);
});
test('browser source declares only the supported first-release GNU target', () => {
  const root = path.resolve(import.meta.dirname, '../../..');
  const selected = readReleaseTargets('runtime-extensions/@taichuy/browser-obscura', root);
  assert.deepEqual(selected.map(target => target.rust_target), ['x86_64-unknown-linux-gnu']);
});
test('broken sidecar symlinks fail rather than enabling the default six targets', t => {
  const { root, plugin, plugin_dir } = fixture(t);
  fs.symlinkSync(path.join(root, 'missing.json'), path.join(plugin, 'release-targets.json'));
  assert.throws(() => readReleaseTargets(plugin_dir, root), /symlink/);
});
