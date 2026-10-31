import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  buildSignedModelPricingCatalog,
  buildModelPricingCatalog,
  canonicalRulesBytes,
  discoverModelPricingRules,
  updateModelPricingCatalog,
  verifyModelPricingCatalog
} from '../model-pricing-catalog.mjs';

function catalog() {
  const rules = [];
  return {
    schema_version: '1flowbase.model-pricing/v2',
    catalog_version: '2026-08-17.1',
    generated_at: '2026-08-17T12:00:00Z',
    currency_code: 'USD',
    rules_checksum: `sha256:${crypto.createHash('sha256').update(JSON.stringify(rules)).digest('hex')}`,
    signature: null,
    rules
  };
}

test('checks the canonical rules checksum', () => {
  assert.equal(verifyModelPricingCatalog(catalog()), true);
  const invalid = catalog();
  invalid.rules_checksum = `sha256:${'0'.repeat(64)}`;
  assert.throws(() => verifyModelPricingCatalog(invalid), /checksum mismatch/);
});

test('builds an Ed25519 signature over canonical rule bytes', () => {
  const { privateKey, publicKey } = crypto.generateKeyPairSync('ed25519');
  const signed = buildSignedModelPricingCatalog(catalog(), privateKey, 'official-key');
  assert.equal(signed.signature.algorithm, 'ed25519');
  assert.equal(
    crypto.verify(
      null,
      canonicalRulesBytes(signed),
      publicKey,
      Buffer.from(signed.signature.signature, 'base64')
    ),
    true
  );
});

test('publishes unique standard configurations and the explicit zero fallback', () => {
  const rules = discoverModelPricingRules(path.resolve(import.meta.dirname, '../..'));
  assert.ok(rules.length > 0);
  assert.equal(new Set(rules.map(r => JSON.stringify([r.provider_code,r.upstream_model_id]))).size, rules.length);
  const inventoryPath = path.resolve(import.meta.dirname, '../../model-pricing/_maintenance/upstream-models.json');
  if (fs.existsSync(inventoryPath)) {
    const inventory = JSON.parse(fs.readFileSync(inventoryPath));
    const published = new Set(rules.map(r => JSON.stringify([r.provider_code, r.upstream_model_id])));
    for (const model of inventory.models.filter(m => m.status === 'synchronized')) {
      assert.ok(published.has(JSON.stringify([model.provider_code, model.upstream_model_id])), `missing synchronized model ${model.model}`);
    }
  }
  const zero = rules.find(r => r.provider_code === 'zero');
  for (const meter of ['input','output','cache_hit','cache_write']) assert.equal(zero[`${meter}_token_unit_price`], '0');
});

test('default publication batches contain at most twenty models without losing identities', () => {
  const published = buildModelPricingCatalog({ repoRoot: path.resolve(import.meta.dirname, '../..') });
  assert.equal(published.index.page_size, 20);
  assert.ok(published.pages.every(page => page.document.rules.length <= 20));
  const ids = published.pages.flatMap(page => page.document.rules.map(rule => rule.id));
  assert.equal(new Set(ids).size, published.index.total_rules);
  assert.deepEqual(ids, published.aggregate.rules.map(rule => rule.id));
  for (const page of published.pages) assert.equal(shaForTest(page.bytes), page.checksum);
});

function shaForTest(bytes) {
  return `sha256:${crypto.createHash('sha256').update(bytes).digest('hex')}`;
}

test('search shards cover every identity and page once and declare their exact providers', () => {
  const published = buildModelPricingCatalog({ repoRoot: path.resolve(import.meta.dirname, '../..') });
  const documents = published.shardDocuments.map(shard => JSON.parse(shard.bytes));
  assert.ok(documents.every(shard => shard.entries.length <= 100));
  assert.deepEqual(documents.flatMap(shard => shard.entries), published.search.entries);
  assert.deepEqual(published.index.search_shards.flatMap(shard => shard.pages), published.index.pages.map(page => page.page));
  for (const [index, shard] of published.shardDocuments.entries()) {
    assert.equal(shaForTest(shard.bytes), shard.reference.checksum);
    assert.deepEqual(shard.reference.provider_codes, [...new Set(documents[index].entries.map(entry => entry.provider_code))].sort());
  }
});

function historicalRules() {
  return JSON.parse(fs.readFileSync(new URL('./fixtures/model-pricing/historical-2026-10-11.json', import.meta.url)));
}

