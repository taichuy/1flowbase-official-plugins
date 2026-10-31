import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
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
  zai: { modelsDev: 'zai', liteLlm: 'zai' },
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

function providerMapping(code) {
  if (code === 'zero') return null;
  return Object.hasOwn(PROVIDERS, code) ? PROVIDERS[code] : { modelsDev: code, liteLlm: code };
}

// Discover identities from the upstream union, rather than using local files as an allowlist.
export function discoverUpstreamModels(modelsDev, liteLlm) {
  const models = new Map();
  const alias = (key, field) => Object.keys(PROVIDERS).find(code => PROVIDERS[code][field] === key) ?? key;
  const add = (provider_code, upstream_model_id) => {
    if (!provider_code || !upstream_model_id) return;
    const source = { provider_code, upstream_model_id };
    models.set(JSON.stringify([provider_code, upstream_model_id]), source);
  };
  for (const [provider, catalog] of Object.entries(modelsDev)) {
    for (const id of Object.keys(catalog.models ?? {})) add(alias(provider, 'modelsDev'), id);
  }
  for (const [key, model] of Object.entries(liteLlm)) {
    const provider = model?.litellm_provider;
    if (key === 'sample_spec' || typeof provider !== 'string' || !/^[a-zA-Z0-9_.-]+$/.test(provider)) continue;
    const id = key.startsWith(`${provider}/`) ? key.slice(provider.length + 1) : key;
    add(alias(provider, 'liteLlm'), id);
  }
  return [...models.values()].sort((a, b) => `${a.provider_code}/${a.upstream_model_id}`.localeCompare(`${b.provider_code}/${b.upstream_model_id}`));
}

function upstreamEntry(source, modelsDev, liteLlm) {
  const provider = providerMapping(source.provider_code);
  if (!provider) return {};
  const id = source.upstream_model_id;
  return {
    model: modelsDev[provider.modelsDev]?.models?.[id],
    fallback: [liteLlm[`${provider.liteLlm}/${id}`], liteLlm[id]]
      .find(model => model?.litellm_provider === provider.liteLlm),
  };
}

// Convert only conditions expressible by the pricing contract; never flatten a tier into the base rate.
function resolveRules(source, modelsDev, liteLlm) {
  const { model, fallback } = upstreamEntry(source, modelsDev, liteLlm);
  const tiers = model?.cost?.tiers;
  const rules = [];
  if (tiers !== undefined) {
    if (!Array.isArray(tiers)) return { reason: 'invalid-price-tiers' };
    for (const tier of tiers) {
      if (tier.tier?.type !== 'context' || !Number.isSafeInteger(tier.tier.size) || tier.tier.size < 0) {
        return { reason: 'unsupported-price-tier' };
      }
      const overrides = {};
      for (const meter of METERS) {
        const field = FIELDS[meter][0];
        if (!Object.hasOwn(tier, field)) continue;
        const price = decimalPrice(tier[field]);
        if (price === null) return { reason: `invalid-tier-${meter}` };
        overrides[`${meter}_token_unit_price`] = price;
      }
      if (!Object.keys(overrides).length) return { reason: 'empty-price-tier' };
      rules.push({ when: { input_tokens: { operator: 'gt', value: tier.tier.size } }, overrides });
    }
    rules.sort((a, b) => a.when.input_tokens.value - b.when.input_tokens.value);
  } else {
    // LiteLLM encodes context thresholds in field names; preserve exact token boundaries.
    const thresholds = [...new Set(Object.keys(fallback ?? {}).flatMap(key => {
      const match = /^(?:input_cost_per_token|output_cost_per_token|cache_read_input_token_cost|cache_creation_input_token_cost)_above_(\d+)k_tokens$/.exec(key);
      return match ? [Number(match[1]) * 1000] : [];
    }))].sort((a, b) => a - b);
    for (const threshold of thresholds) {
      const overrides = {};
      for (const meter of METERS) {
        const field = `${FIELDS[meter][1]}_above_${threshold / 1000}k_tokens`;
        if (!Object.hasOwn(fallback, field)) continue;
        const price = decimalPrice(fallback[field], 6);
        if (price === null) return { reason: `invalid-tier-${meter}` };
        overrides[`${meter}_token_unit_price`] = price;
      }
      rules.push({ when: { input_tokens: { operator: 'gt', value: threshold } }, overrides });
    }
    if (model?.cost?.context_over_200k && !thresholds.length) return { reason: 'ambiguous-context-price-threshold' };
  }
  // LiteLLM's TTL table maps these fields to ephemeral_1h / ephemeral_12h token buckets.
  // https://github.com/BerriAI/litellm/blob/main/litellm/litellm_core_utils/llm_cost_calc/utils.py
  for (const hours of [1, 12]) {
    const ttl = hours * 3600;
    const field = `cache_creation_input_token_cost_above_${hours}hr`;
    if (!fallback || !Object.hasOwn(fallback, field)) continue;
    const price = decimalPrice(fallback[field], 6);
    if (price === null) return { reason: 'invalid-ttl-cache_write' };
    rules.push({ when: { cache_write_ttl_seconds: ttl }, overrides: { cache_write_token_unit_price: price } });
    const contextRates = Object.entries(fallback).flatMap(([name, value]) => {
      const match = new RegExp(`^${field}_above_(\\d+)k_tokens$`).exec(name);
      return match ? [{ threshold: Number(match[1]) * 1000, value }] : [];
    }).sort((a, b) => a.threshold - b.threshold);
    for (const { threshold, value } of contextRates) {
      const price = decimalPrice(value, 6);
      if (price === null) return { reason: 'invalid-ttl-context-cache_write' };
      rules.push({ when: { input_tokens: { operator: 'gt', value: threshold }, cache_write_ttl_seconds: ttl }, overrides: { cache_write_token_unit_price: price } });
    }
  }
  return { rules };
}

