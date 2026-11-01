import test, { before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { snapshotDescriptor, verifyPackageAssets } from '../assets.mjs';

const repo = fileURLToPath(new URL('../../../', import.meta.url));
const pluginPath = 'runtime-extensions/@taichuy/taichuy-calendar';
const checkerPath = `${pluginPath}/scripts/check-package.mjs`;
const descriptorPath = '_assets/runtime/descriptor.json';
const backendPath = '_assets/runtime/backend.js';
const browserPath = '_assets/ui/browser.js';
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
let seed;

function tar(file, cwd, create) {
  const fd = fs.openSync(file, create ? 'w' : 'r');
  try {
    execFileSync('tar', create ? ['-czf', '-', '.'] : ['-xf', '-'], {
      cwd, stdio: create ? ['ignore', fd, 'pipe'] : [fd, 'ignore', 'pipe'],
    });
  } finally { fs.closeSync(fd); }
}

before(() => {
  seed = fs.mkdtempSync(path.join(os.tmpdir(), 'calendar-ci-seed-'));
  const source = path.join(seed, 'source');
  fs.mkdirSync(source);
  const archive = path.join(seed, 'source.tar');
  const fd = fs.openSync(archive, 'w');
  try {
    // Freeze real committed assets/contracts while Root rebuilds the working tree.
    execFileSync('git', ['archive', 'HEAD', pluginPath], { cwd: repo, stdio: ['ignore', fd, 'pipe'] });
  } finally { fs.closeSync(fd); }
  tar(archive, source, false);
  fs.copyFileSync(path.join(repo, checkerPath), path.join(source, checkerPath));
});
after(() => fs.rmSync(seed, { recursive: true, force: true }));

function fixture(t) {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), 'calendar-ci-fixture-'));
  t.after(() => fs.rmSync(base, { recursive: true, force: true }));
  const source = path.join(base, 'source');
  fs.cpSync(path.join(seed, 'source'), source, { recursive: true });
  const root = path.join(source, pluginPath);
  const binary = path.join(root, `bin/taichuy-calendar${process.platform === 'win32' ? '.exe' : ''}`);
  fs.mkdirSync(path.dirname(binary), { recursive: true });
  // Existence checks only; no test runs a runtime, Cargo or a service.
  fs.writeFileSync(binary, 'fixture binary bytes\n');
  return { base, source, root, binary, snapshot: path.join(base, 'descriptor.snapshot') };
}
const load = async f => (await import(pathToFileURL(path.join(f.root, 'scripts/check-package.mjs')).href)).checkPackage;
const cli = (f, args = []) => spawnSync(process.execPath, [path.join(f.root, 'scripts/check-package.mjs'), ...args], {
  cwd: f.base, encoding: 'utf8',
});
const helperCli = args => spawnSync(process.execPath, [path.join(repo, 'scripts/calendar-runtime-ci/assets.mjs'), ...args], { encoding: 'utf8' });
const crlf = file => {
  const original = fs.readFileSync(file);
  const changed = Buffer.from(original.toString('utf8').replace(/(?<!\r)\n/g, '\r\n'));
  assert.notDeepEqual(changed, original, 'The fixture must contain real LF bytes');
  fs.writeFileSync(file, changed);
  return changed;
};

function stage(f, windows = false) {
  const root = fs.mkdtempSync(path.join(f.base, 'package staging '));
  fs.cpSync(path.join(f.root, '_assets'), path.join(root, '_assets'), { recursive: true });
  let manifest = fs.readFileSync(path.join(f.root, 'manifest.yaml'), 'utf8');
  if (windows) manifest = manifest.replace(/^(\s*entry: bin\/taichuy-calendar)$/gm, '$1.exe');
  fs.writeFileSync(path.join(root, 'manifest.yaml'), manifest);
  fs.mkdirSync(path.join(root, 'bin'));
  fs.copyFileSync(f.binary, path.join(root, `bin/taichuy-calendar${windows ? '.exe' : ''}`));
  for (const absent of ['package.json', 'migration', 'scripts', 'types']) {
    assert.ok(!fs.existsSync(path.join(root, absent)), `Not a distributed file: ${absent}`);
  }
  return root;
}
function pack(f, root) {
  const archive = path.join(f.base, 'calendar package.1flowbasepkg');
  tar(archive, root, true);
  return archive;
}

