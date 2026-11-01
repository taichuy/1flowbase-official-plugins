import assert from 'node:assert/strict';
import test, { before, after } from 'node:test';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync, copyFileSync, chmodSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { corpus, stateSequence, timezones } from '../runtime-tests/corpus.mjs';
import { assertFixture, createReference, observe } from '../runtime-tests/reference.mjs';
import { assertTransportError, binaryName, copyInstalledPackage, FRAME_BYTES, hostEnvelope, readInstalledPackage, runBinary, unwrapBusiness } from '../runtime-tests/binary.mjs';

// Missing installed package is a visible SKIP, never an engine pass or a fallback
// to target/release, a Node child, cargo, build scripts or a downloaded executable.
const configured = process.env.RUNTIME_PACKAGE_ROOT;
const binaryOnly = { skip: configured ? false : 'BINARY NOT VERIFIED: set RUNTIME_PACKAGE_ROOT to a version root containing bin and _assets' };
let temporary, installed, copied, unrelated, noNodePath, source, descriptor, executableHash;
const observed = [];
before(() => {
  if (!configured) return;
  installed = resolve(configured);
  ({ source, descriptor, executableHash } = readInstalledPackage(installed));
  temporary = mkdtempSync(resolve(tmpdir(), 'calendar-runtime-acceptance-'));
  copied = copyInstalledPackage(installed, resolve(temporary, 'relocated'), descriptor.version);
  unrelated = resolve(temporary, 'unrelated-cwd');
  noNodePath = resolve(temporary, 'no-node-on-path');
  mkdirSync(unrelated);
  mkdirSync(noNodePath);
});
after(() => {
  try {
    if (configured && copied) observe('runtime-binary', { executable_sha256: executableHash, descriptor, results: observed }, resolve(copied, descriptor.path));
  } finally { if (temporary) rmSync(temporary, { recursive: true, force: true }); }
});
function run(root, request, options = {}) {
  return runBinary(root, request, { cwd: unrelated, emptyPath: noNodePath, ...options });
}
function fixtureById(id) {
  const fixture = corpus.find(item => item.id === id);
  assert.ok(fixture, `missing corpus ID ${id}`);
  return fixture;
}
function assertCase(root, fixture, options) {
  const input = { operation: fixture.operation, body: fixture.body };
  const node = createReference(resolve(copied, descriptor.path), source).call(input);
  assertFixture(fixture, node); // correctness FIRST; engine agreement is not the oracle
  const result = unwrapBusiness(run(root, hostEnvelope(fixture.operation, fixture.body), options));
  assertFixture(fixture, result);
  assert.deepEqual(result, node, `${fixture.id}: exact Node/QuickJS tree equality (no numeric epsilon)`);
  return result;
}
for (const fixture of corpus) test(`runtime installed binary: ${fixture.id}`, binaryOnly, () => {
  const output = assertCase(copied, fixture);
  observed.push({ id: fixture.id, output });
});
test('runtime installed binary: repeated business/failure/state sequence remains fresh per process', binaryOnly, () => {
  const byId = new Map();
  for (const id of stateSequence) {
    const output = assertCase(copied, fixtureById(id));
    if (byId.has(id)) assert.deepEqual(output, byId.get(id), id);
    byId.set(id, output);
  }
});
test('runtime installed binary: representative eight-operation and small-offset corpus is TZ-invariant', binaryOnly, () => {
  const ids = [
    'month-cny-2024', 'convert-solar-cny', 'almanac-cny', 'birth-known-leap-instant',
    'solar-beijing', 'bazi-cny', 'ziwei-cny', 'regions-root',
    'solar-previous-civil-day', 'solar-next-civil-day',
    'birth-positive-offset-previous-year', 'birth-negative-offset-next-year',
    ...[-15, -1, 1, 15].map(offset => `birth-small-minute-offset-${offset}`),
    ...[-15, -1, 1, 15].map(offset => `solar-small-minute-offset-${offset}`),
  ];
  const byZone = timezones.map(TZ => ({ TZ, outputs: ids.map(id => assertCase(copied, fixtureById(id), { TZ })) }));
  for (const zone of byZone.slice(1)) assert.deepEqual(zone.outputs, byZone[0].outputs, zone.TZ);
  observed.push({ id: 'binary-timezones', zones: byZone });
});

