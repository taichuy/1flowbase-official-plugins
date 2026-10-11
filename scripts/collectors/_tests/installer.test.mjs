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
const collectorVersion = JSON.parse(await fs.readFile(path.join(repoRoot,
  'runtime-extensions/@taichuy/codex-logs-collector/collector-manifest.json'), 'utf8')).version;
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
  const archiveName = `codex-logs-collector-${collectorVersion}-linux-${os.arch() === 'arm64' ? 'arm64' : 'amd64'}.tar.gz`;
  const archivePath = path.join(directory, archiveName);
  execFileSync('tar', ['-czf', archivePath, '-C', staging, './codex-logs-collector']);
  const bytes = await fs.readFile(archivePath);
  const sha = crypto.createHash('sha256').update(bytes).digest('hex');
  const received = [], events = new Map(); let corrupt = false, rejectUploads = false;
  const server = http.createServer(async (request, response) => {
    if (request.url === `/download/${archiveName}`) { response.end(corrupt ? Buffer.from('bad archive') : bytes); return; }
    if (request.url === '/download/checksums.txt') { response.end(`${sha}  ${archiveName}\n`); return; }
    if (request.url !== '/api/logs/v1/events') { response.writeHead(404).end(); return; }
    if (rejectUploads) { request.resume(); response.writeHead(503).end(); return; }
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
  const installDir = path.join(directory, "installed with 'quotes'"); const config = path.join(installDir, 'config.json');
  const hookHome = path.join(directory, 'codex-config');
  const args = ['--endpoint', `${base}/api/logs/v1/events`, '--installation-id', id, '--source', source,
    '--install-dir', installDir, '--release-base', `${base}/download`, '--version', collectorVersion, '--codex-home', hookHome];
  t.after(async () => {
    await new Promise(resolve => server.close(resolve)); await fs.rm(directory, { recursive: true, force: true });
  });
  return { directory, source, args, config, installDir, hookHome, received, events,
    corrupt: () => { corrupt = true; }, rejectUploads: value => { rejectUploads = value; } };
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
  const removed = await run('bash', [installer, '--uninstall', '--installation-id', f.args[3], '--install-dir', f.installDir, '--codex-home', f.hookHome]);
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

test('native CLI validates byte targets and retains oversized singleton events', async t => {
  const f = await fixture(t);
  const installed = await run('bash', [installer, ...f.args, '--no-start'],
    { FLOWBASE_AGENT_LOGS_API_KEY: 'fixture-secret' });
  assert.equal(installed.code, 0, installed.output);
  const binary = path.join(f.installDir, 'bin/codex-logs-collector');
  for (const target of ['0', '-1', 'invalid']) {
    const rejected = await run(binary, ['import', '--config', f.config, '--batch-bytes', target]);
    assert.notEqual(rejected.code, 0);
    assert.match(rejected.output, /positive integer/);
    assert.equal(f.received.length, 0);
  }
  const imported = await run(binary, ['import', '--config', f.config, '--batch-bytes', '1']);
  assert.equal(imported.code, 0, imported.output);
  assert.equal(f.events.size, 4);
  assert.equal(f.received.length, 4);
  assert.ok(f.received.every(item => item.batch.events.length === 1));
  const resumed = await run(binary, ['import', '--config', f.config]);
  assert.equal(resumed.code, 0, resumed.output);
  assert.equal(f.received.length, 4);
});

async function installHooks(f) {
  const result = await run('bash', [installer, ...f.args], { FLOWBASE_AGENT_LOGS_API_KEY: 'fixture-hook-key' });
  assert.equal(result.code, 0, result.output);
  return path.join(f.installDir, 'bin/codex-logs-collector');
}
const input = (f, event = 'Stop', extras = {}) => JSON.stringify({
  hook_event_name: event, session_id: 'fixture-session', turn_id: 'fixture-turn',
  transcript_path: path.join(f.source, 'sessions/rollout.jsonl'), ...extras,
});

test('installer merges idempotent hooks, executes the quoted command and uninstalls only its handlers', async t => {
  const f = await fixture(t);
  await fs.mkdir(f.hookHome, { recursive: true });
  const other = { type: 'command', command: 'printf "{}"', statusMessage: 'existing user hook' };
  const hookPath = path.join(f.hookHome, 'hooks.json');
  await fs.writeFile(hookPath, JSON.stringify({ description: 'keep me', hooks: { Stop: [{ hooks: [other] }] } }));
  await fs.writeFile(path.join(f.hookHome, 'config.toml'), 'notify = ["keep-existing-notify"]\n');
  await installHooks(f);
  assert.equal(f.received.length, 0, 'installation must not start background collection or import all history');
  const hooks = JSON.parse(await fs.readFile(hookPath));
  assert.equal(hooks.description, 'keep me'); assert.deepEqual(hooks.hooks.Stop[0].hooks[0], other);
  assert.equal(hooks.hooks.Stop.length, 2);
  const command = hooks.hooks.Stop[1].hooks[0].command;
  // An unrelated malformed rollout proves this handler does not scan all history.
  await fs.writeFile(path.join(f.source, 'sessions/unrelated.jsonl'), 'not a valid rollout\n');
  const captured = await run('bash', ['-c', command], {}, input(f));
  assert.equal(captured.code, 0, captured.output); assert.equal(captured.output.trim(), '{}');
  assert.equal(f.events.size, 4);
  const before = await fs.readFile(hookPath);
  await installHooks(f); assert.deepEqual(await fs.readFile(hookPath), before);
  assert.equal(await fs.readFile(path.join(f.hookHome, 'config.toml'), 'utf8'), 'notify = ["keep-existing-notify"]\n');
  const removed = await run('bash', [installer, '--uninstall', '--installation-id', f.args[3], '--install-dir', f.installDir, '--codex-home', f.hookHome]);
  assert.equal(removed.code, 0, removed.output);
  const retained = JSON.parse(await fs.readFile(hookPath));
  assert.deepEqual(retained.hooks.Stop, [{ hooks: [other] }]);
  assert.equal(retained.description, 'keep me');
});

test('Stop waits for the committed terminal record and does not upload a partial JSONL line', async t => {
  const f = await fixture(t); const binary = await installHooks(f);
  const file = path.join(f.source, 'sessions/rollout.jsonl');
  const lines = (await fs.readFile(file, 'utf8')).trimEnd().split('\n');
  await fs.writeFile(file, lines.slice(0, -1).join('\n') + '\n');
  const pending = run(binary, ['hook', '--config', f.config], {}, input(f));
  await new Promise(resolve => setTimeout(resolve, 250));
  await fs.appendFile(file, lines.at(-1));
  await new Promise(resolve => setTimeout(resolve, 250));
  assert.equal(f.events.size, 0, 'uncommitted terminal line must not be accepted');
  await fs.appendFile(file, '\n');
  const captured = await pending; assert.equal(captured.code, 0, captured.output);
  assert.equal(f.events.size, 4);
  assert.ok([...f.events.values()].some(event => event.kind === 'task_end' && event.content === 'fixture final'));
});

test('failed delivery persists across processes and a later hook retries the original source', async t => {
  const f = await fixture(t); const binary = await installHooks(f);
  f.rejectUploads(true);
  const failed = await run(binary, ['hook', '--config', f.config], {}, input(f));
  assert.equal(failed.code, 0, failed.output); assert.equal(f.events.size, 0);
  const config = JSON.parse(await fs.readFile(f.config));
  assert.ok((await fs.readdir(config.state_path + '.pending')).length > 0);
  f.rejectUploads(false);
  // A null transcript still drains pending work from an earlier event/session.
  const recovered = await run(binary, ['hook', '--config', f.config], {}, input(f, 'SessionStart', { transcript_path: null }));
  assert.equal(recovered.code, 0, recovered.output); assert.equal(f.events.size, 4);
  assert.deepEqual(await fs.readdir(config.state_path + '.pending'), []);
  assert.equal((await run(binary, ['hook', '--config', f.config], {}, input(f))).code, 0);
  assert.equal(f.received.length, 1, 'acknowledged events are not uploaded again');
});

test('concurrent hooks preserve distinct sources, source identity and one durable checkpoint', async t => {
  const f = await fixture(t); const binary = await installHooks(f);
  const second = path.join(f.source, 'sessions/second.jsonl');
  const original = await fs.readFile(path.join(f.source, 'sessions/rollout.jsonl'), 'utf8');
  await fs.writeFile(second, original.replaceAll('fixture-session', 'second-session').replaceAll('fixture-turn', 'second-turn'));
  const results = await Promise.all(Array.from({ length: 8 }, (_, index) => run(binary, ['hook', '--config', f.config], {},
    input(f, 'PostToolUse', index % 2 ? { transcript_path: second, turn_id: 'second-turn' } : {}))));
  for (const result of results) assert.equal(result.code, 0, result.output);
  assert.equal(f.events.size, 8); assert.equal(new Set(f.received.map(item => item.batch.source_id)).size, 1);
  const config = JSON.parse(await fs.readFile(f.config));
  assert.equal(Object.keys(JSON.parse(await fs.readFile(config.state_path)).files).length, 2);
  assert.deepEqual(await fs.readdir(config.state_path + '.pending'), []);
});

test('SubagentStop selects the child transcript and rejects out-of-source hook paths', async t => {
  const f = await fixture(t); const binary = await installHooks(f);
  const child = path.join(f.source, 'sessions/child.jsonl');
  const original = await fs.readFile(path.join(f.source, 'sessions/rollout.jsonl'), 'utf8');
  await fs.writeFile(child, original.replaceAll('fixture-session', 'child-session'));
  const captured = await run(binary, ['hook', '--config', f.config], {}, input(f, 'SubagentStop', { agent_transcript_path: child }));
  assert.equal(captured.code, 0, captured.output); assert.equal(f.events.size, 4);
  assert.ok([...f.events.values()].every(event => event.source_session_id === 'child-session'));
  const outside = path.join(f.directory, 'outside.jsonl'); await fs.writeFile(outside, original);
  const rejected = await run(binary, ['hook', '--config', f.config], {}, input(f, 'Stop', { transcript_path: outside }));
  assert.equal(rejected.code, 0, rejected.output); assert.equal(f.received.length, 1);
});

test('invalid existing hooks are retained byte for byte', async t => {
  const f = await fixture(t);
  await fs.mkdir(f.hookHome, { recursive: true });
  const hookPath = path.join(f.hookHome, 'hooks.json');
  await fs.writeFile(hookPath, 'malformed existing config');
  const result = await run('bash', [installer, ...f.args], { FLOWBASE_AGENT_LOGS_API_KEY: 'fixture-hook-key' });
  assert.notEqual(result.code, 0); assert.equal(await fs.readFile(hookPath, 'utf8'), 'malformed existing config');
});