test('preserves the historical standard, context and TTL price contracts', () => {
  const rules = historicalRules();
  const astra = rules.find(r => r.upstream_model_id === 'gpt-6-astra');
  assert.equal(astra.id, '21000000-0000-4000-8000-000000000104');
  assert.equal(astra.priority, 0);
  assert.equal(astra.cache_write_token_unit_price, '12.5');
  assert.deepEqual(astra.rules, [{when:{input_tokens:{operator:'gt',value:272000}},overrides:{input_token_unit_price:'20',output_token_unit_price:'75',cache_hit_token_unit_price:'2',cache_write_token_unit_price:'25'}}]);
  for (const [model, base, large] of [
    ['gpt-6-sol', ['2', '10', '0.2', '2.5'], ['4', '15', '0.4', '5']],
    ['gpt-6.1-sol', ['2', '10', '0.1', '2.5'], ['4', '15', '0.2', '5']],
    ['gpt-6-luna', ['0.1', '0.5', '0.01', '0.125'], ['0.2', '0.75', '0.02', '0.25']],
  ]) {
    const rule = rules.find(r => r.upstream_model_id === model);
    assert.ok(rule, `${model} pricing exists`);
    assert.deepEqual(['input', 'output', 'cache_hit', 'cache_write'].map(m => rule[`${m}_token_unit_price`]), base);
    assert.deepEqual(rule.rules, [{
      when: { input_tokens: { operator: 'gt', value: 272000 } },
      overrides: Object.fromEntries(['input', 'output', 'cache_hit', 'cache_write'].map((m, i) => [`${m}_token_unit_price`, large[i]])),
    }]);
  }
  const fable = rules.find(r => r.upstream_model_id === 'claude-fable-5-1');
  assert.equal(fable.cache_write_token_unit_price, '12.5');
  assert.deepEqual(fable.rules, [{when:{cache_write_ttl_seconds:3600},overrides:{cache_write_token_unit_price:'20'}}]);
  for (const [model, base, hourWrite] of [
    ['claude-opus-5-5', ['4', '20', '0.2', '5'], '8'],
    ['claude-sonnet-5-5', ['2', '10', '0.1', '2.5'], '4'],
  ]) {
    const rule = rules.find(r => r.provider_code === 'anthropic' && r.upstream_model_id === model);
    assert.ok(rule, `${model} pricing exists`);
    for (const meter of ['input', 'output', 'cache_hit', 'cache_write']) {
      assert.equal(rule[`${meter}_token_unit_size`], 1_000_000);
    }
    assert.deepEqual(['input', 'output', 'cache_hit', 'cache_write'].map(m => rule[`${m}_token_unit_price`]), base);
    assert.deepEqual(rule.rules, [{
      when: { cache_write_ttl_seconds: 3600 },
      overrides: { cache_write_token_unit_price: hourWrite },
    }]);
  }
});

function sourceFixture() {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'model-pricing-catalog-'));
  fs.mkdirSync(path.join(repoRoot, 'model-pricing/@zeta/model-z'), { recursive: true });
  fs.mkdirSync(path.join(repoRoot, 'model-pricing/@alpha/model-a'), { recursive: true });
  fs.writeFileSync(path.join(repoRoot, 'model-pricing/catalog-source.json'), JSON.stringify({
    schema_version: '1flowbase.model-pricing-source/v2',
    catalog_version: '2026-08-18.1',
    currency_code: 'USD'
  }));
  const writeSource = (provider, modelKey, upstreamModelId, id) => {
    fs.writeFileSync(
      path.join(repoRoot, `model-pricing/@${provider}/${modelKey}/pricing.json`),
      JSON.stringify({
        schema_version: '1flowbase.model-pricing-source/v2',
        provider_code: provider,
        upstream_model_id: upstreamModelId,
        currency_code: 'USD',
        id,
        input_token_unit_size: 1_000_000,
        input_token_unit_price: '1',
        output_token_unit_size: 1_000_000,
        output_token_unit_price: '2',
        cache_hit_token_unit_size: 1_000_000,
        cache_hit_token_unit_price: '0.5',
        effective_from: '2026-08-18T00:00:00Z',
        effective_to: null,
        timezone: 'UTC',
        weekday_mask: 127,
        local_time_start: null,
        local_time_end: null,
        priority: 0,
        enabled: true,
        cache_write_token_unit_size: 1_000_000,
        cache_write_token_unit_price: '1',
        rules: [],
        extensions: {}
      })
    );
  };
  writeSource('zeta', 'model-z', 'vendor/model-z', '20000000-0000-4000-8000-000000000002');
  writeSource('alpha', 'model-a', 'model-a', '20000000-0000-4000-8000-000000000001');
  return repoRoot;
}

