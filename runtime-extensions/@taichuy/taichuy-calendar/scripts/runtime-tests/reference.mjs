import assert from 'node:assert/strict';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { types } from 'node:util';
import { Script, createContext } from 'node:vm';
import { dataSchemas } from '../runtime-schemas.mjs';

export const pluginRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
export const defaultAsset = resolve(pluginRoot, '_assets/runtime/backend.js');
export const VM_TIMEOUT_MS = 30_000;

/** No JSON.stringify/parse "sanitizer": validate raw values before copying them.
 * Trusted prototypes are captured BEFORE bundle evaluation; accept cross-realm
 * plain objects/arrays, not constructor-name spoofing, classes, dates or proxies.
 */
export function finitePlainTree(value, realm = {}, path = '$', ancestors = new Set()) {
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return value;
  if (typeof value === 'number') {
    assert.ok(Number.isFinite(value), `${path}: non-finite number`);
    return value;
  }
  assert.equal(typeof value, 'object', `${path}: not JSON data`);
  assert.ok(!types.isProxy(value), `${path}: proxy is not a plain tree`);
  assert.ok(!ancestors.has(value), `${path}: cycle`);
  const array = Array.isArray(value);
  const proto = Object.getPrototypeOf(value);
  assert.ok(array
    ? proto === Array.prototype || proto === realm.arrayPrototype
    : proto === null || proto === Object.prototype || proto === realm.objectPrototype,
  `${path}: non-plain prototype`);
  ancestors.add(value);
  const descriptors = Object.getOwnPropertyDescriptors(value);
  const keys = Reflect.ownKeys(descriptors);
  const output = array ? [] : {};
  for (const key of keys) {
    assert.equal(typeof key, 'string', `${path}: symbol key`);
    const descriptor = descriptors[key];
    assert.ok(Object.hasOwn(descriptor, 'value'), `${path}.${key}: accessor`);
    if (array && key === 'length') continue;
    assert.equal(descriptor.enumerable, true, `${path}.${key}: non-enumerable data`);
    if (array) assert.ok(/^(0|[1-9][0-9]*)$/.test(key) && Number(key) < value.length, `${path}.${key}: array extra key`);
    const child = finitePlainTree(descriptor.value, realm, `${path}.${key}`, ancestors);
    // defineProperty retains an own __proto__ key without mutating our prototype.
    Object.defineProperty(output, key, { value: child, enumerable: true, writable: true, configurable: true });
  }
  if (array) assert.equal(keys.length - 1, value.length, `${path}: sparse array`);
  ancestors.delete(value);
  return output;
}

/** Standalone same wrapper as QuickJS: no Node globals, browser, timers or deps. */
export function createReference(assetPath = defaultAsset, source = readFileSync(assetPath, 'utf8')) {
  const context = createContext(Object.create(null), { codeGeneration: { strings: false, wasm: false } });
  const realm = new Script('({objectPrototype: Object.prototype, arrayPrototype: Array.prototype})').runInContext(context);
  new Script(source, { filename: assetPath }).runInContext(context, { timeout: VM_TIMEOUT_MS });
  assert.equal(new Script('typeof main').runInContext(context), 'function', 'bundle must expose main');
  return {
    call(input) {
      // Validated JSON-native test input crosses the boundary as a value, not source.
      context.__fixtureInput = finitePlainTree(input);
      try {
        const raw = new Script('main(__fixtureInput)').runInContext(context, { timeout: VM_TIMEOUT_MS });
        return finitePlainTree(raw, realm);
      } finally { delete context.__fixtureInput; }
    },
  };
}

