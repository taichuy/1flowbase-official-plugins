import test from 'node:test';
import assert from 'node:assert/strict';

import {
  cleanupAssetLines,
  matchesProviderPlatformAsset,
} from '../assets.mjs';

const base = '1flowbase@openai@0.2.36@darwin-amd64';
const digest = 'a'.repeat(64);

test('matches current short and historical digest package names for one platform', () => {
  assert.equal(matchesProviderPlatformAsset(`${base}.1flowbasepkg`, base), true);
  assert.equal(
    matchesProviderPlatformAsset(`${base}@${digest}.1flowbasepkg`, base),
    true
  );
});

test('rejects other platforms and malformed legacy package names', () => {
  assert.equal(
    matchesProviderPlatformAsset('1flowbase@openai@0.2.36@darwin-arm64.1flowbasepkg', base),
    false
  );
  assert.equal(matchesProviderPlatformAsset(`${base}@${'a'.repeat(63)}.1flowbasepkg`, base), false);
  assert.equal(matchesProviderPlatformAsset(`${base}@${digest}.zip`, base), false);
});

test('cleanup output contains only replaceable same-platform package assets', () => {
  assert.equal(
    cleanupAssetLines(
      {
        assets: [
          { id: 1, name: `${base}.1flowbasepkg` },
          { id: 2, name: `${base}@${digest}.1flowbasepkg` },
          { id: 3, name: `${base}.sha256` },
        ],
      },
      base
    ),
    `1\t${base}.1flowbasepkg\n2\t${base}@${digest}.1flowbasepkg`
  );
});
