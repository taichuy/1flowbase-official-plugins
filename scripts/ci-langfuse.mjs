import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { spawn, execFileSync } from 'node:child_process';
const root = process.cwd();
const out = path.join(root, 'tmp/test-governance/langfuse');
const plugin = path.join(root, 'runtime-extensions/@taichuy/langfuse');
const host = path.join(root, 'dist/conformance-host');
fs.mkdirSync(out, { recursive: true });
const receipt = { result: 'incomplete', commands: [] };
const checkpoint = () => fs.writeFileSync(path.join(out, 'receipt.json'), JSON.stringify(receipt, null, 2));
const sha = cwd => execFileSync('git', ['rev-parse', 'HEAD'], { cwd, encoding: 'utf8' }).trim();
const digest = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
function cpu() { return fs.readFileSync('/proc/stat', 'utf8').split('\n')[0].trim().split(/\s+/).slice(1).map(Number); }
async function run(label, binary, args, cwd = root) {
  const fd = fs.openSync(path.join(out, `${label}.log`), 'w');
  const item = { label, command: [binary, ...args] }; receipt.commands.push(item); checkpoint();
  const child = spawn(binary, args, { cwd, env: process.env, detached: true, stdio: ['ignore', fd, fd] });
  let previous = cpu(), stopped = false;
  const monitor = setInterval(() => {
    const current = cpu(), total = current.reduce((n, v, i) => n + v - previous[i], 0);
    const idle = current[3] - previous[3] + current[4] - previous[4]; previous = current;
    const mem = Object.fromEntries([...fs.readFileSync('/proc/meminfo', 'utf8').matchAll(/^(\w+):\s+(\d+)/gm)].map(m => [m[1], Number(m[2])]));
    const disk = fs.statfsSync(root);
    const sample = { cpu: total ? 1 - idle / total : 0, memory: 1 - mem.MemAvailable / mem.MemTotal, disk: 1 - disk.bavail / disk.blocks };
    fs.appendFileSync(path.join(out, 'resources.jsonl'), JSON.stringify({ label, ...sample }) + '\n');
    if (!stopped && Object.values(sample).some(v => v >= 0.9)) { stopped = true; item.resource_stop = sample; try { process.kill(-child.pid, 'SIGKILL'); } catch {} }
  }, 2000);
  try {
    await new Promise((resolve, reject) => {
      child.once('error', reject);
      child.once('exit', (code, signal) => { item.exit_code = code; item.signal = signal; code === 0 && !stopped ? resolve() : reject(new Error(`${label} failed; see artifact log`)); });
    });
  } finally { clearInterval(monitor); fs.closeSync(fd); checkpoint(); }
}
try {
  assert.equal(process.env.GITHUB_ACTIONS, 'true');
  receipt.official_sha = sha(root); receipt.host_sha = sha(host);
  assert.equal(receipt.official_sha, process.env.EXPECTED_OFFICIAL_SHA);
  assert.equal(receipt.host_sha, process.env.EXPECTED_HOST_SHA);
  const manifest = fs.readFileSync(path.join(plugin, 'Cargo.toml'), 'utf8');
  assert.ok(manifest.includes(`rev = "${receipt.host_sha}"`), 'SDK must pin exact paired host source');
  const allowed = fs.readFileSync('/proc/self/status', 'utf8').match(/^Cpus_allowed_list:\s*(.+)$/m)[1].trim().split(',').flatMap(part => { const [a, b = a] = part.split('-').map(Number); return Array.from({ length: b - a + 1 }, (_, i) => a + i); });
  const capacity = Math.max(1, Math.floor(Math.min(os.availableParallelism(), allowed.length) / 2));
  execFileSync('taskset', ['-pc', allowed.slice(0, capacity).join(','), String(process.pid)]);
  process.env.CARGO_BUILD_JOBS = String(capacity);
  process.env.CARGO_PROFILE_DEV_DEBUG = '0'; process.env.CARGO_PROFILE_TEST_DEBUG = '0';
  const locked = fs.existsSync(path.join(plugin, 'Cargo.lock')) ? ['--locked'] : [];
  await run('plugin-tests', 'cargo', ['test', ...locked, '--all-targets', '--', '--test-threads', String(capacity)], plugin);
  const tests = fs.readFileSync(path.join(out, 'plugin-tests.log'), 'utf8');
  const counts = [...tests.matchAll(/test result: ok\. (\d+) passed/g)].map(m => Number(m[1]));
  assert.ok(counts.reduce((a, b) => a + b, 0) > 0, 'targeted plugin tests must execute');
  receipt.tests = counts.reduce((a, b) => a + b, 0);
  await run('plugin-build', 'cargo', ['build', '--locked', '--bin', 'langfuse'], plugin);
  await run('plugin-package', 'node', [path.join(host, 'scripts/node/plugin/cli.js'), 'package', plugin, '--out', out, '--runtime-binary', path.join(plugin, 'target/debug/langfuse'), '--target', 'x86_64-unknown-linux-gnu']);
  fs.copyFileSync(path.join(plugin, 'Cargo.lock'), path.join(out, 'Cargo.lock'));
  receipt.packages = fs.readdirSync(out).filter(f => f.endsWith('.1flowbasepkg')).map(filename => ({ filename, sha256: digest(path.join(out, filename)) }));
  assert.equal(receipt.packages.length, 1);
  receipt.result = 'pass'; checkpoint();
  process.stdout.write(JSON.stringify(receipt) + '\n');
} catch (error) { receipt.result = 'fail'; receipt.error = error.message; checkpoint(); process.stderr.write(error.message + '\n'); process.exitCode = 1; }
