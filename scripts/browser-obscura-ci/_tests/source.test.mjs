import test from 'node:test';
import assert from 'node:assert/strict';
import { readSdkPin } from '../source.mjs';
const sha = 'a'.repeat(40);
const dependency = pin => `runtime-extension-sdk = { git = "https://github.com/taichuy/1flowbase.git", rev = "${pin}" }`;
test('SDK pin is a full host source SHA and matches the requested source', () => {
  assert.equal(readSdkPin(dependency(sha), sha), sha);
  assert.equal(readSdkPin(dependency(sha)), sha);
});
test('SDK branches, short SHAs, wrong repository, and mismatched requested source fail', () => {
  for (const manifest of [dependency('main'), dependency('abcdef0'), dependency(sha).replace('taichuy/1flowbase', 'other/1flowbase'), 'runtime-extension-sdk = { path = "/local/sdk" }']) assert.throws(() => readSdkPin(manifest), /exact official host git revision/);
  assert.throws(() => readSdkPin(dependency(sha), 'b'.repeat(40)), /must equal the plugin SDK pin/);
});
