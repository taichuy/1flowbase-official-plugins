import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, cpSync, mkdirSync, readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { basename, resolve } from 'node:path';
import { finitePlainTree } from './reference.mjs';

export const FRAME_BYTES = 1024 * 1024; // shared CapabilityStdioRequest unary frame
export const binaryName = process.platform === 'win32' ? 'taichuy-calendar.exe' : 'taichuy-calendar';
export const executionBudget = { timeout_ms: 30_000, memory_bytes: null, max_output_bytes: 1_048_576 };
export function hostEnvelope(operation, body) {
  return { method: 'execute', input: {
    handler: operation, input_payload: { path: {}, query: {}, body },
    execution_budget: { ...executionBudget },
  } };
}
export function readInstalledPackage(root) {
  const descriptor = JSON.parse(readFileSync(resolve(root, '_assets/runtime/descriptor.json'), 'utf8'));
  assert.equal(basename(root), descriptor.version, 'RUNTIME_PACKAGE_ROOT must be the actual installed version directory');
  assert.equal(descriptor.path, '_assets/runtime/backend.js');
  assert.match(descriptor.version, /^[a-zA-Z0-9._-]+$/);
  const source = readFileSync(resolve(root, descriptor.path));
  assert.equal(source.length, descriptor.bytes, 'installed descriptor length must match installed bytes');
  assert.equal(createHash('sha256').update(source).digest('hex'), descriptor.sha256, 'installed descriptor hash must match installed bytes');
  const executable = readFileSync(resolve(root, 'bin', binaryName));
  return { descriptor, source: source.toString('utf8'), executableHash: createHash('sha256').update(executable).digest('hex') };
}
export function copyInstalledPackage(sourceRoot, targetParent, version) {
  const root = resolve(targetParent, version);
  mkdirSync(resolve(root, 'bin'), { recursive: true });
  copyFileSync(resolve(sourceRoot, 'bin', binaryName), resolve(root, 'bin', binaryName));
  chmodSync(resolve(root, 'bin', binaryName), 0o755);
  // No UI business copies, source modules, Node, node_modules, target or rebuild.
  cpSync(resolve(sourceRoot, '_assets/runtime'), resolve(root, '_assets/runtime'), { recursive: true });
  return root;
}
export function runBinary(root, request, { cwd = root, TZ = 'UTC', raw = false, executable, emptyPath } = {}) {
  const input = raw ? request : JSON.stringify(finitePlainTree(request));
  assert.ok(Buffer.byteLength(input) <= FRAME_BYTES + 1, 'fixture frame is intentionally bounded');
  const result = spawnSync(executable ?? resolve(root, 'bin', binaryName), [], {
    input, cwd, env: { ...process.env, TZ, ...(emptyPath ? { PATH: emptyPath, NODE_PATH: '', NODE_OPTIONS: '' } : {}) },
    encoding: 'utf8', timeout: 35_000, maxBuffer: 2 * FRAME_BYTES,
  });
  assert.ifError(result.error);
  assert.equal(result.signal, null, 'binary must complete within the host budget');
  assert.equal(result.status, 0, `binary transport process failed: ${result.stderr.slice(0, 2000)}`);
  assert.equal(result.stderr, '', 'normal protocol calls must not emit stderr');
  assert.ok(Buffer.byteLength(result.stdout) <= executionBudget.max_output_bytes, 'canonical output frame exceeds budget');
  assert.equal(result.stdout.split('\n').length, 2, 'exactly one newline-delimited protocol response');
  assert.ok(result.stdout.endsWith('\n'));
  return finitePlainTree(JSON.parse(result.stdout));
}
export function unwrapBusiness(response) {
  assert.deepEqual(Object.keys(response).sort(), ['error', 'ok', 'result']);
  assert.equal(response.ok, true, `business result must have outer ok:true: ${JSON.stringify(response.error)}`);
  assert.equal(response.error, null);
  assert.equal(typeof response.result?.ok, 'boolean');
  return response.result;
}
export function assertTransportError(response, code) {
  assert.deepEqual(response, { ok: false, result: null, error: { message: code, provider_summary: null } });
}