test('an empty catalog publishes no search shards and removes retired shard files', () => {
  const repoRoot = sourceFixture();
  try {
    updateModelPricingCatalog({ repoRoot });
    const searchRoot = path.join(repoRoot, 'model-pricing/catalog/v1/search');
    assert.ok(fs.readdirSync(searchRoot).length > 0);
    fs.rmSync(path.join(repoRoot, 'model-pricing/@alpha'), { recursive: true });
    fs.rmSync(path.join(repoRoot, 'model-pricing/@zeta'), { recursive: true });
    updateModelPricingCatalog({ repoRoot });
    const index = JSON.parse(fs.readFileSync(path.join(repoRoot, 'model-pricing/catalog/v1/index.json')));
    assert.equal(index.total_rules, 0);
    assert.deepEqual(index.search_shards, []);
    assert.deepEqual(fs.readdirSync(searchRoot), []);
    assert.equal(updateModelPricingCatalog({ repoRoot, check: true }).changed, false);
  } finally { fs.rmSync(repoRoot, { recursive: true, force: true }); }
});

test('AC-001 derives provider identity from @provider and keeps upstream model id in the source', () => {
  const repoRoot = sourceFixture();
  const rules = discoverModelPricingRules(repoRoot);
  assert.deepEqual(rules.map((rule) => [rule.provider_code, rule.upstream_model_id]), [
    ['alpha', 'model-a'],
    ['zeta', 'vendor/model-z']
  ]);
  const sourcePath = path.join(repoRoot, 'model-pricing/@alpha/model-a/pricing.json');
  const source = JSON.parse(fs.readFileSync(sourcePath, 'utf8'));
  source.provider_code = 'wrong';
  fs.writeFileSync(sourcePath, JSON.stringify(source));
  assert.throws(() => discoverModelPricingRules(repoRoot), /provider_code must match @alpha/);
});

test('AC-002 generates deterministic pages, search metadata, state, and aggregate snapshot', () => {
  const repoRoot = sourceFixture();
  const first = updateModelPricingCatalog({
    repoRoot,
    pageSize: 1,
    now: '2026-08-18T01:00:00Z'
  });
  assert.equal(first.pageCount, 2);
  const indexPath = path.join(repoRoot, 'model-pricing/catalog/v1/index.json');
  const firstIndex = fs.readFileSync(indexPath, 'utf8');
  const index = JSON.parse(firstIndex);
  const pageOne = JSON.parse(fs.readFileSync(path.join(repoRoot, 'model-pricing/catalog/v1/pages/1.json')));
  const pageTwo = JSON.parse(fs.readFileSync(path.join(repoRoot, 'model-pricing/catalog/v1/pages/2.json')));
  const search = JSON.parse(fs.readFileSync(path.join(repoRoot, 'model-pricing/catalog/v1/search-index.json')));
  assert.equal(index.total_rules, 2);
  assert.equal(pageOne.rules[0].provider_code, 'alpha');
  assert.equal(pageTwo.rules[0].provider_code, 'zeta');
  assert.equal(pageOne.next_cursor, pageTwo.cursor);
  assert.equal(search.entries[1].upstream_model_id, 'vendor/model-z');
  assert.equal(search.entries[1].catalog_page.page, 2);
  assert.equal(verifyModelPricingCatalog(JSON.parse(
    fs.readFileSync(path.join(repoRoot, 'model-pricing/catalog/v1/catalog.json'))
  )), true);
  const second = updateModelPricingCatalog({
    repoRoot,
    pageSize: 1,
    now: '2099-01-01T00:00:00Z'
  });
  assert.equal(second.changed, false);
  assert.equal(fs.readFileSync(indexPath, 'utf8'), firstIndex);
});

test('AC-002 check mode detects generated catalog drift without rewriting it', () => {
  const repoRoot = sourceFixture();
  updateModelPricingCatalog({ repoRoot, now: '2026-08-18T01:00:00Z' });
  const indexPath = path.join(repoRoot, 'model-pricing/catalog/v1/index.json');
  fs.writeFileSync(indexPath, '{}\n');
  assert.throws(() => updateModelPricingCatalog({ repoRoot, check: true }), /catalog drift/);
  assert.equal(fs.readFileSync(indexPath, 'utf8'), '{}\n');
});

test('AC-006 requires a new rule id when published pricing content changes', () => {
  const repoRoot = sourceFixture();
  updateModelPricingCatalog({ repoRoot, now: '2026-08-18T01:00:00Z' });
  const sourcePath = path.join(repoRoot, 'model-pricing/@alpha/model-a/pricing.json');
  const source = JSON.parse(fs.readFileSync(sourcePath, 'utf8'));
  source.input_token_unit_price = '9';
  fs.writeFileSync(sourcePath, JSON.stringify(source));
  assert.throws(
    () => updateModelPricingCatalog({ repoRoot, now: '2026-08-19T01:00:00Z' }),
    /is immutable; publish a new rule id/
  );
});

