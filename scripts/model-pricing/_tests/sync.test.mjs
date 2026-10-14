import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { updateModelPricingCatalog, verifyModelPricingCatalog } from '../../model-pricing-catalog.mjs';
import { decimalPrice, fetchJson, loadUpstreamData, resolvePrices, syncModelPricing, MODELS_DEV_URL } from '../sync.mjs';

const NOW = '2026-10-14T09:17:00.000Z';
const price = { input: 3, output: 15, cache_read: 0.3, cache_write: 3.75 };
const fallback = { litellm_provider: 'anthropic', input_cost_per_token: 4e-6, output_cost_per_token: 20e-6, cache_read_input_token_cost: 0.4e-6, cache_creation_input_token_cost: 5e-6 };
const upstream = () => ({ modelsDev: { anthropic: { models: { 'test-model': { cost: { ...price } } } } }, liteLlm: { 'test-model': { ...fallback } } });

function fixture(t) {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'model-pricing-sync-'));
  t.after(() => fs.rmSync(repoRoot, { recursive: true, force: true }));
  const sourcePath = path.join(repoRoot, 'model-pricing/@anthropic/test-model/pricing.json');
  const source = {
    schema_version: '1flowbase.model-pricing-source/v2', id: crypto.randomUUID(),
    provider_code: 'anthropic', upstream_model_id: 'test-model', currency_code: 'USD',
    input_token_unit_size: 1_000_000, input_token_unit_price: '1',
    output_token_unit_size: 1_000_000, output_token_unit_price: '5',
    cache_hit_token_unit_size: 1_000_000, cache_hit_token_unit_price: '0.1',
    cache_write_token_unit_size: 1_000_000, cache_write_token_unit_price: '1.25',
    effective_from: '2026-10-01T00:00:00Z', effective_to: null, timezone: 'UTC',
    weekday_mask: 127, local_time_start: null, local_time_end: null, priority: 0, enabled: true,
    extensions: { source: 'vendor_published', verified_on: '2026-10-01', source_url: 'https://vendor.example/prices', custom: 'keep' },
    rules: [
      { when: { input_tokens: { operator: 'gt', value: 100000 } }, overrides: { input_token_unit_price: '2' } },
      { when: { cache_write_ttl_seconds: 3600 }, overrides: { cache_write_token_unit_price: '2' } },
      { when: { timezone: 'UTC', weekday_mask: 31, local_time_start: '01:00:00', local_time_end: '04:00:00' }, overrides: { output_token_unit_price: '7' } },
    ],
  };
  fs.mkdirSync(path.dirname(sourcePath), { recursive: true });
  fs.writeFileSync(sourcePath, JSON.stringify(source, null, 2) + '\n');
  fs.writeFileSync(path.join(repoRoot, 'model-pricing/catalog-source.json'), JSON.stringify({
    schema_version: '1flowbase.model-pricing-source/v2', catalog_version: '2026-10-14.1', currency_code: 'USD',
  }));
  updateModelPricingCatalog({ repoRoot, now: '2026-10-01T00:00:00Z' });
  return { repoRoot, source, sourcePath };
}

function snapshot(root) {
  const files = {};
  function walk(dir) {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const file = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(file);
      else files[path.relative(root, file)] = fs.readFileSync(file, 'utf8');
    }
  }
  walk(root);
  return files;
}

test('converts token and million-token prices exactly, including scientific notation and explicit zero', () => {
  assert.equal(decimalPrice(6.25e-7, 6), '0.625');
  assert.equal(decimalPrice(1e-8, 6), '0.01');
  assert.equal(decimalPrice('1.234567890123456789e-6', 6), '1.234567890123456789');
  assert.equal(decimalPrice('0001.2500'), '1.25');
  assert.equal(decimalPrice(0), '0');
  for (const value of [undefined, null, '', true, -1, '-0', Infinity, NaN, {}, '1e200', '0.0000000000000000001']) {
    assert.equal(decimalPrice(value), null, String(value));
  }
});

test('prefers Models.dev per meter and supplements only missing or invalid fields from the same serving provider', () => {
  const data = upstream();
  delete data.modelsDev.anthropic.models['test-model'].cost.cache_write;
  const source = { provider_code: 'anthropic', upstream_model_id: 'test-model' };
  const resolved = resolvePrices(source, data.modelsDev, data.liteLlm);
  assert.equal(resolved.rates.input_token_unit_price, '3');
  assert.equal(resolved.rates.cache_write_token_unit_price, '5');
  assert.equal(resolved.origins.input.source, 'models.dev');
  assert.equal(resolved.origins.cache_write.source, 'litellm');
  data.liteLlm['test-model'].litellm_provider = 'bedrock';
  assert.equal(resolvePrices(source, data.modelsDev, data.liteLlm).reason, 'missing-or-invalid-cache_write');
  data.modelsDev.anthropic.models['test-model'].cost.cache_write = 0;
  assert.equal(resolvePrices(source, data.modelsDev, data.liteLlm).rates.cache_write_token_unit_price, '0');
});

