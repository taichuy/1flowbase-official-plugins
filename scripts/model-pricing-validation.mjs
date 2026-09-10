export const METERS = ['input', 'output', 'cache_hit', 'cache_write'];
export const RATE_FIELDS = METERS.flatMap(m => [`${m}_token_unit_size`, `${m}_token_unit_price`]);
const schedule = ['effective_from', 'effective_to', 'timezone', 'weekday_mask', 'local_time_start', 'local_time_end'];
const own = (v, k) => Object.hasOwn(v, k);
function object(v, required, optional, fail) {
  if (!v || typeof v !== 'object' || Array.isArray(v) || required.some(k => !own(v, k)) ||
      Object.keys(v).some(k => ![...required, ...optional].includes(k))) fail();
}
function positive(v) { return Number.isSafeInteger(v) && v > 0; }
function decimal(v) {
  return typeof v === 'string' && /^[0-9]+(\.[0-9]{1,18})?$/.test(v) &&
    BigInt(v.replace('.', '')) <= 79228162514264337593543950335n;
}
function validateSchedule(v, fail, conditional = false) {
  for (const k of ['effective_from', 'effective_to']) {
    if (own(v, k) && !(v[k] === null && k === 'effective_to' && !conditional) &&
        (typeof v[k] !== 'string' || !/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?(?:Z|[+-]\d\d:\d\d)$/.test(v[k]) || !Number.isFinite(Date.parse(v[k])))) fail();
  }
  if (v.effective_from && v.effective_to && Date.parse(v.effective_from) >= Date.parse(v.effective_to)) fail();
  if (own(v, 'timezone')) {
    if (typeof v.timezone !== 'string' || !v.timezone) fail();
    try { new Intl.DateTimeFormat('en', { timeZone: v.timezone }); } catch { fail(); }
  }
  if (own(v, 'weekday_mask') && (!Number.isSafeInteger(v.weekday_mask) || v.weekday_mask < 1 || v.weekday_mask > 127)) fail();
  for (const k of ['local_time_start', 'local_time_end']) {
    if (own(v, k) && !(v[k] === null && !conditional) &&
        (typeof v[k] !== 'string' || !/^(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d$/.test(v[k]))) fail();
  }
  if ((v.local_time_start != null) !== (v.local_time_end != null)) fail();
  if (conditional && v.local_time_start != null && !own(v, 'timezone')) fail();
}
export function validatePricingConfiguration(v, context = 'pricing configuration', source = false) {
  const fail = () => { throw new Error(`${context} has an invalid v2 pricing configuration`); };
  const required = ['id', 'provider_code', 'upstream_model_id', 'currency_code', ...RATE_FIELDS,
    ...schedule, 'priority', 'enabled', 'extensions', 'rules'];
  object(v, source ? ['schema_version', ...required] : [...required, 'source_kind', 'source_catalog_id', 'source_version', 'source_checksum'], [], fail);
  if (!source && (v.source_kind !== 'official' || v.source_catalog_id !== v.id ||
      typeof v.source_version !== 'string' || !v.source_version ||
      !/^sha256:[a-f0-9]{64}$/.test(v.source_checksum))) fail();
  if (source && v.schema_version !== '1flowbase.model-pricing-source/v2') fail();
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(v.id) ||
      !['provider_code', 'upstream_model_id'].every(k => typeof v[k] === 'string' && v[k].length) ||
      v.currency_code !== 'USD' || !Number.isSafeInteger(v.priority) || v.priority < 0 || typeof v.enabled !== 'boolean' ||
      !v.extensions || typeof v.extensions !== 'object' || Array.isArray(v.extensions)) fail();
  for (const k of RATE_FIELDS) if (!(k.endsWith('_size') ? positive(v[k]) : decimal(v[k]))) fail();
  validateSchedule(v, fail);
  if (!Array.isArray(v.rules)) fail();
  for (const rule of v.rules) {
    object(rule, ['when', 'overrides'], [], fail);
    object(rule.when, [], ['input_tokens', 'cache_write_ttl_seconds', ...schedule], fail);
    object(rule.overrides, [], RATE_FIELDS, fail);
    if (!Object.keys(rule.when).length || !Object.keys(rule.overrides).length) fail();
    if (own(rule.when, 'input_tokens')) {
      const t = rule.when.input_tokens;
      object(t, ['operator', 'value'], [], fail);
      if (!['gt', 'gte'].includes(t.operator) || !Number.isSafeInteger(t.value) || t.value < 0) fail();
    }
    if (own(rule.when, 'cache_write_ttl_seconds') && (!positive(rule.when.cache_write_ttl_seconds) ||
        Object.keys(rule.overrides).some(k => !k.startsWith('cache_write_')))) fail();
    validateSchedule(rule.when, fail, true);
    for (const [k, value] of Object.entries(rule.overrides)) if (!(k.endsWith('_size') ? positive(value) : decimal(value))) fail();
  }
  return v;
}