// Covers both the unchanged default CLI and the post-cargo explicit target binary.
test('existing checker keeps default bin and accepts the real target binary override', async t => {
  const f = fixture(t);
  const result = await (await load(f))();
  assert.equal(result.frontend.status, 'release_files_ready');
  assert.equal(result.backend.status, 'backend_release_files_ready');
  assert.equal(cli(f).status, 0);
  fs.unlinkSync(f.binary);
  assert.notEqual(cli(f).status, 0);
  const target = path.join(f.base, 'target binary.exe');
  fs.writeFileSync(target, 'target-specific compiled binary');
  const actual = cli(f, ['--runtime-binary', target]);
  assert.equal(actual.status, 0, actual.stderr);
  assert.equal(JSON.parse(actual.stdout.trim().split('\n')[1]).sha256, result.backend.sha256);
});

test('pre-cargo checks assets without bin and writes an exact outside-plugin descriptor snapshot', async t => {
  const f = fixture(t);
  fs.unlinkSync(f.binary);
  await snapshotDescriptor(f.root, f.snapshot);
  const result = helperCli(['snapshot', f.root, `${f.snapshot}.cli`]);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).status, 'pre_cargo_assets_verified');
  assert.deepEqual(fs.readFileSync(f.snapshot), fs.readFileSync(path.join(f.root, descriptorPath)));
  await assert.rejects(snapshotDescriptor(f.root, f.snapshot), /EEXIST/);
  await assert.rejects(snapshotDescriptor(f.root, path.join(f.root, 'snapshot')), /outside the plugin/);
  assert.ok(!fs.existsSync(path.join(f.root, 'snapshot')));
});

test('real 34-line backend license CRLF drift fails raw byte checks, never normalization', async t => {
  const f = fixture(t);
  const file = path.join(f.root, backendPath);
  const original = fs.readFileSync(file);
  assert.equal(original.byteLength, 1690662);
  assert.equal(sha256(original), 'a57d5bb0d2990ad3f129ef72fe8a59501eac7193094d0eba5398076fa4b8c952');
  const changed = crlf(file);
  assert.equal(changed.byteLength, 1690696);
  assert.match(sha256(changed), /^b7f8/);
  await assert.rejects(snapshotDescriptor(f.root, f.snapshot), /backend byte length differs/);
  assert.ok(!fs.existsSync(f.snapshot));
  const result = cli(f, ['--runtime-binary', f.binary]);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /backend byte length differs/);
  fs.writeFileSync(file, original);
  original[100] ^= 1;
  fs.writeFileSync(file, original);
  await assert.rejects((await load(f))(), /backend SHA-256 differs/);
});

test('source checker rejects real browser CRLF, business drift and descriptor/version mismatch', async t => {
  const f = fixture(t);
  const check = await load(f);
  for (const asset of [browserPath, '_assets/ui/business/lunar-typescript.js']) {
    const file = path.join(f.root, asset);
    const original = fs.readFileSync(file);
    crlf(file);
    await assert.rejects(check());
    fs.writeFileSync(file, original);
  }
  const file = path.join(f.root, descriptorPath);
  const value = JSON.parse(fs.readFileSync(file));
  value.version = '0.0.0-mismatch';
  fs.writeFileSync(file, JSON.stringify(value));
  await assert.rejects(check());
});

