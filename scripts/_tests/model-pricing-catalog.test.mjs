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

test('publishes one provider-independent zero-cost fallback', () => {
  const repositoryRoot = path.resolve(import.meta.dirname, '../..');
  const published = JSON.parse(
    fs.readFileSync(
      path.join(repositoryRoot, 'model-pricing/catalog/v1/catalog.json'),
      'utf8'
    )
  );
  assert.equal(verifyModelPricingCatalog(published), true);
  assert.equal(published.rules.length, 1);
  const [rule] = published.rules;
  assert.equal(rule.provider_code, 'zero');
  assert.equal(rule.upstream_model_id, 'any');
  assert.equal(rule.input_token_unit_price, '0');
  assert.equal(rule.output_token_unit_price, '0');
  assert.equal(rule.cache_hit_token_unit_price, '0');
  assert.equal(rule.extensions.pricing_policy, 'global_zero_fallback');
});