/** Independent civil arithmetic, not Dayjs and never host-local Date parsing. */
export function civilUTC(date, time = '00:00:00') {
  const dateParts = /^([+-]?\d{4,6})-(\d{2})-(\d{2})$/.exec(date);
  const timeParts = /^(\d{2}):(\d{2}):(\d{2})$/.exec(time);
  assert.ok(dateParts && timeParts, `invalid oracle civil input ${date} ${time}`);
  const [year, month, day] = dateParts.slice(1).map(Number);
  const [hour, minute, second] = timeParts.slice(1).map(Number);
  const instant = new Date(0);
  instant.setUTCFullYear(year, month - 1, day);
  instant.setUTCHours(hour, minute, second, 0);
  assert.deepEqual([instant.getUTCFullYear(), instant.getUTCMonth() + 1, instant.getUTCDate(), instant.getUTCHours(), instant.getUTCMinutes(), instant.getUTCSeconds()], [year, month, day, hour, minute, second], 'oracle must not normalize impossible dates');
  return instant.getTime();
}
export function expectedUTC(body) {
  return new Date(civilUTC(body.civil_date, body.civil_time) - body.utc_offset_minutes * 60_000).toISOString();
}
export function gregorianMonthDays(year, month) {
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  return month === 2 ? (leap ? 29 : 28) : ([4, 6, 9, 11].includes(month) ? 30 : 31);
}