test('maps Kimi to international Moonshot and does not borrow Chinese-region or relay pricing', () => {
  const source = { provider_code: 'kimi', upstream_model_id: 'kimi-test' };
  const md = { moonshotai: { models: { 'kimi-test': { cost: price } } }, 'moonshotai-cn': { models: { 'kimi-test': { cost: { ...price, input: 99 } } } } };
  assert.equal(resolvePrices(source, md, {}).rates.input_token_unit_price, '3');
  delete md.moonshotai;
  assert.equal(resolvePrices(source, md, {}).reason, 'missing-or-invalid-input');
  assert.equal(resolvePrices({ provider_code: 'zero', upstream_model_id: 'any' }, md, {}).reason, 'unmapped-provider');
});

test('updates all four rates, publishes a new immutable ID/version, preserves rules and regenerates the catalog', t => {
  const f = fixture(t);
  const result = syncModelPricing({ ...f, ...upstream(), now: NOW });
  assert.deepEqual(result.updated, ['anthropic/test-model']);
  const updated = JSON.parse(fs.readFileSync(f.sourcePath));
  assert.notEqual(updated.id, f.source.id);
  assert.equal(updated.effective_from, NOW);
  assert.equal(updated.extensions.custom, 'keep');
  assert.equal(updated.extensions.source, 'community_pricing');
  assert.equal(updated.extensions.verified_on, undefined);
  assert.equal(updated.extensions.source_url, undefined);
  assert.equal(updated.extensions.price_sync.meters.input.url, MODELS_DEV_URL);
  assert.deepEqual(updated.rules, f.source.rules);
  assert.deepEqual(['input', 'output', 'cache_hit', 'cache_write'].map(m => updated[`${m}_token_unit_price`]), ['3', '15', '0.3', '3.75']);
  assert.equal(result.catalog_version, '2026-10-14.2');
  const aggregate = JSON.parse(fs.readFileSync(path.join(f.repoRoot, 'model-pricing/catalog/v1/catalog.json')));
  assert.equal(verifyModelPricingCatalog(aggregate), true);
  assert.equal(aggregate.rules[0].id, updated.id);
  const state = JSON.parse(fs.readFileSync(path.join(f.repoRoot, 'model-pricing/_maintenance/catalog-state.json')));
  assert.ok(state.rules[f.source.id], 'historical immutable ID is retained');
  assert.ok(state.rules[updated.id]);
  assert.equal(updateModelPricingCatalog({ repoRoot: f.repoRoot, check: true }).changed, false);
});

test('a repeated sync is byte-for-byte idle even with a new timestamp or changed provenance', t => {
  const f = fixture(t);
  const data = upstream();
  syncModelPricing({ ...f, ...data, now: NOW });
  const before = snapshot(f.repoRoot);
  assert.deepEqual(syncModelPricing({ ...f, ...data, now: '2026-10-15T01:00:00Z' }).updated, []);
  data.liteLlm['test-model'].input_cost_per_token = 0.1;
  assert.deepEqual(syncModelPricing({ ...f, ...data }).updated, []);
  assert.deepEqual(snapshot(f.repoRoot), before);
});

test('equivalent prices with another unit size do not create a revision', t => {
  const f = fixture(t);
  const source = { ...f.source, input_token_unit_size: 1000, input_token_unit_price: '0.001' };
  // Set up an unpublished fixture baseline with its own immutable state.
  fs.writeFileSync(f.sourcePath, JSON.stringify({ ...source, id: crypto.randomUUID() }));
  updateModelPricingCatalog({ repoRoot: f.repoRoot });
  const data = upstream();
  data.modelsDev.anthropic.models['test-model'].cost = { input: '1.00', output: 5, cache_read: 0.1, cache_write: 1.25 };
  const before = snapshot(f.repoRoot);
  assert.equal(syncModelPricing({ ...f, ...data }).unchanged, 1);
  assert.deepEqual(snapshot(f.repoRoot), before);
});

test('missing or invalid cache write skips the whole model without deletion or partial updates', t => {
  const f = fixture(t);
  const before = snapshot(f.repoRoot);
  for (const value of [undefined, null, -1, true, 'bad']) {
    const data = upstream();
    data.modelsDev.anthropic.models['test-model'].cost.cache_write = value;
    delete data.liteLlm['test-model'].cache_creation_input_token_cost;
    const result = syncModelPricing({ ...f, ...data });
    assert.deepEqual(result.updated, []);
    assert.equal(result.skipped[0].reason, 'missing-or-invalid-cache_write');
    assert.deepEqual(snapshot(f.repoRoot), before);
  }
});

