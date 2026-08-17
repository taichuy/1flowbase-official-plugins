import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';

import {
  buildSignedModelPricingCatalog,
  canonicalRulesBytes,
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

test('publishes a zero-cost default for every official static LLM model', () => {
  const repositoryRoot = path.resolve(import.meta.dirname, '../..');
  const published = JSON.parse(
    fs.readFileSync(
      path.join(repositoryRoot, 'model-pricing/catalog/v1/catalog.json'),
      'utf8'
    )
  );
  assert.equal(verifyModelPricingCatalog(published), true);
  const expected = [];
  const providersRoot = path.join(
    repositoryRoot,
    'runtime-extensions/@taichuy'
  );
  for (const providerCode of fs.readdirSync(providersRoot)) {
    const modelsRoot = path.join(providersRoot, providerCode, 'models/llm');
    if (!fs.existsSync(modelsRoot)) continue;
    for (const filename of fs.readdirSync(modelsRoot)) {
      if (!filename.endsWith('.yaml')) continue;
      const source = fs.readFileSync(path.join(modelsRoot, filename), 'utf8');
      const modelId = source.match(/^model:\s*(\S+)\s*$/m)?.[1];
      if (modelId) expected.push(`${providerCode}\0${modelId}`);
    }
  }
  const actual = new Map(
    published.rules.map((rule) => [
      `${rule.provider_code}\0${rule.upstream_model_id}`,
      rule
    ])
  );
  assert.deepEqual([...actual.keys()].sort(), expected.sort());
  for (const rule of actual.values()) {
    assert.equal(rule.input_token_unit_price, '0');
    assert.equal(rule.output_token_unit_price, '0');
    assert.equal(rule.cache_hit_token_unit_price, '0');
    assert.equal(rule.extensions.pricing_policy, 'official_zero_default');
  }
});
