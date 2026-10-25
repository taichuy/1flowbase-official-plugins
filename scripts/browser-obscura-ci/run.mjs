import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawn, execFileSync } from 'node:child_process';
import { readSdkPin } from './source.mjs';

const root = path.resolve(import.meta.dirname, '../..');
const plugin = path.join(root, 'runtime-extensions/@taichuy/browser-obscura');
const host = path.join(root, 'dist/conformance-host');
const out = path.join(root, 'tmp/test-governance/browser-obscura');
const target = 'x86_64-unknown-linux-gnu';
fs.mkdirSync(out, { recursive: true });
const receipt = { result: 'incomplete', target, commands: [], started_at: new Date().toISOString() };
const checkpoint = () => fs.writeFileSync(path.join(out, 'receipt.json'), `${JSON.stringify(receipt, null, 2)}\n`);
const sha = cwd => execFileSync('git', ['rev-parse', 'HEAD'], { cwd, encoding: 'utf8' }).trim();
const digest = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
async function run(label, binary, args, cwd = root, env = process.env) {
  const fd = fs.openSync(path.join(out, `${label}.log`), 'w');
  const command = { label, argv: [binary, ...args], cwd: path.relative(root, cwd) || '.', started_at: new Date().toISOString() };
  receipt.commands.push(command); checkpoint();
  try {
    await new Promise((resolve, reject) => {
      const child = spawn(binary, args, { cwd, env, stdio: ['ignore', fd, fd] });
      child.once('error', reject);
      child.once('exit', (code, signal) => {
        command.exit_code = code; command.signal = signal;
        code === 0 ? resolve() : reject(new Error(`${label} failed; see ${label}.log`));
      });
    });
  } finally { command.finished_at = new Date().toISOString(); fs.closeSync(fd); checkpoint(); }
}
try {
  assert.equal(process.env.GITHUB_ACTIONS, 'true', 'This runner is CI-only; local execution is not authorized');
  assert.equal(process.platform, 'linux'); assert.equal(process.arch, 'x64');
  for (const variable of ['EXPECTED_HOST_SHA', 'EXPECTED_PLUGIN_SHA']) {
    assert.match(process.env[variable] ?? '', /^[a-f0-9]{40}$/, `${variable} must be an exact source SHA`);
  }
  receipt.plugin_source_sha = sha(root); receipt.host_source_sha = sha(host);
  assert.equal(receipt.plugin_source_sha, process.env.EXPECTED_PLUGIN_SHA);
  assert.equal(receipt.host_source_sha, process.env.EXPECTED_HOST_SHA);
  const cargoManifest = fs.readFileSync(path.join(plugin, 'Cargo.toml'), 'utf8');
  receipt.sdk_pin = readSdkPin(cargoManifest, receipt.host_source_sha);
  assert.ok(fs.existsSync(path.join(plugin, 'Cargo.lock')), 'Fixed source CI requires the plugin lockfile');
  receipt.cargo_lock_sha256 = digest(path.join(plugin, 'Cargo.lock'));
  fs.copyFileSync(path.join(plugin, 'Cargo.lock'), path.join(out, 'Cargo.lock'));
  // CI uses the runner's available CPU allocation. No taskset, half-machine cap,
  // resource-stop loop, browser daemon, or local absolute source path is needed.
  const env = { ...process.env, CARGO_BUILD_JOBS: String(os.availableParallelism()), CARGO_INCREMENTAL: '0' };
  receipt.build_jobs = Number(env.CARGO_BUILD_JOBS);
  await run('rust-version', 'rustc', ['--version', '--verbose']);
  await run('nextest-version', 'cargo', ['nextest', '--version']);
  await run('release-tooling-tests', 'node', ['--test',
    'scripts/_tests/detect-version-releases.test.mjs', 'scripts/_tests/list-provider-package-targets.test.mjs',
    'scripts/provider-release/_tests/targets.test.mjs', 'scripts/browser-obscura-ci/_tests/source.test.mjs']);
  const junit = path.join(plugin, 'target/nextest/ci/junit.xml');
  fs.rmSync(junit, { force: true });
  try {
    await run('plugin-tests', 'cargo', ['nextest', 'run', '--release', '--locked', '--all-targets', '--target', target,
      '--profile', 'ci', '--config-file', path.join(root, 'scripts/browser-obscura-ci/nextest.toml'), '--no-fail-fast'], plugin, env);
  } finally {
    if (fs.existsSync(junit)) fs.copyFileSync(junit, path.join(out, 'junit.xml'));
  }
  const report = fs.readFileSync(junit, 'utf8');
  const counts = { tests: 0, failures: 0, errors: 0, skipped: 0 };
  for (const [suite] of report.matchAll(/<testsuite\b[^>]*>/g)) {
    for (const key of Object.keys(counts)) counts[key] += Number(suite.match(new RegExp(`\\b${key}="(\\d+)"`))?.[1] ?? 0);
  }
  receipt.tests = { ...counts, passed: counts.tests - counts.failures - counts.errors - counts.skipped };
  assert.equal(counts.failures + counts.errors, 0);
  assert.ok(receipt.tests.passed > 0, 'A successful build or zero tests is not behavioral evidence');
  await run('plugin-build', 'cargo', ['build', '--release', '--locked', '--bin', 'browser-obscura', '--target', target], plugin, env);
  const runtimeBinary = path.join(plugin, `target/${target}/release/browser-obscura`);
  assert.ok(fs.statSync(runtimeBinary).isFile());
  receipt.runtime_binary_sha256 = digest(runtimeBinary);
  // Remove only previous package outputs, preventing a stale artifact from passing.
  for (const name of fs.readdirSync(out).filter(name => name.endsWith('.1flowbasepkg'))) fs.rmSync(path.join(out, name));
  await run('plugin-package', 'node', [path.join(host, 'scripts/node/plugin/cli.js'), 'package', plugin, '--out', out,
    '--runtime-binary', runtimeBinary, '--target', target]);
  receipt.packages = fs.readdirSync(out).filter(name => name.endsWith('.1flowbasepkg')).map(filename => ({ filename, sha256: digest(path.join(out, filename)) }));
  assert.equal(receipt.packages.length, 1, 'Exactly one fresh GNU package must be produced');
  assert.equal(digest(path.join(plugin, 'Cargo.lock')), receipt.cargo_lock_sha256, 'Locked tests/build must preserve the source lockfile');
  receipt.result = 'pass'; receipt.finished_at = new Date().toISOString(); checkpoint();
  process.stdout.write(`${JSON.stringify(receipt)}\n`);
} catch (error) {
  receipt.result = 'fail'; receipt.error = error.message; receipt.finished_at = new Date().toISOString(); checkpoint();
  process.stderr.write(`${error.message}\n`); process.exitCode = 1;
}
