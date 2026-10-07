import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import crypto from 'node:crypto';
import { spawn, execFileSync } from 'node:child_process';

const repoRoot = path.resolve(import.meta.dirname, '../../..');
const installer = path.join(repoRoot, 'installers/client-collectors/install.sh');
const nativeBinary = process.env.FLOWBASE_COLLECTOR_TEST_BINARY;
if (!nativeBinary) throw new Error('FLOWBASE_COLLECTOR_TEST_BINARY must identify the built native collector');
const row = (type, payload) => ({ type, payload, timestamp: '2026-10-07T08:00:00Z' });
const encode = rows => rows.map(item => JSON.stringify(item)).join('\n') + '\n';
const run = (command, args, env = {}, input = '') => new Promise((resolve, reject) => {
  const child = spawn(command, args, { env: { ...process.env, ...env }, stdio: ['pipe', 'pipe', 'pipe'] });
  let output = '';
  child.stdout.on('data', bytes => { output += bytes; }); child.stderr.on('data', bytes => { output += bytes; });
  child.on('error', reject); child.on('close', code => resolve({ code, output }));
  child.stdin.end(input);
});

async function fixture(t) {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'native-installer-'));
  const source = path.join(directory, 'custom-codex');
  await fs.mkdir(path.join(source, 'sessions'), { recursive: true });
  await fs.writeFile(path.join(source, 'history.jsonl'), 'this is not a rollout\n');
  await fs.writeFile(path.join(source, 'sessions/rollout.jsonl'), encode([
    row('session_meta', { id: 'fixture-session', model_provider: 'fixture' }),
    row('turn_context', { turn_id: 'fixture-turn', model: 'fixture-model' }),
    row('response_item', { type: 'message', role: 'user', content: [{ text: 'fixture question' }] }),
    row('event_msg', { type: 'task_complete', turn_id: 'fixture-turn', last_agent_message: 'fixture final' }),
  ]));
  const staging = path.join(directory, 'release'); await fs.mkdir(staging);
  await fs.copyFile(nativeBinary, path.join(staging, 'codex-logs-collector'));
  await fs.chmod(path.join(staging, 'codex-logs-collector'), 0o755);
  const archiveName = `codex-logs-collector-0.1.0-linux-${os.arch() === 'arm64' ? 'arm64' : 'amd64'}.tar.gz`;
  const archivePath = path.join(directory, archiveName);
  execFileSync('tar', ['-czf', archivePath, '-C', staging, './codex-logs-collector']);
  const bytes = await fs.readFile(archivePath);
  const sha = crypto.createHash('sha256').update(bytes).digest('hex');
  const received = [], events = new Map(); let corrupt = false;
  const server = http.createServer(async (request, response) => {
    if (request.url === `/download/${archiveName}`) { response.end(corrupt ? Buffer.from('bad archive') : bytes); return; }
    if (request.url === '/download/checksums.txt') { response.end(`${sha}  ${archiveName}\n`); return; }
    if (request.url !== '/api/logs/v1/events') { response.writeHead(404).end(); return; }
    let body = ''; for await (const chunk of request) body += chunk;
    const batch = JSON.parse(body); received.push({ batch, authorization: request.headers.authorization });
    let fresh = 0, duplicates = 0;
    for (const event of batch.events) {
      const identity = `${batch.source_id}:${event.event_id}`;
      if (events.has(identity)) { assert.deepEqual(events.get(identity), event); duplicates++; }
      else { events.set(identity, event); fresh++; }
    }
    response.setHeader('Content-Type', 'application/json');
    response.end(JSON.stringify({ data: { accepted_events: fresh, duplicate_events: duplicates,
      record_ids: ['00000000-0000-0000-0000-000000000001'] }, meta: null }));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  const id = `qa-${process.pid}-${crypto.randomUUID()}`;
  const installDir = path.join(directory, 'installed'); const config = path.join(installDir, 'config.json');
  const args = ['--endpoint', `${base}/api/logs/v1/events`, '--installation-id', id, '--source', source,
    '--install-dir', installDir, '--release-base', `${base}/download`, '--version', '0.1.0'];
  t.after(async () => {
    if (process.env.FLOWBASE_COLLECTOR_SYSTEMD_TEST === '1') await run('bash', [installer, '--uninstall', '--installation-id', id, '--install-dir', installDir]);
    await new Promise(resolve => server.close(resolve)); await fs.rm(directory, { recursive: true, force: true });
  });
  return { directory, source, args, config, installDir, received, events, corrupt: () => { corrupt = true; } };
}

test('real native download/config/import/reinstall retains identity, checkpoint and source', async t => {
  const f = await fixture(t); const key = 'fixture-private-key';
  const install = () => run('bash', [installer, ...f.args, '--no-start'], { FLOWBASE_AGENT_LOGS_API_KEY: key });
  const first = await install(); assert.equal(first.code, 0, first.output); assert.ok(!first.output.includes(key));
  const config = JSON.parse(await fs.readFile(f.config, 'utf8'));
  assert.equal(config.endpoint.endsWith('/api/logs/v1/events'), true); assert.equal(config.api_key, key);
  assert.equal((await fs.stat(f.config)).mode & 0o777, 0o600);
  const binary = path.join(f.installDir, 'bin/codex-logs-collector');
  const imported = await run(binary, ['import', '--config', f.config], { CODEX_HOME: path.join(f.directory, 'different-runtime-home') });
  assert.equal(imported.code, 0, imported.output); assert.equal(f.events.size, 4);
  const stateBefore = await fs.readFile(config.state_path, 'utf8');
  const second = await install(); assert.equal(second.code, 0, second.output);
  assert.equal(await fs.readFile(config.state_path, 'utf8'), stateBefore);
  assert.equal((await run(binary, ['import', '--config', f.config])).code, 0);
  assert.equal(f.events.size, 4); assert.equal(f.received.length, 1);
  assert.ok(f.received.every(item => item.authorization === `Bearer ${key}`));
  const removed = await run('bash', [installer, '--uninstall', '--installation-id', f.args[3], '--install-dir', f.installDir]);
  assert.equal(removed.code, 0, removed.output);
  assert.equal(await fs.readFile(config.state_path, 'utf8'), stateBefore);
  assert.ok((await fs.readFile(path.join(f.source, 'sessions/rollout.jsonl'), 'utf8')).includes('fixture question'));
  await assert.rejects(fs.stat(binary), { code: 'ENOENT' });
});

test('corrupted release cannot replace existing executable or configuration', async t => {
  const f = await fixture(t);
  const env = { FLOWBASE_AGENT_LOGS_API_KEY: 'fixture-secret' };
  assert.equal((await run('bash', [installer, ...f.args, '--no-start'], env)).code, 0);
  const binary = path.join(f.installDir, 'bin/codex-logs-collector');
  const before = await fs.readFile(binary); const config = await fs.readFile(f.config);
  f.corrupt();
  const result = await run('bash', [installer, ...f.args, '--no-start'], env);
  assert.notEqual(result.code, 0); assert.match(result.output, /checksum mismatch/);
  assert.deepEqual(await fs.readFile(binary), before); assert.deepEqual(await fs.readFile(f.config), config);
});

if (process.env.FLOWBASE_COLLECTOR_SYSTEMD_TEST === '1') {
  test('real user systemd starts, reinstalls and removes only its proof-owned collector', async t => {
    const f = await fixture(t); const key = 'fixture-systemd-key';
    const env = { FLOWBASE_AGENT_LOGS_API_KEY: key };
    const first = await run('bash', [installer, ...f.args], env);
    assert.equal(first.code, 0, first.output);
    const deadline = Date.now() + 15000;
    while (f.events.size < 4 && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 100));
    assert.equal(f.events.size, 4, 'native background collector must upload source facts');
    const config = JSON.parse(await fs.readFile(f.config, 'utf8'));
    const before = JSON.parse(await fs.readFile(config.state_path, 'utf8'));
    const reinstalled = await run('bash', [installer, ...f.args], env);
    assert.equal(reinstalled.code, 0, reinstalled.output);
    const after = JSON.parse(await fs.readFile(config.state_path, 'utf8'));
    assert.equal(after.source_id, before.source_id); assert.deepEqual(after.files, before.files);
  });
}
