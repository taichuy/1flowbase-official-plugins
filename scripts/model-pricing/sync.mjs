import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { updateModelPricingCatalog } from '../model-pricing-catalog.mjs';
import { METERS, validatePricingConfiguration } from '../model-pricing-validation.mjs';

export const MODELS_DEV_URL = 'https://models.dev/api.json';
export const LITELLM_URL = 'https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json';

// Map serving providers explicitly; never search other providers by model name.
const PROVIDERS = {
  anthropic: { modelsDev: 'anthropic', liteLlm: 'anthropic' },
  openai: { modelsDev: 'openai', liteLlm: 'openai' },
  google: { modelsDev: 'google', liteLlm: 'gemini' },
  deepseek: { modelsDev: 'deepseek', liteLlm: 'deepseek' },
  xai: { modelsDev: 'xai', liteLlm: 'xai' },
  zai: { modelsDev: 'zai', liteLlm: 'z_ai' },
  kimi: { modelsDev: 'moonshotai', liteLlm: 'moonshot' },
};
const FIELDS = {
  input: ['input', 'input_cost_per_token'],
  output: ['output', 'output_cost_per_token'],
  cache_hit: ['cache_read', 'cache_read_input_token_cost'],
  cache_write: ['cache_write', 'cache_creation_input_token_cost'],
};
const json = value => `${JSON.stringify(value, null, 2)}\n`;
const readJson = file => JSON.parse(fs.readFileSync(file, 'utf8'));

// Shift decimal digits instead of multiplying IEEE-754 floats. Emit rust_decimal-compatible strings.
export function decimalPrice(value, shift = 0) {
  if ((typeof value !== 'number' && typeof value !== 'string') ||
      (typeof value === 'number' && !Number.isFinite(value))) return null;
  const match = /^(\d+)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/.exec(String(value));
  if (!match) return null;
  const [, integer, fraction = '', exponent = '0'] = match;
  const power = Number(exponent) + shift;
  if (!Number.isSafeInteger(power) || Math.abs(power) > 100) return null;
  const digits = integer + fraction;
  const point = integer.length + power;
  const expanded = point <= 0 ? `0.${'0'.repeat(-point)}${digits}`
    : point >= digits.length ? digits + '0'.repeat(point - digits.length)
      : `${digits.slice(0, point)}.${digits.slice(point)}`;
  const [whole, decimal = ''] = expanded.split('.');
  const normalizedWhole = whole.replace(/^0+(?=\d)/, '');
  const normalizedDecimal = decimal.replace(/0+$/, '');
  const result = normalizedWhole + (normalizedDecimal ? `.${normalizedDecimal}` : '');
  if (normalizedDecimal.length > 18 || BigInt(result.replace('.', '')) > 79228162514264337593543950335n) return null;
  return result;
}