// A small assertion-only JSON-schema reader. Public schemas constrain shape only;
// they NEVER provide expected business values and never transform the output.
export function assertSchema(value, schema, path = '$') {
  if (schema.anyOf || schema.oneOf) {
    const matches = (schema.anyOf ?? schema.oneOf).filter(candidate => {
      try { assertSchema(value, candidate, path); return true; } catch { return false; }
    }).length;
    assert.ok(schema.oneOf ? matches === 1 : matches >= 1, `${path}: schema union mismatch`);
    return;
  }
  if (Object.hasOwn(schema, 'const')) assert.deepEqual(value, schema.const, path);
  if (schema.enum) assert.ok(schema.enum.includes(value), `${path}: enum mismatch`);
  if (schema.type === 'null') assert.equal(value, null, path);
  else if (schema.type === 'array') {
    assert.ok(Array.isArray(value), `${path}: expected array`);
    value.forEach((child, index) => assertSchema(child, schema.items, `${path}/${index}`));
  } else if (schema.type === 'object') {
    assert.ok(value !== null && typeof value === 'object' && !Array.isArray(value), `${path}: expected object`);
    for (const key of schema.required ?? []) assert.ok(Object.hasOwn(value, key), `${path}/${key}: required`);
    for (const key of Object.keys(value)) {
      const child = schema.properties?.[key] ?? schema.additionalProperties;
      assert.ok(child && child !== false, `${path}/${key}: unexpected field`);
      if (child !== true) assertSchema(value[key], child, `${path}/${key}`);
    }
  } else if (schema.type === 'integer') assert.ok(Number.isSafeInteger(value), `${path}: expected safe integer`);
  else if (schema.type === 'number') assert.ok(typeof value === 'number' && Number.isFinite(value), `${path}: expected finite number`);
  else if (schema.type) assert.equal(typeof value, schema.type, path);
  if (schema.pattern) assert.match(value, new RegExp(schema.pattern), path);
}
function pointer(value, path) {
  return path.split('/').slice(1).reduce((parent, key) => {
    assert.ok(parent !== null && parent !== undefined, `missing pointer ${path}`);
    key = key.replaceAll('~1', '/').replaceAll('~0', '~');
    assert.ok(Object.hasOwn(parent, key), `missing pointer ${path}`);
    return parent[key];
  }, value);
}
function assertBirth(data, body) {
  assert.equal(data.utc_datetime, expectedUTC(body), 'UTC must equal independent fixed-offset arithmetic');
  for (const key of ['civil_date', 'civil_time', 'utc_offset_minutes', 'gender']) assert.deepEqual(data[key], body[key], key);
  assert.deepEqual(data.location.codes, body.location_codes);
}
export function assertFixture(fixture, output) {
  finitePlainTree(output);
  assert.equal(output.ok, fixture.expected.ok, `${fixture.id}: business status`);
  if (!fixture.expected.ok) {
    assert.deepEqual(output, { ok: false, error: fixture.expected.error }, `${fixture.id}: exact public error`);
    return;
  }
  assert.deepEqual(Object.keys(output).sort(), ['data', 'ok'], fixture.id);
  const { data } = output;
  assertSchema(data, dataSchemas[fixture.operation]);
  for (const [path, expected] of Object.entries(fixture.expected.exact ?? {})) assert.deepEqual(pointer(data, path), expected, `${fixture.id}${path}`);
  for (const check of fixture.expected.checks ?? []) {
    if (check === 'birth-utc') assertBirth(data, fixture.body);
    else if (check === 'chart-birth-utc') assertBirth(data.birth, fixture.body);
    else if (check === 'gregorian-month') {
      const { year, month } = fixture.body;
      assert.equal(data.year, year);
      assert.equal(data.month, month);
      assert.equal(data.days.length, gregorianMonthDays(year, month));
      assert.equal(data.first_weekday, new Date(civilUTC(`${year}-${String(month).padStart(2, '0')}-01`)).getUTCDay());
      data.days.forEach((day, index) => {
        const date = `${year}-${String(month).padStart(2, '0')}-${String(index + 1).padStart(2, '0')}`;
        assert.equal(day.civil_date, date);
        assert.equal(day.weekday, new Date(civilUTC(date)).getUTCDay());
      });
    } else if (check === 'almanac-hours') {
      assert.deepEqual(data.time_luck.map(time => [time.branch, time.hour]), [...'子丑寅卯辰巳午未申酉戌亥'].map((branch, index) => [branch, index * 2]));
    } else if (check === 'solar-seconds') {
      const seconds = (civilUTC(data.civil_date, data.civil_time) - civilUTC(fixture.body.civil_date, fixture.body.civil_time)) / 1000;
      assert.ok(Number.isSafeInteger(seconds));
      assert.equal(data.correction_seconds, seconds, 'true solar correction must EXACTLY equal independent output civil minus input civil');
      for (const key of ['utc_offset_minutes', 'longitude', 'latitude']) assert.equal(data[key], fixture.body[key]);
    } else if (check === 'bazi-tree') {
      assert.equal(data.daYunDetails.length, 8);
      for (const decade of data.daYunDetails) {
        assert.equal(decade.years.length, 10);
        assert.equal(decade.endYear - decade.startYear, 9);
        // tyme LunarYear.getMonths() includes the leap month (12 OR 13).
        // The complete month values/count still compare exactly across engines.
        decade.years.forEach(year => {
          assert.ok(year.months.length === 12 || year.months.length === 13);
          assert.equal(new Set(year.months.map(month => month.month)).size, year.months.length);
        });
      }
      for (const pillar of Object.values(data.eightChar)) assert.equal(pillar.gan_zhi, pillar.heavenly_stem + pillar.earthly_branch);
      assert.equal(data.dayMaster, data.eightChar.day.heavenly_stem);
    } else if (check === 'ziwei-tree') {
      assert.equal(data.palaces.length, 12);
      assert.equal(new Set(data.palaces.map(palace => palace.name)).size, 12);
      assert.deepEqual(data.palaces.map(palace => palace.index).sort((a, b) => a - b), Array.from({ length: 12 }, (_, index) => index));
      assert.equal(data.horoscope.as_of, fixture.body.as_of);
      assert.equal(data.horoscope.as_of_time, fixture.body.as_of_time);
      assert.equal(data.palaces.filter(palace => palace.isBodyPalace).length, 1);
    } else assert.fail(`Unknown oracle ${check}`);
  }
}

export function observe(name, observations, assetPath = defaultAsset) {
  if (!process.env.TEST_EVIDENCE_DIR) return;
  const directory = resolve(process.env.TEST_EVIDENCE_DIR);
  mkdirSync(directory, { recursive: true });
  writeFileSync(resolve(directory, `${name}.json`), JSON.stringify({
    asset_sha256: createHash('sha256').update(readFileSync(assetPath)).digest('hex'),
    node: process.version, timezone: process.env.TZ ?? null, observations,
  }, null, 2) + '\n');
}
