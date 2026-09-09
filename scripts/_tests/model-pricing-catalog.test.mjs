import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  buildSignedModelPricingCatalog,
  canonicalRulesBytes,
  discoverModelPricingRules,
  updateModelPricingCatalog,
  verifyModelPricingCatalog
} from '../model-pricing-catalog.mjs';

function catalog() {
  const rules = [];
  return {
    schema_version: '1flowbase.model-pricing/v1',
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

test('publishes standard USD API prices with one provider-independent zero-cost fallback', () => {
  const repositoryRoot = path.resolve(import.meta.dirname, '../..');
  const published = JSON.parse(
    fs.readFileSync(
      path.join(repositoryRoot, 'model-pricing/catalog/v1/catalog.json'),
      'utf8'
    )
  );
  assert.equal(verifyModelPricingCatalog(published), true);
  assert.equal(published.rules.length, 34);
  const fallbackRules = published.rules.filter(
    (candidate) => candidate.provider_code === 'zero' && candidate.upstream_model_id === 'any'
  );
  assert.equal(fallbackRules.length, 1);
  const [rule] = fallbackRules;
  assert.equal(rule.provider_code, 'zero');
  assert.equal(rule.upstream_model_id, 'any');
  assert.equal(rule.input_token_unit_price, '0');
  assert.equal(rule.output_token_unit_price, '0');
  assert.equal(rule.cache_hit_token_unit_price, '0');
  assert.equal(rule.extensions.pricing_policy, 'global_zero_fallback');
  assert.equal(
    published.rules.filter((candidate) => candidate.rating_policy_enabled).length,
    9
  );
  assert.equal(
    published.rules.filter((candidate) => candidate.provider_code === 'deepseek').length,
    10
  );
  assert.equal(
    published.rules.some((candidate) => candidate.upstream_model_id === 'glm-5.3'),
    false
  );
  const astra = published.rules.find(
    (candidate) => candidate.provider_code === 'openai' && candidate.upstream_model_id === 'gpt-6-astra'
  );
  assert.deepEqual(
    {
      input: astra?.input_token_unit_price,
      cache_hit: astra?.cache_hit_token_unit_price,
      output: astra?.output_token_unit_price,
      long_context: astra?.rating_policy?.tiers?.[0]?.rates
    },
    {
      input: '10',
      cache_hit: '1',
      output: '50',
      long_context: {
        input: { unit_size: 1000000, unit_price: '20' },
        output: { unit_size: 1000000, unit_price: '75' },
        cache_hit: { unit_size: 1000000, unit_price: '2' }
      }
    }
  );
  const fable = published.rules.find(
    (candidate) => candidate.provider_code === 'anthropic' && candidate.upstream_model_id === 'claude-fable-5-1'
  );
  assert.deepEqual(
    {
      input: fable?.input_token_unit_price,
      cache_hit: fable?.cache_hit_token_unit_price,
      output: fable?.output_token_unit_price,
      rating_policy_enabled: fable?.rating_policy_enabled
    },
    { input: '10', cache_hit: '0.25', output: '50', rating_policy_enabled: false }
  );
});

function sourceFixture() {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'model-pricing-catalog-'));
  fs.mkdirSync(path.join(repoRoot, 'model-pricing/@zeta/model-z'), { recursive: true });
  fs.mkdirSync(path.join(repoRoot, 'model-pricing/@alpha/model-a'), { recursive: true });
  fs.writeFileSync(path.join(repoRoot, 'model-pricing/catalog-source.json'), JSON.stringify({
    schema_version: '1flowbase.model-pricing-source/v1',
    catalog_version: '2026-08-18.1',
    currency_code: 'USD'
  }));
  const writeSource = (provider, modelKey, upstreamModelId, id) => {
    fs.writeFileSync(
      path.join(repoRoot, `model-pricing/@${provider}/${modelKey}/pricing.json`),
      JSON.stringify({
        schema_version: '1flowbase.model-pricing-source/v1',
        provider_code: provider,
        upstream_model_id: upstreamModelId,
        currency_code: 'USD',
        rules: [{
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
          rating_policy_enabled: false,
          rating_policy: {},
          extensions: {}
        }]
      })
    );
  };
  writeSource('zeta', 'model-z', 'vendor/model-z', '20000000-0000-4000-8000-000000000002');
  writeSource('alpha', 'model-a', 'model-a', '20000000-0000-4000-8000-000000000001');
  return repoRoot;
}

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
  source.rules[0].input_token_unit_price = '9';
  fs.writeFileSync(sourcePath, JSON.stringify(source));
  assert.throws(
    () => updateModelPricingCatalog({ repoRoot, now: '2026-08-19T01:00:00Z' }),
    /is immutable; publish a new rule id/
  );
});