test('does not add upstream-only models and allows explicit opt-out', t => {
  const f = fixture(t);
  const source = { ...f.source, id: crypto.randomUUID(), extensions: { price_sync: false } };
  fs.writeFileSync(f.sourcePath, JSON.stringify(source));
  updateModelPricingCatalog({ repoRoot: f.repoRoot });
  const data = upstream();
  data.modelsDev.anthropic.models['upstream-only'] = { cost: price };
  const before = snapshot(f.repoRoot);
  assert.equal(syncModelPricing({ ...f, ...data }).skipped[0].reason, 'disabled-or-opted-out');
  assert.deepEqual(snapshot(f.repoRoot), before);
});

test('missing Models.dev models can use a complete native LiteLLM entry', t => {
  const f = fixture(t);
  const data = upstream();
  data.modelsDev.anthropic.models = { other: { cost: price } };
  const result = syncModelPricing({ ...f, ...data });
  assert.equal(result.updated.length, 1);
  const updated = JSON.parse(fs.readFileSync(f.sourcePath));
  assert.deepEqual(['input', 'output', 'cache_hit', 'cache_write'].map(m => updated[`${m}_token_unit_price`]), ['4', '20', '0.4', '5']);
  assert.ok(Object.values(updated.extensions.price_sync.meters).every(m => m.source === 'litellm'));
});

test('future, expired and disabled configurations stay untouched', t => {
  const f = fixture(t);
  for (const patch of [{ effective_from: '2027-01-01T00:00:00Z' }, { effective_to: '2026-10-14T09:17:00Z' }, { enabled: false }]) {
    fs.writeFileSync(f.sourcePath, JSON.stringify({ ...f.source, ...patch, id: crypto.randomUUID() }));
    updateModelPricingCatalog({ repoRoot: f.repoRoot });
    const before = snapshot(f.repoRoot);
    assert.equal(syncModelPricing({ ...f, ...upstream(), now: NOW }).updated.length, 0);
    assert.deepEqual(snapshot(f.repoRoot), before);
  }
});

test('dry run returns planned updates and never writes prices, version, timestamps or catalog', t => {
  const f = fixture(t);
  const before = snapshot(f.repoRoot);
  assert.equal(syncModelPricing({ ...f, ...upstream(), now: NOW, dryRun: true }).updated.length, 1);
  assert.deepEqual(snapshot(f.repoRoot), before);
});

test('invalid upstream responses and existing catalog drift fail without writes', t => {
  const f = fixture(t);
  const before = snapshot(f.repoRoot);
  for (const patch of [{ modelsDev: {} }, { liteLlm: [] }, { modelsDev: { error: 'rate limited' } }]) {
    assert.throws(() => syncModelPricing({ ...f, ...upstream(), ...patch }), /empty or invalid/);
    assert.deepEqual(snapshot(f.repoRoot), before);
  }
  fs.writeFileSync(path.join(f.repoRoot, 'model-pricing/catalog/v1/index.json'), '{}');
  const drifted = snapshot(f.repoRoot);
  assert.throws(() => syncModelPricing({ ...f, ...upstream() }), /catalog drift/);
  assert.deepEqual(snapshot(f.repoRoot), drifted);
});

test('a later malformed source fails preflight before any other model is updated', t => {
  const f = fixture(t);
  const badFile = path.join(f.repoRoot, 'model-pricing/@anthropic/z-invalid/pricing.json');
  fs.mkdirSync(path.dirname(badFile), { recursive: true });
  fs.writeFileSync(badFile, JSON.stringify({ ...f.source, id: crypto.randomUUID(), upstream_model_id: 'bad', input_token_unit_price: '-1' }));
  const before = snapshot(f.repoRoot);
  assert.throws(() => syncModelPricing({ ...f, ...upstream() }), /invalid v2/);
  assert.deepEqual(snapshot(f.repoRoot), before);
});

test('a catalog write failure rolls back the pricing sources, version and generated files', t => {
  const f = fixture(t);
  const before = snapshot(f.repoRoot);
  const original = fs.writeFileSync;
  let failed = false;
  fs.writeFileSync = (file, ...args) => {
    if (!failed && String(file).endsWith('catalog/v1/index.json')) {
      failed = true;
      throw new Error('simulated disk failure');
    }
    return original(file, ...args);
  };
  try {
    assert.throws(() => syncModelPricing({ ...f, ...upstream() }), /simulated disk failure/);
  } finally {
    fs.writeFileSync = original;
  }
  assert.equal(failed, true);
  assert.deepEqual(snapshot(f.repoRoot), before);
});

test('network failures retry a bounded number of times and never turn into empty pricing snapshots', async () => {
  let calls = 0;
  await assert.rejects(fetchJson('https://example.invalid', {
    fetchImpl: async () => { calls++; return { ok: false, status: 503 }; }, attempts: 2, retryDelayMs: 0,
  }), /HTTP 503/);
  assert.equal(calls, 2);
  await assert.rejects(loadUpstreamData({ fetchImpl: async url => {
    if (url === MODELS_DEV_URL) return { ok: true, json: async () => upstream().modelsDev };
    throw new Error('network unavailable');
  }, attempts: 1 }), /network unavailable/);
});
