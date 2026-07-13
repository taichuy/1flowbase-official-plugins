import assert from 'node:assert/strict';
import test from 'node:test';

import { detectMcpVersionReleases } from '../detect-mcp-version-releases.mjs';

const manifest = (bundleVersion) => JSON.stringify({ bundle_version: bundleVersion });

test('detects a versioned MCP bundle release', () => {
  assert.deepEqual(
    detectMcpVersionReleases([
      {
        organization: 'taichuy',
        bundleId: '1flowbase_zh_hans',
        beforeManifest: manifest('1.0.0'),
        afterManifest: manifest('1.1.0')
      }
    ]),
    [
      {
        bundle_dir: 'mcp/taichuy/1flowbase_zh_hans',
        organization: 'taichuy',
        bundle_id: '1flowbase_zh_hans',
        version: '1.1.0',
        release_tag: 'mcp-taichuy-1flowbase_zh_hans-v1.1.0',
        asset_name: 'taichuy-1flowbase_zh_hans-v1.1.0.zip'
      }
    ]
  );
});

test('rejects bundle content changes without a version bump', () => {
  assert.throws(
    () =>
      detectMcpVersionReleases([
        {
          organization: 'taichuy',
          bundleId: '1flowbase_zh_hans',
          beforeManifest: manifest('1.0.0'),
          afterManifest: manifest('1.0.0')
        }
      ]),
    /without a bundle_version bump/
  );
});