test('rejects old policies, malformed conditions, overrides and duplicate models', () => {
  const repoRoot = sourceFixture();
  const sourcePath = path.join(repoRoot, 'model-pricing/@alpha/model-a/pricing.json');
  const base = JSON.parse(fs.readFileSync(sourcePath));
  for (const mutate of [
    s => { s.rating_policy = {}; }, s => { delete s.rules; },
    s => { s.rules = [{when:{},overrides:{input_token_unit_price:'2'}}]; },
    s => { s.rules = [{when:{input_tokens:{operator:'lt',value:1}},overrides:{input_token_unit_price:'2'}}]; },
    s => { s.rules = [{when:{cache_write_ttl_seconds:300},overrides:{input_token_unit_price:'2'}}]; },
    s => { s.rules = [{when:{timezone:'Invalid/Zone'},overrides:{output_token_unit_price:'2'}}]; },
    s => { s.rules = [{when:{local_time_start:'01:00:00',local_time_end:'04:00:00'},overrides:{output_token_unit_price:'2'}}]; },
    s => { s.cache_write_token_unit_price = '79228162514264337593543950336'; },
    s => { s.rules = [{when:{weekday_mask:128},overrides:{output_token_unit_price:'2'}}]; },
    s => { s.rules = [{when:{input_tokens:{operator:'gt',value:1}},overrides:{},default:{}}]; },
  ]) {
    const source = structuredClone(base); mutate(source);
    fs.writeFileSync(sourcePath,JSON.stringify(source));
    assert.throws(() => discoverModelPricingRules(repoRoot), /invalid v2/);
  }
  fs.writeFileSync(sourcePath,JSON.stringify(base));
  const duplicatePath = path.join(repoRoot,'model-pricing/@alpha/duplicate');
  fs.mkdirSync(duplicatePath);
  fs.writeFileSync(path.join(duplicatePath,'pricing.json'),JSON.stringify({...base,id:'20000000-0000-4000-8000-000000000003'}));
  assert.throws(() => discoverModelPricingRules(repoRoot), /provider\/model configurations must be unique/);
  fs.rmSync(repoRoot,{recursive:true,force:true});
});


test('Sol and Haiku price whole requests at exact input boundaries and compose Haiku TTL rules', () => {
  const rules = historicalRules();
  const meters = ['input', 'output', 'cache_hit', 'cache_write'];
  const rates = (model, input, ttl) => {
    const source = rules.find(r => r.upstream_model_id === model);
    assert.ok(source);
    const prices = { ...source };
    for (const rule of source.rules) {
      const threshold = rule.when.input_tokens;
      if (threshold && !(threshold.operator === 'gt' ? input > threshold.value : input >= threshold.value)) continue;
      if (rule.when.cache_write_ttl_seconds !== undefined && rule.when.cache_write_ttl_seconds !== ttl) continue;
      Object.assign(prices, rule.overrides);
    }
    return meters.map(m => prices[m + '_token_unit_price']);
  };
  for (const [input, expected] of [
    [272000, ['2', '10', '0.1', '2.5']],
    [272001, ['4', '15', '0.2', '5']],
  ]) assert.deepEqual(rates('gpt-6.1-sol', input), expected);
  for (const [input, ttl, expected] of [
    [100000, undefined, ['0.1', '0.5', '0.01', '0.125']],
    [100000, 300, ['0.1', '0.5', '0.01', '0.125']],
    [100000, 3600, ['0.1', '0.5', '0.01', '0.2']],
    [100001, undefined, ['0.5', '2.5', '0.05', '0.625']],
    [100001, 300, ['0.5', '2.5', '0.05', '0.625']],
    [100001, 3600, ['0.5', '2.5', '0.05', '1']],
  ]) assert.deepEqual(rates('claude-haiku-5-5', input, ttl), expected);
  for (const [model, expected] of [
    ['gpt-6.1-sol', 0.30], ['claude-opus-5-5', 0.60],
    ['claude-sonnet-5-5', 0.30], ['claude-haiku-5-5', 0.015],
  ]) {
    const [inputPrice, outputPrice] = rates(model, 100000, 300).map(Number);
    assert.ok(Math.abs((100000 * inputPrice + 10000 * outputPrice) / 1000000 - expected) < 1e-12);
  }
});
