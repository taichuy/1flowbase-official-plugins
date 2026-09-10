// Read-only validator for the offline v1 conversion boundary.
export function validateRatingPolicy(rule, context) {
  if (typeof rule.rating_policy_enabled !== 'boolean' ||
      rule.rating_policy === null || typeof rule.rating_policy !== 'object' ||
      Array.isArray(rule.rating_policy)) {
    throw new Error(`${context} has an invalid rating policy`);
  }
  if (!rule.rating_policy_enabled) return;
  const policy = rule.rating_policy;
  if (policy.schema_version === '1flowbase.model-rating-policy/v2') {
    validateTokenPricingPolicy(policy, context);
    return;
  }
  if (policy.schema_version !== '1flowbase.model-rating-policy/v1' ||
      policy.type !== 'input_token_tiers' || !Array.isArray(policy.tiers) ||
      policy.tiers.length === 0) {
    throw new Error(`${context} has an unsupported rating policy`);
  }
  let previousThreshold = -1;
  for (const tier of policy.tiers) {
    const threshold = tier?.when?.value;
    if (!['gt', 'gte'].includes(tier?.when?.operator) ||
        !Number.isSafeInteger(threshold) || threshold < 0 || threshold <= previousThreshold) {
      throw new Error(`${context} rating policy tiers must be strictly ascending`);
    }
    previousThreshold = threshold;
    for (const meter of ['input', 'output', 'cache_hit']) {
      const rate = tier?.rates?.[meter];
      if (!Number.isSafeInteger(rate?.unit_size) || rate.unit_size < 1 ||
          typeof rate.unit_price !== 'string' ||
          !/^[0-9]+(\.[0-9]{1,18})?$/.test(rate.unit_price)) {
        throw new Error(`${context} has an invalid ${meter} tier rate`);
      }
    }
  }
}

function validateTokenPricingPolicy(policy, context) {
  const fail = () => { throw new Error(`${context} has an invalid v2 rating policy`); };
  const object = (value, required, optional = []) => {
    if (!value || typeof value !== 'object' || Array.isArray(value) ||
        required.some((key) => !Object.hasOwn(value, key)) ||
        Object.keys(value).some((key) => ![...required, ...optional].includes(key))) fail();
  };
  const decimal = (value) => {
    if (typeof value !== 'string' || !/^[0-9]+(\.[0-9]{1,18})?$/.test(value)) fail();
    // Match Decimal::from_str_exact: the unscaled coefficient must fit 96 bits.
    if (BigInt(value.replace('.', '')) > 79228162514264337593543950335n) fail();
  };
  const rates = (value) => {
    object(value, ['input', 'output', 'cache_hit', 'cache_write']);
    for (const meter of ['input', 'output', 'cache_hit']) decimal(value[meter]);
    const write = value.cache_write;
    if (write && Object.hasOwn(write, 'unit_price')) {
      object(write, ['unit_price']);
      decimal(write.unit_price);
    } else {
      object(write, ['by_ttl_seconds']);
      const buckets = write.by_ttl_seconds;
      if (!buckets || typeof buckets !== 'object' || Array.isArray(buckets) ||
          Object.keys(buckets).length === 0) fail();
      for (const [ttl, price] of Object.entries(buckets)) {
        if (!/^[1-9][0-9]*$/.test(ttl) || !Number.isSafeInteger(Number(ttl))) fail();
        decimal(price);
      }
    }
  };
  object(policy, ['schema_version', 'type', 'unit_size', 'rates'], ['input_token_tiers']);
  if (policy.type !== 'token_pricing' || !Number.isSafeInteger(policy.unit_size) || policy.unit_size < 1) fail();
  rates(policy.rates);
  if (Object.hasOwn(policy, 'input_token_tiers')) {
    if (!Array.isArray(policy.input_token_tiers) || policy.input_token_tiers.length === 0) fail();
    let previous = -1;
    for (const tier of policy.input_token_tiers) {
      object(tier, ['when', 'rates']);
      object(tier.when, ['operator', 'value']);
      if (!['gt', 'gte'].includes(tier.when.operator) ||
          !Number.isSafeInteger(tier.when.value) || tier.when.value < 0 || tier.when.value <= previous) fail();
      previous = tier.when.value;
      rates(tier.rates);
    }
  }
}

