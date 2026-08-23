import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import { stableJson, updateUiComponentCatalog } from '../ui-component-catalog-publisher.mjs';
import {
  buildSignedUiComponentRelease,
  registerSignedUiComponentRelease,
  verifySignedUiComponentRelease,
} from '../ui-component-release.mjs';

function writeJson(repoRoot, relativePath, value) {
  const filePath = path.join(repoRoot, relativePath);
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, stableJson(value));
}

function makeFixture() {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'official-ui-component-release-'));
  writeJson(repoRoot, 'ui_components/catalog-source.json', {
    schema_version: '1flowbase.ui-component-catalog-source/v1',
    catalog_version: '1.0.0',
    generated_at: '2026-08-23T00:00:00.000Z',
    page_size: 100,
  });
  writeJson(repoRoot, 'ui_components/@taichuy/ant-design-x/bubble.json', {
    schema_version: '1flowbase.ui-component-source/v1',
    component_code: 'taichuy.ant-design-x.bubble',
    name: 'Bubble',
    description: 'Render a maintained conversational message sample.',
    import_code: "import { Bubble } from '@ant-design/x';",
    source_code: '<Bubble content="Hello" />',
    origin: 'official',
    source: 'taichuy',
    group: 'ant-design-x',
    upstream: { identity: '@ant-design/x', version: '2.9.0' },
    version: '1.0.0',
    keywords: ['chat', 'message'],
    updated_at: '2026-08-23T00:00:00.000Z',
  });
  updateUiComponentCatalog({ repoRoot });
  return repoRoot;
}

test('D1-AC-02 signs and verifies the exact downloadable Seed bytes', () => {
  const repoRoot = makeFixture();
  const { privateKey, publicKey } = crypto.generateKeyPairSync('ed25519');
  const record = buildSignedUiComponentRelease({ repoRoot, privateKey, keyId: 'fixture-key' });

  assert.equal(record.release_tag, 'ui-component-catalog-v1.0.0');
  assert.match(record.download_url, /ui-component-catalog-v1\.0\.0\.json$/);
  assert.equal(verifySignedUiComponentRelease({ repoRoot, record, publicKey }), true);
  const catalog = registerSignedUiComponentRelease({ repoRoot, record, publicKey });
  assert.deepEqual(catalog.releases, [record]);

  const registeredAgain = registerSignedUiComponentRelease({ repoRoot, record, publicKey });
  assert.deepEqual(registeredAgain.releases, [record]);
});

test('D1-AC-02/D1-AC-03 rejects tampered assets and immutable release conflicts', () => {
  const repoRoot = makeFixture();
  const { privateKey, publicKey } = crypto.generateKeyPairSync('ed25519');
  const record = buildSignedUiComponentRelease({ repoRoot, privateKey, keyId: 'fixture-key' });

  fs.appendFileSync(path.join(repoRoot, 'ui_components/dist/catalog-seed.json'), ' ');
  assert.throws(
    () => verifySignedUiComponentRelease({ repoRoot, record, publicKey }),
    /checksum mismatch/,
  );

  updateUiComponentCatalog({ repoRoot });
  registerSignedUiComponentRelease({ repoRoot, record, publicKey });
  const conflict = { ...record, key_id: 'different-key' };
  assert.throws(
    () => registerSignedUiComponentRelease({ repoRoot, record: conflict, publicKey }),
    /immutable release conflict|signature is invalid/,
  );
});