test('rejects unsupported executable rating policies at the catalog boundary', () => {
  const repoRoot = sourceFixture();
  const sourcePath = path.join(repoRoot, 'model-pricing/@alpha/model-a/pricing.json');
  const source = JSON.parse(fs.readFileSync(sourcePath, 'utf8'));
  source.rules[0].rating_policy_enabled = true;
  source.rules[0].rating_policy = {
    schema_version: '1flowbase.model-rating-policy/v1',
    type: 'arbitrary_expression',
    expression: 'input_tokens * 2'
  };
  fs.writeFileSync(sourcePath, JSON.stringify(source));
  assert.throws(() => discoverModelPricingRules(repoRoot), /unsupported rating policy/);
});

// AC1/AC6: exercise the real publisher boundary, including immutable history.
test('v2 catalog revisions preserve old rules and publish complete token pricing', () => {
  const rules = discoverModelPricingRules(path.resolve(import.meta.dirname, '../..'));
  const latest = (model) => rules.filter((rule) => rule.upstream_model_id === model)
    .sort((a, b) => b.priority - a.priority || b.effective_from.localeCompare(a.effective_from));
  const astra = latest('gpt-6-astra');
  assert.equal(astra.length, 2);
  assert.equal(astra[0].effective_from, '2026-09-09T00:00:00Z');
  assert.equal(astra[1].rating_policy.schema_version, '1flowbase.model-rating-policy/v1');
  assert.deepEqual(astra[0].rating_policy.rates, {
    input: '10', output: '50', cache_hit: '1', cache_write: { unit_price: '12.5' }
  });
  assert.deepEqual(astra[0].rating_policy.input_token_tiers, [{
    when: { operator: 'gt', value: 272000 },
    rates: { input: '20', output: '75', cache_hit: '2', cache_write: { unit_price: '25' } }
  }]);
  const fable = latest('claude-fable-5-1');
  assert.equal(fable.length, 2);
  assert.deepEqual(fable[0].rating_policy.rates, {
    input: '10', output: '50', cache_hit: '0.25',
    cache_write: { by_ttl_seconds: { '300': '12.5', '3600': '20' } }
  });
  assert.equal(fable[1].rating_policy_enabled, false);
  assert.equal(latest('claude-fable-5').length, 1);
});

test('AC1 rejects malformed v2 policies and accepts complete ascending replacement tiers', () => {
  const repoRoot = sourceFixture();
  const sourcePath = path.join(repoRoot, 'model-pricing/@alpha/model-a/pricing.json');
  const source = JSON.parse(fs.readFileSync(sourcePath, 'utf8'));
  const rates = { input: '10', output: '50', cache_hit: '1', cache_write: { unit_price: '12.5' } };
  const policy = { schema_version: '1flowbase.model-rating-policy/v2', type: 'token_pricing', unit_size: 1000000,
    rates, input_token_tiers: [{ when: { operator: 'gt', value: 272000 }, rates },
      { when: { operator: 'gte', value: 500000 }, rates }] };
  const publish = (candidate) => {
    source.rules[0].rating_policy_enabled = true;
    source.rules[0].rating_policy = candidate;
    fs.writeFileSync(sourcePath, JSON.stringify(source));
    return discoverModelPricingRules(repoRoot);
  };
  assert.doesNotThrow(() => publish(policy));
  const invalid = [
    p => { p.unknown = 1; }, p => { p.unit_size = 0; }, p => { p.unit_size = 1.5; },
    p => { p.rates.input = 10; }, p => { p.rates.output = '-1'; },
    p => { p.rates.cache_hit = '1e2'; }, p => { delete p.rates.cache_write; },
    p => { p.rates.cache_write.by_ttl_seconds = { '300': '1' }; },
    p => { p.rates.cache_write = { by_ttl_seconds: {} }; },
    p => { p.rates.cache_write = { by_ttl_seconds: { '0': '1' } }; },
    p => { p.rates.cache_write = { by_ttl_seconds: { '0300': '1' } }; },
    p => { p.rates.cache_write = { by_ttl_seconds: { '300': 'NaN' } }; },
    p => { p.input_token_tiers[1].when.value = 272000; },
    p => { p.input_token_tiers[0].when.operator = 'lt'; },
    p => { delete p.input_token_tiers[0].rates.output; },
    p => { p.input_token_tiers[0].when.extra = true; },
    p => { p.input_token_tiers = []; }
  ];
  for (const mutate of invalid) {
    const candidate = structuredClone(policy);
    mutate(candidate);
    assert.throws(() => publish(candidate), /invalid v2 rating policy/);
  }
  fs.rmSync(repoRoot, { recursive: true, force: true });
});