test('actual tar packages pass for POSIX and Windows manifests without undistributed metadata', async t => {
  const f = fixture(t);
  await snapshotDescriptor(f.root, f.snapshot);
  for (const windows of [false, true]) {
    const root = stage(f, windows);
    const binary = path.join(root, `bin/taichuy-calendar${windows ? '.exe' : ''}`);
    const direct = cli(f, ['--asset-root', root, '--runtime-binary', binary]);
    assert.equal(direct.status, 0, direct.stderr);
    const archive = pack(f, root);
    const result = await verifyPackageAssets(archive, f.root, f.snapshot);
    assert.equal(result.status, 'package_assets_verified');
    const command = helperCli(['verify-package', f.root, f.snapshot, archive]);
    assert.equal(command.status, 0, command.stderr);
    assert.equal(JSON.parse(command.stdout).status, 'package_assets_verified');
    assert.equal(result.backend.sha256, JSON.parse(fs.readFileSync(f.snapshot)).sha256);
  }
});

test('postarchive rejects backend/browser/business CRLF, missing binary and manifest drift despite clean source', async t => {
  const f = fixture(t);
  await snapshotDescriptor(f.root, f.snapshot);
  for (const asset of [backendPath, browserPath, '_assets/ui/business/lunar-typescript.js']) {
    const root = stage(f);
    crlf(path.join(root, asset));
    await assert.rejects(verifyPackageAssets(pack(f, root), f.root, f.snapshot));
  }
  for (const missing of [descriptorPath, 'bin/taichuy-calendar']) {
    const root = stage(f);
    fs.unlinkSync(path.join(root, missing));
    await assert.rejects(verifyPackageAssets(pack(f, root), f.root, f.snapshot), /ENOENT/);
  }
  const root = stage(f, true);
  fs.appendFileSync(path.join(root, 'manifest.yaml'), '\n');
  await assert.rejects(verifyPackageAssets(pack(f, root), f.root, f.snapshot), /Package manifest must match/);
  assert.equal((await (await load(f))()).backend.status, 'backend_release_files_ready');
});

test('postarchive requires descriptor identity byte-for-byte, not just equivalent JSON', async t => {
  const f = fixture(t);
  await snapshotDescriptor(f.root, f.snapshot);
  const root = stage(f);
  fs.appendFileSync(path.join(root, descriptorPath), '\n');
  await assert.rejects(verifyPackageAssets(pack(f, root), f.root, f.snapshot), /pre-cargo include_str/);
});

test('a self-consistent backend rebuild after cargo cannot replace the compiled descriptor identity', async t => {
  const f = fixture(t);
  await snapshotDescriptor(f.root, f.snapshot);
  fs.appendFileSync(path.join(f.root, backendPath), '\n');
  const bytes = fs.readFileSync(path.join(f.root, backendPath));
  for (const relative of [descriptorPath, 'migration/backend-contract.json']) {
    const file = path.join(f.root, relative);
    const metadata = JSON.parse(fs.readFileSync(file));
    Object.assign(metadata, { bytes: bytes.byteLength, sha256: sha256(bytes) });
    fs.writeFileSync(file, JSON.stringify(metadata));
  }
  assert.equal((await (await load(f))()).backend.status, 'backend_release_files_ready');
  await assert.rejects(verifyPackageAssets(pack(f, stage(f)), f.root, f.snapshot), /pre-cargo include_str/);
});

test('failed unpack and failed checks clean temporary directories; missing snapshot/invalid CLI fail closed', async t => {
  const f = fixture(t);
  await snapshotDescriptor(f.root, f.snapshot);
  const temp = path.join(f.base, 'runner temp');
  fs.mkdirSync(temp);
  const previous = process.env.RUNNER_TEMP;
  process.env.RUNNER_TEMP = temp;
  t.after(() => { if (previous === undefined) delete process.env.RUNNER_TEMP; else process.env.RUNNER_TEMP = previous; });
  const archive = pack(f, stage(f));
  await verifyPackageAssets(archive, f.root, f.snapshot);
  assert.deepEqual(fs.readdirSync(temp), []);
  const bad = stage(f);
  crlf(path.join(bad, backendPath));
  await assert.rejects(verifyPackageAssets(pack(f, bad), f.root, f.snapshot));
  assert.deepEqual(fs.readdirSync(temp), []);
  fs.writeFileSync(archive, 'not a tar archive');
  await assert.rejects(verifyPackageAssets(archive, f.root, f.snapshot));
  assert.deepEqual(fs.readdirSync(temp), []);
  await assert.rejects(verifyPackageAssets(archive, f.root, `${f.snapshot}.missing`), /ENOENT/);
  for (const args of [['--runtime-binary'], ['--typo', f.root], ['--asset-root', f.root, '--asset-root', f.root]]) {
    assert.notEqual(cli(f, args).status, 0);
  }
  const badMode = helperCli(['typo', f.root, f.snapshot]);
  assert.notEqual(badMode.status, 0);
});