function object(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

export function validateUpstreamData(modelsDev, liteLlm) {
  if (!object(modelsDev) || !Object.values(modelsDev).some(provider =>
    object(provider?.models) && Object.keys(provider.models).length > 0)) {
    throw new Error('Models.dev returned an empty or invalid provider catalog');
  }
  if (!object(liteLlm) || !Object.entries(liteLlm).some(([key, model]) =>
    key !== 'sample_spec' && object(model) && typeof model.litellm_provider === 'string')) {
    throw new Error('LiteLLM returned an empty or invalid model catalog');
  }
}

export function resolvePrices(source, modelsDev, liteLlm) {
  const provider = PROVIDERS[source.provider_code];
  if (!provider) return { reason: 'unmapped-provider' };
  const modelId = source.upstream_model_id;
  const cost = modelsDev[provider.modelsDev]?.models?.[modelId]?.cost;
  // Accept only the native ID or provider-prefixed ID AND an exact provider tag.
  const fallback = [liteLlm[`${provider.liteLlm}/${modelId}`], liteLlm[modelId]]
    .find(model => model?.litellm_provider === provider.liteLlm);
  const rates = {};
  const origins = {};
  for (const meter of METERS) {
    const [modelsField, liteField] = FIELDS[meter];
    let price = decimalPrice(cost?.[modelsField]);
    let origin = { source: 'models.dev', provider: provider.modelsDev, model: modelId, field: `cost.${modelsField}`, url: MODELS_DEV_URL };
    if (price === null) {
      price = decimalPrice(fallback?.[liteField], 6);
      origin = { source: 'litellm', provider: provider.liteLlm, model: modelId, field: liteField, url: LITELLM_URL };
    }
    if (price === null) return { reason: `missing-or-invalid-${meter}` };
    rates[`${meter}_token_unit_size`] = 1_000_000;
    rates[`${meter}_token_unit_price`] = price;
    origins[meter] = origin;
  }
  return { rates, origins };
}

function pricesEqual(source, rates) {
  // Compare prices per token, so equivalent unit sizes or decimal spelling do not churn.
  return METERS.every(meter => {
    const key = `${meter}_token_unit_price`;
    const [left, lf = ''] = source[key].split('.');
    const [right, rf = ''] = rates[key].split('.');
    return BigInt(left + lf) * BigInt(rates[`${meter}_token_unit_size`]) * 10n ** BigInt(rf.length) ===
      BigInt(right + rf) * BigInt(source[`${meter}_token_unit_size`]) * 10n ** BigInt(lf.length);
  });
}

function nextVersion(previous, now) {
  const match = /^(\d{4}-\d{2}-\d{2})\.(\d+)$/.exec(previous);
  if (!match) throw new Error(`unsupported catalog_version for automatic sync: ${previous}`);
  const day = now.slice(0, 10);
  return match[1] >= day ? `${match[1]}.${BigInt(match[2]) + 1n}` : `${day}.1`;
}

export function syncModelPricing({ repoRoot, modelsDev, liteLlm, now = new Date().toISOString(), dryRun = false }) {
  validateUpstreamData(modelsDev, liteLlm);
  // Catch existing invalid/immutable sources and generated drift before any writes.
  updateModelPricingCatalog({ repoRoot, check: true });
  const instant = new Date(now).toISOString();
  const pending = [];
  const skipped = [];
  let unchanged = 0;
  const root = path.join(repoRoot, 'model-pricing');
  for (const provider of fs.readdirSync(root, { withFileTypes: true }).filter(p => p.isDirectory() && p.name.startsWith('@'))) {
    const providerRoot = path.join(root, provider.name);
    for (const model of fs.readdirSync(providerRoot, { withFileTypes: true }).filter(m => m.isDirectory())) {
      const file = path.join(providerRoot, model.name, 'pricing.json');
      if (!fs.existsSync(file)) continue;
      const source = readJson(file);
      const label = `${source.provider_code}/${source.upstream_model_id}`;
      if (!source.enabled || source.extensions.price_sync === false) {
        skipped.push({ model: label, reason: 'disabled-or-opted-out' });
        continue;
      }
      if (Date.parse(source.effective_from) > Date.parse(instant) ||
          (source.effective_to !== null && Date.parse(source.effective_to) <= Date.parse(instant))) {
        skipped.push({ model: label, reason: 'not-current' });
        continue;
      }
      const resolved = resolvePrices(source, modelsDev, liteLlm);
      if (resolved.reason) {
        skipped.push({ model: label, reason: resolved.reason });
        continue;
      }
      if (pricesEqual(source, resolved.rates)) {
        unchanged++;
        continue;
      }
      const updated = {
        ...source,
        ...resolved.rates,
        id: crypto.randomUUID(),
        effective_from: instant,
        extensions: {
          ...source.extensions,
          source: 'community_pricing',
          price_sync: { synced_at: instant, meters: resolved.origins },
        },
      };
      // A previous vendor verification cannot attest to new community prices.
      delete updated.extensions.source_url;
      delete updated.extensions.verified_on;
      validatePricingConfiguration(updated, label, true);
      pending.push({ file, source: updated, model: label });
    }
  }
  const result = { updated: pending.map(p => p.model).sort(), unchanged, skipped: skipped.sort((a, b) => a.model.localeCompare(b.model)), dry_run: dryRun };
  if (!pending.length) return result;
  const metadataPath = path.join(root, 'catalog-source.json');
  const metadata = readJson(metadataPath);
  const version = nextVersion(metadata.catalog_version, instant);
  result.catalog_version = version;
  if (dryRun) return result;
  // Roll back source/catalog changes if generation rejects the candidate (including reused historical IDs).
  const trackedRoots = ['catalog', 'dist', '_maintenance'];
  const backups = new Map(pending.map(p => [p.file, fs.readFileSync(p.file)]));
  backups.set(metadataPath, fs.readFileSync(metadataPath));
  for (const directory of trackedRoots) {
    const walk = dir => {
      if (!fs.existsSync(dir)) return;
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const file = path.join(dir, entry.name);
        if (entry.isDirectory()) walk(file);
        else if (entry.isFile()) backups.set(file, fs.readFileSync(file));
      }
    };
    walk(path.join(root, directory));
  }
  try {
    for (const update of pending) fs.writeFileSync(update.file, json(update.source));
    fs.writeFileSync(metadataPath, json({ ...metadata, catalog_version: version }));
    updateModelPricingCatalog({ repoRoot, now: instant });
    updateModelPricingCatalog({ repoRoot, check: true });
  } catch (error) {
    for (const [file, bytes] of backups) fs.writeFileSync(file, bytes);
    throw error;
  }
  return result;
}

