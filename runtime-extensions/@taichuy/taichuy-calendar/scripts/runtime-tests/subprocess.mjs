// Machine-readable bounded subprocess entry; not a Node test auto-discovery file.
import assert from 'node:assert/strict';
import { corpus } from './corpus.mjs';
import { assertFixture, createReference, defaultAsset } from './reference.mjs';
const asset = process.argv[2] ?? defaultAsset;
const observations = corpus.map(fixture => {
  const output = createReference(asset).call({ operation: fixture.operation, body: fixture.body });
  assertFixture(fixture, output); // Never let equal-but-wrong engines satisfy the corpus.
  return { id: fixture.id, output };
});
const text = JSON.stringify(observations);
assert.ok(Buffer.byteLength(text) <= 16 * 1024 * 1024, 'subprocess observations unexpectedly large');
process.stdout.write(text + '\n');