test('real Git core.autocrlf=true checkout preserves Root attributes and rejects an unprotected checkout', async t => {
  const f = fixture(t);
  const git = args => execFileSync('git', ['-c', 'core.autocrlf=true', ...args], { cwd: f.source, stdio: ['ignore', 'pipe', 'pipe'] });
  fs.copyFileSync(path.join(repo, '.gitattributes'), path.join(f.source, '.gitattributes'));
  git(['init', '-q']);
  git(['add', '--', '.']);
  const backend = fs.readFileSync(path.join(f.root, backendPath));
  const browser = fs.readFileSync(path.join(f.root, browserPath));
  const manifest = fs.readFileSync(path.join(f.root, 'manifest.yaml'));
  fs.rmSync(path.join(f.root, '_assets'), { recursive: true });
  fs.unlinkSync(path.join(f.root, 'manifest.yaml'));
  git(['checkout', '--', '.']);
  assert.deepEqual(fs.readFileSync(path.join(f.root, backendPath)), backend);
  assert.deepEqual(fs.readFileSync(path.join(f.root, browserPath)), browser);
  assert.deepEqual(fs.readFileSync(path.join(f.root, 'manifest.yaml')), manifest);
  assert.equal((await (await load(f))()).backend.status, 'backend_release_files_ready');
  fs.unlinkSync(path.join(f.source, '.gitattributes'));
  git(['rm', '--cached', '--', '.gitattributes']);
  fs.unlinkSync(path.join(f.root, backendPath));
  git(['checkout', '--', `${pluginPath}/${backendPath}`]);
  assert.equal(fs.readFileSync(path.join(f.root, backendPath)).byteLength, 1690696);
  await assert.rejects((await load(f))(), /backend byte length differs/);
});

test('calendar-only workflow hooks bracket cargo and package upload using the actual target binary', () => {
  const workflow = fs.readFileSync(path.join(repo, '.github/workflows/provider-release.yml'), 'utf8');
  const snapshot = workflow.indexOf('node scripts/calendar-runtime-ci/assets.mjs snapshot');
  const cross = workflow.indexOf('cross "${cargo_build_args[@]}"');
  const cargo = workflow.indexOf('cargo "${cargo_build_args[@]}"');
  const source = workflow.indexOf('node "${PLUGIN_DIR}/scripts/check-package.mjs" --runtime-binary "${package_runtime_binary_path}"');
  const packaging = workflow.indexOf('package_output="$(node host/scripts/node/plugin/cli.js');
  const located = workflow.indexOf('if [ -z "${package_file}" ]');
  const proof = workflow.indexOf('node scripts/calendar-runtime-ci/assets.mjs verify-package');
  const metadata = workflow.indexOf('package_name="$(basename');
  for (const index of [snapshot, cross, cargo, source, packaging, located, proof, metadata]) assert.ok(index >= 0);
  assert.ok(snapshot < cross && snapshot < cargo && cargo < source && source < packaging);
  assert.ok(packaging < located && located < proof && proof < metadata);
  for (const index of [snapshot, source, proof]) {
    assert.match(workflow.slice(index - 80, index), /if \[ "\$\{PROVIDER_CODE\}" = "taichuy-calendar" \]; then\s+$/);
  }
  assert.equal((workflow.match(/\$\{RUNNER_TEMP\}\/taichuy-calendar-descriptor\.json/g) || []).length, 2);
});
