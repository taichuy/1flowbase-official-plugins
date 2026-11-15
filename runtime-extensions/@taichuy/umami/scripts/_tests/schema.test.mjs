import assert from 'node:assert/strict';
import test from 'node:test';
import codec from '../schema.cjs';

test('schema annotations are removed while business names, defaults and examples remain exact', () => {
  const original = {
    $schema: 'https://json-schema.org/draft/2020-12/schema', id: 'AnnotatedSchema',
    type: 'object', properties: { id: { type: 'string' }, $schema: { type: 'string' }, $defs: { type: 'boolean' } },
    required: ['id'], default: { id: 'business-id', $schema: 'business-schema' }, examples: [{ id: 'example-id' }],
    $defs: { Entity: { id: 'EntityAnnotation', type: 'object', properties: { id: { type: 'string' } }, required: ['id'] } },
    allOf: [{ $ref: '#/$defs/Entity' }],
  };
  const cleaned = codec.cleanSchema(original);
  assert.equal(cleaned.id, undefined);
  assert.deepEqual(Object.keys(cleaned.properties), ['id', '$schema', '$defs']);
  assert.deepEqual(cleaned.default, original.default);
  assert.deepEqual(cleaned.examples, original.examples);
  const resolved = codec.resolveSchemaReferences(cleaned);
  assert.deepEqual(resolved.allOf[0].properties.id, { type: 'string' });
  assert.equal(resolved.$defs, undefined);
  assert.deepEqual(resolved.properties.$defs, { type: 'boolean' });
  assert.deepEqual(resolved.required, ['id']);
  assert.deepEqual(resolved.default, original.default);
});

test('missing and cyclic references fail closed instead of removing input fields', () => {
  assert.throws(() => codec.resolveSchemaReferences({ $ref: '#/$defs/missing' }), /Unresolved/);
  assert.throws(() => codec.resolveSchemaReferences({ $defs: { loop: { $ref: '#/$defs/loop' } }, $ref: '#/$defs/loop' }), /Recursive/);
});