export async function fetchJson(url, { fetchImpl = fetch, timeoutMs = 60_000, attempts = 3, retryDelayMs = 1_000 } = {}) {
  for (let attempt = 1; attempt <= attempts; attempt++) {
    try {
      const response = await fetchImpl(url, { signal: AbortSignal.timeout(timeoutMs), headers: { Accept: 'application/json' } });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      return await response.json();
    } catch (error) {
      if (attempt === attempts) throw new Error(`cannot fetch ${url}: ${error.message}`, { cause: error });
      await delay(retryDelayMs * attempt);
    }
  }
}

export async function loadUpstreamData(options = {}) {
  // Require both complete snapshots before syncing; outages must not change source precedence.
  const [modelsDev, liteLlm] = await Promise.all([
    fetchJson(MODELS_DEV_URL, options), fetchJson(LITELLM_URL, options),
  ]);
  validateUpstreamData(modelsDev, liteLlm);
  return { modelsDev, liteLlm };
}

async function main(argv) {
  const options = {};
  for (let i = 0; i < argv.length; i++) {
    const argument = argv[i];
    if (argument === '--dry-run') options.dryRun = true;
    else if (argument === '--models-dev-file' || argument === '--litellm-file') {
      if (!argv[i + 1] || argv[i + 1].startsWith('--')) throw new Error(`${argument} requires a path`);
      options[argument === '--models-dev-file' ? 'modelsDevFile' : 'liteLlmFile'] = path.resolve(argv[++i]);
    } else throw new Error(`unknown argument: ${argument}`);
  }
  if (Boolean(options.modelsDevFile) !== Boolean(options.liteLlmFile)) throw new Error('offline replay requires both --models-dev-file and --litellm-file');
  const upstream = options.modelsDevFile
    ? { modelsDev: readJson(options.modelsDevFile), liteLlm: readJson(options.liteLlmFile) }
    : await loadUpstreamData();
  const result = syncModelPricing({ repoRoot: path.resolve(import.meta.dirname, '../..'), ...upstream, dryRun: options.dryRun });
  console.log(JSON.stringify(result, null, 2));
  if (process.env.GITHUB_STEP_SUMMARY) {
    const summary = `### Model pricing sync\n\nUpdated: ${result.updated.length}; unchanged: ${result.unchanged}; skipped: ${result.skipped.length}.\n\n` +
      [...result.updated.map(m => `- Updated: ${m}`), ...result.skipped.map(s => `- Skipped: ${s.model} (${s.reason})`)].join('\n') + '\n';
    fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
}