export function resolvePrices(source, modelsDev, liteLlm) {
  const provider = providerMapping(source.provider_code);
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
  updateModelPricingCatalog({ repoRoot, check: true });
  const instant = new Date(now).toISOString();
  const root = path.join(repoRoot, 'model-pricing');
  const identity = source => JSON.stringify([source.provider_code, source.upstream_model_id]);
  const label = source => `${source.provider_code}/${source.upstream_model_id}`;
  const existing = new Map();
  for (const provider of fs.readdirSync(root, { withFileTypes: true }).filter(p => p.isDirectory() && p.name.startsWith('@'))) {
    for (const model of fs.readdirSync(path.join(root, provider.name), { withFileTypes: true }).filter(m => m.isDirectory())) {
      const file = path.join(root, provider.name, model.name, 'pricing.json');
      if (fs.existsSync(file)) {
        const source = readJson(file);
        existing.set(identity(source), { source, file });
      }
    }
  }
  const upstream = discoverUpstreamModels(modelsDev, liteLlm);
  const candidates = new Map(upstream.map(source => [identity(source), source]));
  const pending = [];
  const removed = [];
  const skipped = [];
  const inventory = [];
  let unchanged = 0;
  for (const source of upstream) {
    const previous = existing.get(identity(source));
    const old = previous?.source;
    const { model, fallback } = upstreamEntry(source, modelsDev, liteLlm);
    const quote = {
      models_dev: model?.cost ?? null,
      litellm: fallback ? Object.fromEntries(Object.entries(fallback).filter(([field]) => /cost|price/.test(field))) : null,
    };
    const skip = reason => {
      skipped.push({ model: label(source), reason });
      inventory.push({ ...source, model: label(source), status: 'unavailable', reason, pricing: quote });
    };
    if (old && (!old.enabled || old.extensions.price_sync === false)) { skip('disabled-or-opted-out'); continue; }
    if (old && (Date.parse(old.effective_from) > Date.parse(instant) ||
        (old.effective_to !== null && Date.parse(old.effective_to) <= Date.parse(instant)))) { skip('not-current'); continue; }
    const mode = fallback?.mode;
    if ((!model && mode && !['chat', 'completion', 'responses'].includes(mode)) ||
        (model?.type && !['chat', 'text', 'llm'].includes(model.type))) { skip('unsupported-model-meter'); continue; }
    const resolved = resolvePrices(source, modelsDev, liteLlm);
    if (resolved.reason) { skip(resolved.reason); continue; }
    const conditions = resolveRules(source, modelsDev, liteLlm);
    if (conditions.reason) { skip(conditions.reason); continue; }
    // Preserve locally authored non-context rules; upstream owns context thresholds and rates.
    const managedRules = old?.extensions.price_sync?.rules ?? [];
    const rules = [...conditions.rules, ...(old?.rules ?? []).filter(r => {
      if (managedRules.some(managed => JSON.stringify(managed) === JSON.stringify(r))) return false;
      if (Object.keys(r.when).length === 1 && r.when.input_tokens) return false;
      if (r.when.cache_write_ttl_seconds &&
          Object.keys(r.when).every(key => ['cache_write_ttl_seconds', 'input_tokens'].includes(key)) &&
          conditions.rules.some(rule => rule.when.cache_write_ttl_seconds === r.when.cache_write_ttl_seconds)) return false;
      return true;
    })];
    inventory.push({ ...source, model: label(source), status: 'synchronized', pricing: quote });
    if (old && pricesEqual(old, resolved.rates) && JSON.stringify(old.rules) === JSON.stringify(rules) &&
        JSON.stringify(managedRules) === JSON.stringify(conditions.rules)) {
      unchanged++;
      continue;
    }
    const updated = {
      schema_version: '1flowbase.model-pricing-source/v2',
      ...source, currency_code: 'USD',
      effective_to: null, timezone: 'UTC', weekday_mask: 127,
      local_time_start: null, local_time_end: null, priority: 0, enabled: true,
      ...old, ...resolved.rates,
      id: crypto.randomUUID(), effective_from: instant, rules,
      extensions: {
        ...old?.extensions, source: 'community_pricing',
        price_sync: { synced_at: instant, meters: resolved.origins, rules: conditions.rules },
      },
    };
    delete updated.extensions.source_url;
    delete updated.extensions.verified_on;
    validatePricingConfiguration(updated, label(source), true);
    // Encode identifiers as one path component (including slash, dots and percent escapes).
    const component = value => {
      const encoded = encodeURIComponent(value).replace(/\./g, '%2E');
      return encoded.length <= 180 ? encoded : `model-${crypto.createHash('sha256').update(value).digest('hex')}`;
    };
    // Provider directory must match the contract identity, and cannot escape the source root.
    if (!/^[a-zA-Z0-9_.-]+$/.test(source.provider_code)) throw new Error(`unsafe provider identity: ${source.provider_code}`);
    const target = previous?.file ?? path.join(root, `@${source.provider_code}`, component(source.upstream_model_id), 'pricing.json');
    pending.push({ file: target, source: updated, model: label(source), added: !old });
  }
  for (const [key, entry] of existing) {
    if (candidates.has(key)) continue;
    const source = entry.source;
    if (providerMapping(source.provider_code) && source.enabled && source.extensions.price_sync !== false &&
        source.extensions.source === 'community_pricing') removed.push({ ...entry, model: label(source) });
    else skipped.push({ model: label(source), reason: 'local-only' });
  }
  const result = {
    discovered: upstream.length,
    added: pending.filter(p => p.added).map(p => p.model).sort(),
    updated: pending.filter(p => !p.added).map(p => p.model).sort(),
    removed: removed.map(p => p.model).sort(), unchanged,
    skipped: skipped.sort((a, b) => a.model.localeCompare(b.model)), dry_run: dryRun,
  };
  const canonical = value => Array.isArray(value) ? value.map(canonical)
    : object(value) ? Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])])) : value;
  const report = json(canonical({ schema_version: '1flowbase.upstream-model-inventory/v1', models: inventory }));
  const reportPath = path.join(root, '_maintenance/upstream-models.json');
  const reportChanged = !fs.existsSync(reportPath) || fs.readFileSync(reportPath, 'utf8') !== report;
  if (!pending.length && !removed.length && !reportChanged) return result;
  const metadataPath = path.join(root, 'catalog-source.json');
  const metadata = readJson(metadataPath);
  const changed = pending.length > 0 || removed.length > 0;
  if (changed) result.catalog_version = nextVersion(metadata.catalog_version, instant);
  if (dryRun) return result;
  // Snapshot the entire pricing tree so additions, removed sources and generated pages roll back together.
  const backupRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'model-pricing-transaction-'));
  const backup = path.join(backupRoot, 'model-pricing');
  fs.cpSync(root, backup, { recursive: true });
  try {
    for (const update of pending) {
      fs.mkdirSync(path.dirname(update.file), { recursive: true });
      fs.writeFileSync(update.file, json(update.source));
    }
    for (const entry of removed) fs.unlinkSync(entry.file);
    if (changed) fs.writeFileSync(metadataPath, json({ ...metadata, catalog_version: result.catalog_version }));
    updateModelPricingCatalog({ repoRoot, now: instant });
    updateModelPricingCatalog({ repoRoot, check: true });
    fs.mkdirSync(path.dirname(reportPath), { recursive: true });
    fs.writeFileSync(reportPath, report);
  } catch (error) {
    fs.rmSync(root, { recursive: true });
    fs.cpSync(backup, root, { recursive: true });
    throw error;
  } finally {
    fs.rmSync(backupRoot, { recursive: true, force: true });
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
    const reasons = result.skipped.reduce((counts, entry) => {
      counts[entry.reason] = (counts[entry.reason] ?? 0) + 1;
      return counts;
    }, {});
    const summary = `### Model pricing sync\n\nDiscovered: ${result.discovered}; added: ${result.added.length}; updated: ${result.updated.length}; removed: ${result.removed.length}; unchanged: ${result.unchanged}; skipped: ${result.skipped.length}.\n\n` +
      Object.entries(reasons).map(([reason, count]) => `- ${reason}: ${count}`).join('\n') +
      '\n\nFull model identities, upstream pricing and availability reasons are in `_maintenance/upstream-models.json` and the `model-pricing-sync-report` artifact.\n';
    fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
}
