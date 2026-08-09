import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  buildSignedI18nRelease,
  registerSignedI18nRelease,
  verifySignedI18nRelease,
} from '../i18n-release.mjs';

function fixture() {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'i18n-release-'));
  fs.mkdirSync(path.join(repoRoot, 'i18n', 'dist'), { recursive: true });
  fs.writeFileSync(path.join(repoRoot, 'i18n', 'catalog.json'), JSON.stringify({ catalog_version: '2.0.4' }));
  fs.writeFileSync(path.join(repoRoot, 'i18n', 'dist', 'catalog-seed.json'), '{"seed":true}\n');
  const { privateKey, publicKey } = crypto.generateKeyPairSync('ed25519');
  return { repoRoot, privateKey, publicKey };
}

test('AC-I18N-SIGN builds a byte-verifiable immutable release record', () => {
  const { repoRoot, privateKey, publicKey } = fixture();
  const record = buildSignedI18nRelease({
    repoRoot,
    privateKey,
    keyId: 'official-key-2026-04',
  });

  assert.equal(record.version, '2.0.4');
  assert.equal(record.release_tag, 'i18n-catalog-v2.0.4');
  assert.match(record.checksum, /^sha256:[a-f0-9]{64}$/);
  assert.equal(record.algorithm, 'ed25519');
  assert.equal(record.key_id, 'official-key-2026-04');
  assert.equal(verifySignedI18nRelease({ repoRoot, record, publicKey }), true);
});

test('AC-I18N-SIGN rejects tampering and immutable version conflicts', () => {
  const { repoRoot, privateKey, publicKey } = fixture();
  const record = buildSignedI18nRelease({ repoRoot, privateKey, keyId: 'key-1' });
  registerSignedI18nRelease({ repoRoot, record, publicKey });
  assert.throws(
    () => registerSignedI18nRelease({ repoRoot, record: { ...record, signature: 'AAAA' }, publicKey }),
    /signature|immutable/i,
  );
  fs.writeFileSync(path.join(repoRoot, 'i18n', 'dist', 'catalog-seed.json'), '{"tampered":true}\n');
  assert.throws(() => verifySignedI18nRelease({ repoRoot, record, publicKey }), /checksum/i);
});
