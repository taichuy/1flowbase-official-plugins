import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const MODEL_PRICING_SCHEMA_VERSION = '1flowbase.model-pricing/v1';

function sha256(bytes) {
  return `sha256:${crypto.createHash('sha256').update(bytes).digest('hex')}`;
}

export function canonicalRulesBytes(catalog) {
  return Buffer.from(JSON.stringify(catalog.rules));
}

export function verifyModelPricingCatalog(catalog) {
  if (catalog?.schema_version !== MODEL_PRICING_SCHEMA_VERSION) {
    throw new Error('unsupported model pricing catalog schema');
  }
  if (catalog.currency_code !== 'USD' || !Array.isArray(catalog.rules)) {
    throw new Error('model pricing catalog must contain USD rules');
  }
  const actual = sha256(canonicalRulesBytes(catalog));
  if (catalog.rules_checksum !== actual) {
    throw new Error('model pricing rules checksum mismatch');
  }
  const ids = new Set();
  for (const rule of catalog.rules) {
    if (!rule.id || ids.has(rule.id)) throw new Error('model pricing rule ids must be unique');
    ids.add(rule.id);
    if (rule.currency_code !== 'USD' || rule.source_kind !== 'official') {
      throw new Error('official model pricing rules must use USD and source_kind=official');
    }
  }
  return true;
}

export function buildSignedModelPricingCatalog(catalog, privateKey, keyId) {
  verifyModelPricingCatalog(catalog);
  if (privateKey.asymmetricKeyType !== 'ed25519') {
    throw new Error('model pricing signing key must be Ed25519');
  }
  const bytes = canonicalRulesBytes(catalog);
  return {
    ...catalog,
    signature: {
      algorithm: 'ed25519',
      key_id: keyId,
      signature: crypto.sign(null, bytes, privateKey).toString('base64')
    }
  };
}

function parseCli(argv) {
  const options = { check: false };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--check') options.check = true;
    else if (argument === '--private-key-pem-file') options.privateKeyPath = path.resolve(argv[++index]);
    else if (argument === '--key-id') options.keyId = argv[++index];
    else if (argument === '--output') options.output = path.resolve(argv[++index]);
    else throw new Error('usage: model-pricing-catalog.mjs --check | --private-key-pem-file PATH --key-id ID --output PATH');
  }
  return options;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const options = parseCli(process.argv.slice(2));
  const repositoryRoot = path.resolve(import.meta.dirname, '..');
  const sourcePath = path.join(repositoryRoot, 'model-pricing', 'catalog', 'v1', 'catalog.json');
  const catalog = JSON.parse(fs.readFileSync(sourcePath, 'utf8'));
  verifyModelPricingCatalog(catalog);
  if (!options.check) {
    if (!options.privateKeyPath || !options.keyId || !options.output) {
      throw new Error('signing requires private key, key id, and output');
    }
    const privateKey = crypto.createPrivateKey(fs.readFileSync(options.privateKeyPath, 'utf8'));
    const signed = buildSignedModelPricingCatalog(catalog, privateKey, options.keyId);
    fs.writeFileSync(options.output, `${JSON.stringify(signed, null, 2)}\n`);
  }
}