test('runtime installed binary: relocation retains version basename and is independent of cwd/Node', binaryOnly, () => {
  const root = copyInstalledPackage(installed, resolve(temporary, 'moved again', 'nested'), descriptor.version);
  const decoy = resolve(temporary, 'decoy-cwd');
  mkdirSync(resolve(decoy, '_assets/runtime'), { recursive: true });
  writeFileSync(resolve(decoy, '_assets/runtime/backend.js'), 'function main(){throw new Error("cwd asset must never run");}');
  const fixture = fixtureById('birth-known-leap-instant');
  const expected = assertCase(root, fixture, { cwd: unrelated });
  assert.deepEqual(assertCase(root, fixture, { cwd: root }), expected);
  assert.deepEqual(assertCase(root, fixture, { cwd: decoy }), expected);
});
test('runtime installed binary: wrong version basename fails deterministically', binaryOnly, () => {
  const root = copyInstalledPackage(installed, resolve(temporary, 'wrong-version'), `${descriptor.version}-wrong`);
  assertTransportError(run(root, hostEnvelope('regions.children', { parent_codes: [] })), 'artifact_version_mismatch');
});
test('runtime installed binary: executable outside bin is rejected', binaryOnly, () => {
  const executable = resolve(copied, binaryName);
  copyFileSync(resolve(copied, 'bin', binaryName), executable);
  chmodSync(executable, 0o755);
  assertTransportError(run(copied, hostEnvelope('regions.children', { parent_codes: [] }), { executable }), 'artifact_descriptor_invalid');
});
test('runtime installed binary: missing backend cannot fall back to cwd or installed original', binaryOnly, () => {
  const root = copyInstalledPackage(installed, resolve(temporary, 'missing-asset'), descriptor.version);
  rmSync(resolve(root, descriptor.path));
  assertTransportError(run(root, hostEnvelope('regions.children', { parent_codes: [] }), { cwd: installed }), 'artifact_missing');
});
test('runtime installed binary: backend is a bounded regular file', binaryOnly, () => {
  const root = copyInstalledPackage(installed, resolve(temporary, 'directory-asset'), descriptor.version);
  rmSync(resolve(root, descriptor.path));
  mkdirSync(resolve(root, descriptor.path));
  assertTransportError(run(root, hostEnvelope('regions.children', { parent_codes: [] })), 'artifact_invalid');
});
test('runtime installed binary: changed on-disk descriptor cannot override compiled descriptor', binaryOnly, () => {
  const root = copyInstalledPackage(installed, resolve(temporary, 'descriptor-not-authority'), descriptor.version);
  writeFileSync(resolve(root, '_assets/runtime/descriptor.json'), JSON.stringify({ ...descriptor, sha256: '0'.repeat(64) }));
  assertCase(root, fixtureById('birth-known-leap-instant'));
});
test('runtime installed binary: same-byte-length tamper fails even with a newly matching on-disk hash', binaryOnly, () => {
  const root = copyInstalledPackage(installed, resolve(temporary, 'tampered-asset'), descriptor.version);
  const bytes = readFileSync(resolve(root, descriptor.path));
  bytes[bytes.length - 1] = bytes[bytes.length - 1] === 32 ? 10 : 32;
  writeFileSync(resolve(root, descriptor.path), bytes);
  writeFileSync(resolve(root, '_assets/runtime/descriptor.json'), JSON.stringify({ ...descriptor, sha256: createHash('sha256').update(bytes).digest('hex') }));
  assertTransportError(run(root, hostEnvelope('regions.children', { parent_codes: [] })), 'artifact_integrity_mismatch');
});
test('runtime installed binary: symlink asset cannot escape installed version root', { ...binaryOnly, skip: binaryOnly.skip || (process.platform === 'win32' ? 'POSIX symlink fixture' : false) }, () => {
  const root = copyInstalledPackage(installed, resolve(temporary, 'escaped-asset'), descriptor.version);
  const outside = resolve(temporary, 'outside-backend.js');
  writeFileSync(outside, source);
  rmSync(resolve(root, descriptor.path));
  symlinkSync(outside, resolve(root, descriptor.path));
  assertTransportError(run(root, hostEnvelope('regions.children', { parent_codes: [] })), 'artifact_path_escape');
});

const request = () => hostEnvelope('birth.normalize', fixtureById('birth-known-leap-instant').body);
const protocolCases = [
  ['malformed JSON', () => '{', 'invalid_request', true],
  ['unknown method', () => ({ ...request(), method: 'not_a_method' }), 'invalid_request'],
  ['supported but unimplemented method', () => ({ ...request(), method: 'validate_config' }), 'unsupported_method'],
  ['missing handler', () => { const value = request(); delete value.input.handler; return value; }, 'invalid_request'],
  ['missing body', () => { const value = request(); delete value.input.input_payload.body; return value; }, 'invalid_request'],
  ['missing execution budget', () => { const value = request(); delete value.input.execution_budget; return value; }, 'execution_budget_missing'],
  ['malformed execution budget', () => { const value = request(); value.input.execution_budget.timeout_ms = -1; return value; }, 'execution_budget_missing'],
  ['host output budget', () => { const value = request(); value.input.execution_budget.max_output_bytes = 100; return value; }, 'output_limit_exceeded'],
];
for (const [name, make, code, raw = false] of protocolCases) test(`runtime installed binary transport: ${name}`, binaryOnly, () => {
  assertTransportError(run(copied, make(), { raw }), code);
});
test('runtime installed binary transport: canonical 1 MiB input boundary, not a private 64 KiB limit', binaryOnly, () => {
  const encoded = JSON.stringify(request());
  const exactFrame = encoded + ' '.repeat(FRAME_BYTES - Buffer.byteLength(encoded));
  assert.equal(Buffer.byteLength(exactFrame), FRAME_BYTES);
  const output = unwrapBusiness(run(copied, exactFrame, { raw: true }));
  assertFixture(fixtureById('birth-known-leap-instant'), output);
  assertTransportError(run(copied, exactFrame + ' ', { raw: true }), 'input_frame_exceeded');
});
