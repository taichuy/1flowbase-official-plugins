// Traverse JSON Schema keywords, never the contents of defaults or examples.
// Property/definition maps carry business names, including `id` and `$schema`.
const maps = new Set(['properties', 'patternProperties', '$defs', 'definitions', 'dependentSchemas']);
const arrays = new Set(['allOf', 'anyOf', 'oneOf', 'prefixItems']);
const singles = new Set(['items', 'additionalProperties', 'additionalItems', 'contains', 'not', 'if', 'then', 'else', 'propertyNames', 'contentSchema', 'unevaluatedProperties', 'unevaluatedItems']);

function children(schema, visit, omitted) {
  if (!schema || typeof schema !== 'object' || Array.isArray(schema)) return structuredClone(schema);
  const result = {};
  for (const [name, value] of Object.entries(schema)) {
    if (omitted.has(name)) continue;
    if (maps.has(name) && value && typeof value === 'object' && !Array.isArray(value)) {
      result[name] = Object.fromEntries(Object.entries(value).map(([field, child]) => [field, visit(child)]));
    } else if (arrays.has(name) && Array.isArray(value)) result[name] = value.map(visit);
    else if (singles.has(name)) result[name] = Array.isArray(value) ? value.map(visit) : visit(value);
    else result[name] = structuredClone(value);
  }
  return result;
}

function cleanSchema(schema) {
  // Zod's schema-node `id` is an annotation. A properties-map entry named id
  // stays intact because children() visits the entry's schema, not its name.
  return children(schema, cleanSchema, new Set(['id', '$schema']));
}

function resolveSchemaReferences(schema) {
  const root = schema;
  function visit(value, trail = []) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return structuredClone(value);
    if (Object.hasOwn(value, '$ref')) {
      const ref = value.$ref;
      if (typeof ref !== 'string' || !ref.startsWith('#/')) throw new Error(`External schema reference ${String(ref)}`);
      if (trail.includes(ref)) throw new Error(`Recursive schema requires an explicit contract: ${ref}`);
      const target = ref.slice(2).split('/').reduce((current, token) => current?.[token.replaceAll('~1', '/').replaceAll('~0', '~')], root);
      if (target === undefined) throw new Error(`Unresolved schema reference ${ref}`);
      const { $ref, ...siblings } = value;
      const resolved = visit(target, [...trail, ref]);
      const rest = visit(siblings, trail);
      if (typeof resolved === 'boolean') {
        if (!Object.keys(rest).length) return resolved;
        return { allOf: [resolved, rest] };
      }
      return { ...resolved, ...rest };
    }
    return children(value, child => visit(child, trail), new Set(['$defs', 'definitions', '$schema', 'id']));
  }
  return visit(schema);
}

module.exports = { cleanSchema, resolveSchemaReferences };
