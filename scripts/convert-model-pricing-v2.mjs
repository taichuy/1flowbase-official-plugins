import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateRatingPolicy } from './model-pricing-legacy.mjs';
import { METERS, RATE_FIELDS, validatePricingConfiguration } from './model-pricing-validation.mjs';
const json = v => `${JSON.stringify(v, null, 2)}\n`;
const checksum = v => `sha256:${crypto.createHash('sha256').update(json(v)).digest('hex')}`;
const fail = () => { throw new Error('unsupported or malformed legacy pricing source'); };
const sparse = (rates, base) => Object.fromEntries(Object.entries(rates).filter(([k,v]) => v !== base[k]));
function convertRule(old) {
  const { rating_policy_enabled, rating_policy, ...base } = old;
  validateRatingPolicy(old, 'legacy source');
  const allowed = ['id', ...RATE_FIELDS.filter(k => !k.startsWith('cache_write')), 'effective_from', 'effective_to', 'timezone', 'weekday_mask', 'local_time_start', 'local_time_end', 'priority', 'enabled', 'extensions'];
  if (Object.keys(base).some(k => !allowed.includes(k)) || allowed.some(k => !Object.hasOwn(base,k))) fail();
  base.cache_write_token_unit_size = base.input_token_unit_size;
  base.cache_write_token_unit_price = base.input_token_unit_price;
  base.rules = [];
  if (!rating_policy_enabled) { if (Object.keys(rating_policy).length) fail(); return base; }
  if (rating_policy.schema_version.endsWith('/v1')) {
    if (Object.keys(rating_policy).some(k => !['schema_version','type','tiers'].includes(k))) fail();
    let previousRates = base;
    for (const tier of rating_policy.tiers) {
      if (Object.keys(tier).some(k => !['when','rates'].includes(k)) || Object.keys(tier.when).some(k => !['operator','value'].includes(k)) ||
          Object.keys(tier.rates).some(k => !['input','output','cache_hit'].includes(k))) fail();
      const rates = {};
      for (const meter of METERS) {
        const rate = tier.rates[meter === 'cache_write' ? 'input' : meter];
        if (Object.keys(rate).some(k => !['unit_size','unit_price'].includes(k))) fail();
        for (const suffix of ['size','price']) rates[`${meter}_token_unit_${suffix}`] = rate[`unit_${suffix}`];
      }
      const overrides = sparse(rates, previousRates);
      previousRates = rates;
      if (Object.keys(overrides).length) base.rules.push({ when: { input_tokens: tier.when }, overrides });
    }
  } else {
    const policy = rating_policy;
    const convertRates = rates => {
      const result = {};
      for (const meter of METERS) {
        result[`${meter}_token_unit_size`] = policy.unit_size;
        const rate = rates[meter];
        result[`${meter}_token_unit_price`] = meter !== 'cache_write' ? rate :
          rate.unit_price ?? Object.entries(rate.by_ttl_seconds).sort((a,b) => Number(a[0])-Number(b[0]))[0][1];
      }
      return result;
    };
    Object.assign(base, convertRates(policy.rates));
    const ttlRules = (rates, when, reference) => {
      for (const [ttl, price] of Object.entries(rates.cache_write.by_ttl_seconds ?? {})) {
        if (price !== reference.cache_write_token_unit_price) base.rules.push({when: {...when, cache_write_ttl_seconds: Number(ttl)}, overrides: {cache_write_token_unit_price: price}});
      }
    };
    ttlRules(policy.rates, {}, base);
    let previousRates = base;
    let previousTtlRates = Boolean(policy.rates.cache_write.by_ttl_seconds);
    for (const tier of policy.input_token_tiers ?? []) {
      const rates = convertRates(tier.rates);
      const when = { input_tokens: tier.when };
      // Complete tier prices reset any earlier TTL-specific price before tier TTL overrides.
      const overrides = sparse(rates, previousRates);
      previousRates = rates;
      if (previousTtlRates) overrides.cache_write_token_unit_price = rates.cache_write_token_unit_price;
      previousTtlRates ||= Boolean(tier.rates.cache_write.by_ttl_seconds);
      if (Object.keys(overrides).length) base.rules.push({ when, overrides });
      ttlRules(tier.rates, when, rates);
    }
  }
  return base;
}
export function convertPricingSource(source) {
  if (source.schema_version === '1flowbase.model-pricing-source/v2') return validatePricingConfiguration(source, 'source', true);
  if (source.schema_version !== '1flowbase.model-pricing-source/v1' || !Array.isArray(source.rules) || !source.rules.length ||
      Object.keys(source).some(k => !['schema_version','provider_code','upstream_model_id','currency_code','rules'].includes(k))) fail();
  const converted = source.rules.map(convertRule);
  for (const c of converted) validatePricingConfiguration({ ...c, schema_version: '1flowbase.model-pricing-source/v2', provider_code: source.provider_code, upstream_model_id: source.upstream_model_id, currency_code: source.currency_code }, 'legacy conversion', true);
  let base;
  if (converted.length === 1) base = converted[0];
  else if (converted.every(c => c.local_time_start !== null && c.effective_from === converted[0].effective_from && c.effective_to === converted[0].effective_to && c.timezone === converted[0].timezone && c.weekday_mask === converted[0].weekday_mask && c.priority === converted[0].priority && c.enabled === converted[0].enabled && !c.rules.length)) {
    const windows = [...converted].sort((a,b) => a.local_time_start.localeCompare(b.local_time_start));
    if (windows[0].local_time_start !== '00:00:00' || windows.at(-1).local_time_end !== '00:00:00' || windows.some((c,i) => i < windows.length-1 && (c.local_time_end !== windows[i+1].local_time_start || c.local_time_start >= c.local_time_end))) fail();
    base = structuredClone(windows[0]);
    base.local_time_start = null; base.local_time_end = null;
    delete base.extensions.time_band;
    for (const window of windows) {
      const overrides = sparse(Object.fromEntries(RATE_FIELDS.map(k => [k,window[k]])), base);
      if (Object.keys(overrides).length) base.rules.push({when: {timezone: window.timezone, local_time_start: window.local_time_start, local_time_end: window.local_time_end}, overrides});
    }
  } else {
    const ordered = [...converted].sort((a,b) => a.effective_from.localeCompare(b.effective_from));
    if (ordered.every(c => c.local_time_start === null && c.local_time_end === null && c.timezone === ordered[0].timezone && c.weekday_mask === ordered[0].weekday_mask && c.enabled === ordered[0].enabled) &&
        ordered.every((c,i) => i === ordered.length-1 ? c.effective_to === null : c.effective_to === ordered[i+1].effective_from) && ordered.every(c => !c.rules.length && c.priority === ordered[0].priority)) {
      base = structuredClone(ordered[0]); base.effective_to = null;
      for (let i=1;i<ordered.length;i++) {
        const overrides = sparse(Object.fromEntries(RATE_FIELDS.map(k => [k,ordered[i][k]])), ordered[i-1]);
        if (Object.keys(overrides).length) base.rules.push({when:{effective_from: ordered[i].effective_from}, overrides});
      }
    } else if (ordered.every(c => c.effective_to === null && c.local_time_start === null && c.local_time_end === null && c.timezone === ordered[0].timezone && c.weekday_mask === ordered[0].weekday_mask && c.enabled === ordered[0].enabled) && ordered.every((c,i) => i === 0 || c.priority > ordered[i-1].priority)) {
      // Authorized latest-only standard selection; history stays in the immutable v1 release/fixture.
      base = structuredClone(ordered.at(-1)); base.priority = 0;
    } else fail();
  }
  return validatePricingConfiguration({schema_version:'1flowbase.model-pricing-source/v2', provider_code:source.provider_code, upstream_model_id:source.upstream_model_id, currency_code:source.currency_code, ...base}, 'converted source', true);
}
export function migratePricingRepository(repoRoot) {
  const root = path.join(repoRoot, 'model-pricing');
  const writes = [];
  const converted = [];
  for (const provider of fs.readdirSync(root).filter(x => x.startsWith('@')).sort()) {
    for (const model of fs.readdirSync(path.join(root,provider)).sort()) {
      const file = path.join(root,provider,model,'pricing.json');
      if (!fs.existsSync(file)) continue;
      const old = JSON.parse(fs.readFileSync(file,'utf8')); const next = convertPricingSource(old);
      if (next.provider_code !== provider.slice(1)) fail();
      converted.push(next);
      if (old.schema_version !== next.schema_version) writes.push([file,next]);
    }
  }
  if (new Set(converted.map(c => JSON.stringify([c.provider_code,c.upstream_model_id]))).size !== converted.length || new Set(converted.map(c => c.id)).size !== converted.length) fail();
  const statePath = path.join(root,'_maintenance/catalog-state.json');
  const state = fs.existsSync(statePath) ? JSON.parse(fs.readFileSync(statePath,'utf8')) : null;
  if (state?.schema_version === '1flowbase.model-pricing-state/v1') {
    const oldCatalog = JSON.parse(fs.readFileSync(path.join(root,'catalog/v1/catalog.json'),'utf8'));
    if (oldCatalog.schema_version !== '1flowbase.model-pricing/v1' || oldCatalog.rules_checksum !== `sha256:${crypto.createHash('sha256').update(JSON.stringify(oldCatalog.rules)).digest('hex')}`) fail();
    const groups = new Map();
    for (const r of oldCatalog.rules) {
      const { provider_code, upstream_model_id, currency_code, source_kind, source_catalog_id, source_checksum, source_version, ...rule } = r;
      if (source_checksum !== checksum({provider_code,upstream_model_id,rule}) || state.rules[r.id]?.source_checksum !== source_checksum) fail();
      const key = JSON.stringify([provider_code,upstream_model_id]);
      if (!groups.has(key)) groups.set(key,{schema_version:'1flowbase.model-pricing-source/v1',provider_code,upstream_model_id,currency_code,rules:[]});
      groups.get(key).rules.push(rule);
    }
    if (Object.keys(state.rules).length !== oldCatalog.rules.length) fail();
    const normalized = new Map([...groups].map(([k,v]) => [k,convertPricingSource(v)]));
    for (const c of converted) {
      const expected = normalized.get(JSON.stringify([c.provider_code,c.upstream_model_id]));
      // Key order is not content: compare canonical structural JSON.
      const stable = v => Array.isArray(v) ? v.map(stable) : v && typeof v === 'object' ? Object.fromEntries(Object.keys(v).sort().map(k=>[k,stable(v[k])])) : v;
      if (!expected || JSON.stringify(stable(expected)) !== JSON.stringify(stable(c))) fail();
    }
    if (normalized.size !== converted.length) fail();
    const rules = {...state.rules};
    for (const {schema_version,...c} of converted) rules[c.id] = {...rules[c.id],source_checksum:checksum(c)};
    writes.push([statePath,{...state,schema_version:'1flowbase.model-pricing-state/v2',source_fingerprint:null,rules}]);
  } else if (state && state.schema_version !== '1flowbase.model-pricing-state/v2') fail();
  const metadataPath = path.join(root,'catalog-source.json');
  const metadata = JSON.parse(fs.readFileSync(metadataPath,'utf8'));
  if (metadata.schema_version === '1flowbase.model-pricing-source/v1') writes.push([metadataPath,{...metadata,schema_version:'1flowbase.model-pricing-source/v2',catalog_version:'2026-09-10.1'}]);
  else if (metadata.schema_version !== '1flowbase.model-pricing-source/v2') fail();
  for (const [file,value] of writes) fs.writeFileSync(file,json(value));
  return writes.length;
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length > 3) throw new Error('usage: convert-model-pricing-v2.mjs [repo-root]');
  console.log(`model-pricing v2: converted ${migratePricingRepository(path.resolve(process.argv[2] ?? path.join(import.meta.dirname,'..')))} files`);
}
