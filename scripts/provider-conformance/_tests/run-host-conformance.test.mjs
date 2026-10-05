import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { stagePackages, assertHostTestEvidence, runHostConformance, HOST_TEST_NAMES } from '../run-host-conformance.mjs';

const manifest = 'manifest_version: 1\nplugin_id: session_retry_distribution\nversion: 1.0.0\nvendor: Taichuy\nslot_codes: [provider_distribution_rule]\nruntime:\n  entry: bin/session-retry-distribution\n';
const archiveName = 'Taichuy@session_retry_distribution@1.0.0@linux-amd64.1flowbasepkg';
const goodLog = `running 2 tests\n${HOST_TEST_NAMES.map(name => `test ${name} ... ok`).join('\n')}\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.1s\n`;

function fixture(t, modify = () => {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'host-package-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const officialRoot = path.join(root, 'official');
  const mainRoot = path.join(root, 'main');
  const packageDir = path.join(root, 'packages');
  const payload = path.join(root, 'payload');
  const source = path.join(officialRoot, 'runtime-extensions/@taichuy/session-retry-distribution');
  for (const dir of [source, mainRoot, packageDir, path.join(payload, 'bin')]) fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(source, 'manifest.yaml'), manifest);
  fs.writeFileSync(path.join(payload, 'manifest.yaml'), manifest);
  fs.writeFileSync(path.join(payload, 'bin/session-retry-distribution'), '#!/bin/sh\nexit 0\n', { mode: 0o755 });
  fs.mkdirSync(path.join(payload, 'readme'));
  fs.writeFileSync(path.join(payload, 'readme/zh.md'), 'actual package resource');
  modify({ payload, source, root });
  execFileSync('tar', ['-czf', path.join(packageDir, archiveName), '-C', payload, '.']);
  return { root, officialRoot, mainRoot, packageDir, stageRoot: path.join(root, 'stage'), artifact: path.join(root, 'receipt.json'), log: path.join(root, 'host.log') };
}

test('real tar package stages archive resources and executable bytes despite distinct directory, ID and vendor', t => {
  const input = fixture(t);
  const rows = stagePackages(input);
  assert.equal(rows.length, 1);
  assert.equal(rows[0].provider_code, 'session-retry-distribution');
  assert.equal(rows[0].asset_plugin_code, 'session_retry_distribution');
  assert.equal(rows[0].asset_vendor, 'Taichuy');
  assert.equal(rows[0].binary_sha256, rows[0].fixture_binary_sha256);
  const plugin = path.join(input.stageRoot, 'runtime-extensions/@taichuy/session-retry-distribution');
  assert.equal(fs.readFileSync(path.join(plugin, 'readme/zh.md'), 'utf8'), 'actual package resource');
  assert.deepEqual(fs.readFileSync(path.join(plugin, 'bin/session-retry-distribution')), fs.readFileSync(path.join(plugin, 'target/debug/session-retry-distribution')));
  assert.equal(fs.existsSync(path.join(plugin, 'src')), false);
});

for (const kind of ['missing', 'duplicate', 'unknown', 'wrong-version', 'wrong-platform']) {
  test(`closed inventory rejects ${kind} archives`, t => {
    const input = fixture(t);
    const file = path.join(input.packageDir, archiveName);
    if (kind === 'missing') fs.unlinkSync(file);
    if (kind === 'duplicate') fs.copyFileSync(file, path.join(input.packageDir, archiveName.replace('.1flowbasepkg', `@${'a'.repeat(64)}.1flowbasepkg`)));
    if (kind === 'unknown') fs.copyFileSync(file, path.join(input.packageDir, 'unknown.1flowbasepkg'));
    if (kind === 'wrong-version' || kind === 'wrong-platform') fs.renameSync(file, file.replace(kind === 'wrong-version' ? '1.0.0' : 'linux-amd64', kind === 'wrong-version' ? '9.0.0' : 'darwin-arm64'));
    assert.throws(() => stagePackages(input), /inventory|archive/u);
  });
}

