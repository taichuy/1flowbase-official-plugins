import assert from 'node:assert/strict';
import test from 'node:test';
import { runInNewContext } from 'node:vm';
import { corpus } from '../runtime-tests/corpus.mjs';
import { assertFixture, civilUTC, expectedUTC, finitePlainTree, gregorianMonthDays } from '../runtime-tests/reference.mjs';

test('runtime assertion walker accepts cross-realm JSON objects without JSON roundtrip', () => {
  const { value, realm } = runInNewContext('({value:{list:[1,null,true,"x"], n: -0},realm:{objectPrototype:Object.prototype,arrayPrototype:Array.prototype}})');
  const copied = finitePlainTree(value, realm);
  assert.deepEqual(copied.list, [1, null, true, 'x']);
  assert.ok(Object.is(copied.n, -0), 'copy must not JSON-normalize negative zero');
  assert.deepEqual(finitePlainTree(Object.assign(Object.create(null), { a: 1 })), { a: 1 });
});
const invalidTrees = [
  ['NaN', () => ({ value: NaN })],
  ['Infinity', () => ({ value: Infinity })],
  ['negative Infinity', () => ({ value: -Infinity })],
  ['undefined', () => ({ value: undefined })],
  ['function', () => ({ value() {} })],
  ['bigint', () => ({ value: 1n })],
  ['symbol value', () => ({ value: Symbol('x') })],
  ['symbol key', () => ({ [Symbol('x')]: 1 })],
  ['Date/toJSON', () => ({ value: new Date(0) })],
  ['Map', () => ({ value: new Map() })],
  ['class', () => new (class DTO { value = 1; })()],
  ['proxy', () => new Proxy({}, { ownKeys() { throw new Error('must not inspect proxy'); } })],
  ['cycle', () => { const value = {}; value.self = value; return value; }],
  ['array hole', () => [1, , 3]],
  ['array extra field', () => Object.assign([1], { extra: 2 })],
  ['hidden field', () => Object.defineProperty({}, 'hidden', { value: 1 })],
];
for (const [name, factory] of invalidTrees) test(`runtime assertion walker rejects ${name} before serialization`, () => {
  assert.throws(() => finitePlainTree(factory()));
});
test('runtime assertion walker never invokes getters or toJSON', () => {
  let calls = 0;
  assert.throws(() => finitePlainTree({ get value() { calls++; return 1; } }));
  assert.throws(() => finitePlainTree({ toJSON() { calls++; return {}; } }));
  assert.equal(calls, 0);
});
test('runtime independent Gregorian/offset oracles reject century and low-year normalization', () => {
  assert.deepEqual([1900, 2000, 2100].map(year => gregorianMonthDays(year, 2)), [28, 29, 28]);
  assert.throws(() => civilUTC('1900-02-29'));
  assert.throws(() => civilUTC('2100-02-29'));
  assert.equal(new Date(civilUTC('0001-01-01')).getUTCFullYear(), 1);
  assert.equal(expectedUTC({ civil_date: '2000-02-29', civil_time: '23:30:00', utc_offset_minutes: 480 }), '2000-02-29T15:30:00.000Z');
});
test('runtime independent oracle rejects equal-but-wrong birth UTC from either engine', () => {
  const fixture = corpus.find(item => item.id === 'birth-known-leap-instant');
  const wrong = { ok: true, data: {
    civil_date: '2000-02-29', civil_time: '23:30:00', utc_offset_minutes: 480,
    utc_datetime: '2000-02-29T23:30:00.000Z', gender: 'male',
    location: { codes: [], labels: [], longitude: null, latitude: null, coordinate_code: null },
  } };
  assert.throws(() => assertFixture(fixture, wrong), /utc_datetime/);
});
test('runtime solar oracle rejects a one-second mismatch, with no tolerance', () => {
  const fixture = corpus.find(item => item.id === 'solar-beijing');
  const output = { ok: true, data: {
    civil_date: '2024-02-10', civil_time: '11:00:00', utc_offset_minutes: 480,
    longitude: 116.4074, latitude: 39.9042, correction_seconds: -3600,
  } };
  assertFixture(fixture, output);
  output.data.correction_seconds++;
  assert.throws(() => assertFixture(fixture, output), /EXACTLY/);
});
