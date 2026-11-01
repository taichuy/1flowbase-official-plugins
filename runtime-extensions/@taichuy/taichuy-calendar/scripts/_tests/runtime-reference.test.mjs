import assert from 'node:assert/strict';
import test, { after } from 'node:test';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { operations } from '../runtime-contract.mjs';
import { corpus, operationNames, stateSequence, timezones } from '../runtime-tests/corpus.mjs';
import { assertFixture, createReference, defaultAsset, observe, pluginRoot } from '../runtime-tests/reference.mjs';
const source = readFileSync(defaultAsset, 'utf8');
const observed = [];

test('runtime corpus independently covers all eight operations: valid, boundary and invalid', () => {
  assert.deepEqual(operations.map(operation => operation.name).sort(), [...operationNames].sort());
  assert.equal(new Set(corpus.map(fixture => fixture.id)).size, corpus.length, 'unique case IDs');
  for (const operation of operationNames) {
    const cases = corpus.filter(fixture => fixture.operation === operation);
    for (const category of ['valid', 'boundary', 'invalid']) {
      assert.ok(cases.some(fixture => fixture.category === category), `${operation}: missing ${category}`);
    }
    assert.ok(cases.some(fixture => fixture.category === 'boundary' && fixture.expected.ok), `${operation}: successful boundary needed`);
    assert.ok(cases.every(fixture => fixture.expected.ok
      ? Object.keys(fixture.expected.exact ?? {}).length > 0 || fixture.expected.checks?.length > 0
      : fixture.expected.error.code && fixture.expected.error.message));
  }
});
for (const fixture of corpus) {
  test(`runtime Node fresh VM: ${fixture.id}`, () => {
    const output = createReference(defaultAsset, source).call({ operation: fixture.operation, body: fixture.body });
    observed.push({ id: fixture.id, output });
    assertFixture(fixture, output);
  });
}
test('runtime one reused VM: alternating operations, gender, offset and failure do not leak state', () => {
  const reused = createReference(defaultAsset, source);
  for (const id of stateSequence) {
    const fixture = corpus.find(candidate => candidate.id === id);
    assert.ok(fixture, id);
    const input = { operation: fixture.operation, body: fixture.body };
    const expected = createReference(defaultAsset, source).call(input);
    assertFixture(fixture, expected);
    const actual = reused.call(input);
    assertFixture(fixture, actual);
    assert.deepEqual(actual, expected, `${id}: reused VM differs from fresh VM`);
  }
});
test('runtime standalone wrapper is exactly TZ-invariant in three real Node subprocesses', () => {
  const byZone = timezones.map(TZ => {
    const result = spawnSync(process.execPath, [resolve(pluginRoot, 'scripts/runtime-tests/subprocess.mjs'), defaultAsset], {
      cwd: pluginRoot, env: { ...process.env, TZ }, encoding: 'utf8',
      timeout: 120_000, maxBuffer: 16 * 1024 * 1024,
    });
    assert.ifError(result.error);
    assert.equal(result.signal, null, TZ);
    assert.equal(result.status, 0, `${TZ}: ${result.stderr.slice(0, 3000)}`);
    const observations = JSON.parse(result.stdout);
    assert.equal(observations.length, corpus.length);
    observations.forEach((observation, index) => {
      assert.equal(observation.id, corpus[index].id);
      assertFixture(corpus[index], observation.output);
    });
    return { TZ, observations };
  });
  for (const zone of byZone.slice(1)) assert.deepEqual(zone.observations, byZone[0].observations, zone.TZ);
  observe('runtime-node-timezones', byZone);
});
after(() => observe('runtime-node-reference', observed));