for (const kind of ['manifest-mismatch', 'missing-bin', 'non-executable', 'symlink', 'hardlink']) {
  test(`archive rejects ${kind}`, t => {
    const input = fixture(t, ({ payload }) => {
      const binary = path.join(payload, 'bin/session-retry-distribution');
      if (kind === 'manifest-mismatch') fs.appendFileSync(path.join(payload, 'manifest.yaml'), '# drift\n');
      if (kind === 'missing-bin') fs.unlinkSync(binary);
      if (kind === 'non-executable') fs.chmodSync(binary, 0o644);
      if (kind === 'symlink') { fs.unlinkSync(binary); fs.symlinkSync('/bin/sh', binary); }
      if (kind === 'hardlink') fs.linkSync(binary, path.join(payload, 'bin/linked-executable'));
    });
    assert.throws(() => stagePackages(input), /manifest|binary|link|type/u);
  });
}

test('tar path escape is rejected before extraction', t => {
  const input = fixture(t);
  execFileSync('tar', ['-czf', path.join(input.packageDir, archiveName), '--transform=s|^[.]/bin|../escape|', '-C', path.join(input.root, 'payload'), '.']);
  assert.throws(() => stagePackages(input), /unsafe archive path/u);
  assert.equal(fs.existsSync(path.join(input.root, 'escape')), false);
});

test('Host evidence requires both actual test names and exactly two successful nonzero cases', () => {
  assert.deepEqual(assertHostTestEvidence(goodLog, 0), HOST_TEST_NAMES);
  for (const log of [
    '', 'test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out;',
    goodLog.replace(HOST_TEST_NAMES[1], 'unrelated'),
    goodLog.replace('2 passed', '1 passed'),
    goodLog.replace('0 ignored', '1 ignored'),
    goodLog.replace('0 filtered out', '1 filtered out'),
    goodLog.replace(' ... ok', ' ... FAILED'),
    `${goodLog}${goodLog}`,
  ]) assert.throws(() => assertHostTestEvidence(log, 0), /Host test/u);
  assert.throws(() => assertHostTestEvidence(goodLog, 1), /Host test/u);
});

test('paired SHA mismatch leaves FAIL receipt and log without running Cargo', async t => {
  const input = fixture(t);
  await assert.rejects(runHostConformance({ ...input, mainSha: 'a'.repeat(40), officialSha: 'b'.repeat(40), target: 'x86_64-unknown-linux-musl' }, {
    checkoutSha: () => 'c'.repeat(40), runCargo: () => { throw new Error('must not run'); },
  }), /SHA mismatch/u);
  assert.equal(JSON.parse(fs.readFileSync(input.artifact)).verdict, 'FAIL');
  assert.equal(fs.existsSync(input.log), true);
  assert.equal(fs.existsSync(input.stageRoot), false);
});

for (const [kind, log, status] of [['pass', goodLog, 0], ['zero', '', 0], ['partial', goodLog.replace('2 passed', '1 passed'), 0], ['failed', goodLog, 1]]) {
  test(`runner records ${kind} evidence and always cleans staging`, async t => {
    const input = fixture(t);
    const options = { ...input, mainSha: 'a'.repeat(40), officialSha: 'b'.repeat(40), target: 'x86_64-unknown-linux-musl' };
    const dependencies = {
      checkoutSha: root => root === input.mainRoot ? options.mainSha : options.officialSha,
      cargoJobs: () => '6',
      runCargo: async (_command, args, config) => {
        assert.ok(args.includes('official_plugin_compatibility'));
        assert.equal(config.env.CARGO_BUILD_JOBS, '6');
        assert.equal(config.env.ONEFLOWBASE_OFFICIAL_PLUGIN_ROOT, input.stageRoot);
        assert.equal(args.includes('--test-threads=1'), false);
        fs.appendFileSync(input.log, log);
        return status;
      },
    };
    if (kind === 'pass') await runHostConformance(options, dependencies);
    else await assert.rejects(runHostConformance(options, dependencies), /Host test/u);
    const receipt = JSON.parse(fs.readFileSync(input.artifact));
    assert.equal(receipt.verdict, kind === 'pass' ? 'PASS' : 'FAIL');
    assert.equal(receipt.packages.length, 1);
    assert.equal(receipt.main_sha, options.mainSha);
    assert.equal(receipt.official_sha, options.officialSha);
    assert.equal(fs.existsSync(input.stageRoot), false);
  });
}

test('empty expected inventory is rejected rather than claiming zero-package conformance', t => {
  const input = fixture(t);
  fs.unlinkSync(path.join(input.officialRoot, 'runtime-extensions/@taichuy/session-retry-distribution/manifest.yaml'));
  assert.throws(() => stagePackages(input), /empty.*inventory/u);
});
