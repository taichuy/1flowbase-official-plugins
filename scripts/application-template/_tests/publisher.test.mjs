import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { generateKeyPairSync } from 'node:crypto';
import { splitPackage } from '../archive.mjs';
import { buildReleasePlan, signArchive, verifyArchive, updateCatalog } from '../publisher.mjs';
import { buildCategoryCatalog } from '../../extension-catalog.mjs';
const fixture = version => ({ schema_version: '1flowbase.portable-template/v1', release: { template_id: '@test/demo', release_version: version, name: 'Demo', description: 'Template', exported_from_system_version: '0.4.1', exported_at: '2026-10-03T00:00:00Z' }, pages: [], applications: [], data_models: [], plugins: [], mcp_bundle: null });
test('signs exact ZIP bytes, refuses changed releases, preserves history and emits discovery metadata', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'template-publisher-')); t.after(() => fs.rm(root, { recursive: true, force: true }));
  const directory = path.join(root, 'applications-demo/@test/demo');
  const keys = generateKeyPairSync('ed25519'); const privateKeyPem = keys.privateKey.export({ type: 'pkcs8', format: 'pem' }); const publicKeyPem = keys.publicKey.export({ type: 'spki', format: 'pem' });
  await splitPackage(fixture(2), directory);
  let plan = await buildReleasePlan({ repoRoot: root }); assert.equal(plan.length, 1); assert.equal(plan[0].asset_name, 'test-demo-v2.zip');
  let signed = await signArchive({ directory, privateKeyPem, keyId: 'test-only', downloadUrl: plan[0].download_url });
  assert.equal(verifyArchive({ ...signed, publicKeyPem }), true);
  assert.equal(verifyArchive({ ...signed, publicKeyPem: generateKeyPairSync('ed25519').publicKey }), false);
  await assert.rejects(updateCatalog({ repoRoot: root, records: [{ ...signed.record, name: 'altered metadata' }], publicKeyPem }), /match source/);
  assert.equal(verifyArchive({ bytes: Buffer.concat([signed.bytes, Buffer.from('tamper')]), record: signed.record, publicKeyPem }), false);
  await updateCatalog({ repoRoot: root, records: [signed.record], publicKeyPem });
  assert.deepEqual(await buildReleasePlan({ repoRoot: root }), []);
  const entry = JSON.parse(await fs.readFile(path.join(directory, 'catalog-entry.json'), 'utf8'));
  assert.equal(entry.checksum, signed.record.checksum); assert.equal(entry.source.kind, 'application_template_release'); assert.equal(entry.source.metadata.release_version, 2);
  const changed = fixture(2); changed.release.name = 'Changed'; await splitPackage(changed, directory);
  await assert.rejects(buildReleasePlan({ repoRoot: root }), /immutable/);
  await splitPackage(fixture(3), directory); plan = await buildReleasePlan({ repoRoot: root });
  signed = await signArchive({ directory, privateKeyPem, keyId: 'test-only', downloadUrl: plan[0].download_url });
  const history = await updateCatalog({ repoRoot: root, records: [signed.record], publicKeyPem }); assert.deepEqual(history.templates[0].versions.map(x => x.release_version), [2, 3]);
  await splitPackage(fixture(1), directory); await assert.rejects(buildReleasePlan({ repoRoot: root }), /increase/);
  const index = JSON.parse(await fs.readFile(path.join(root, 'applications-demo/catalog/v1/index.json'), 'utf8')); assert.ok(index);
});
test('applications-demo shares cursor pagination and search entries', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'template-pages-')); t.after(() => fs.rm(root, { recursive: true, force: true }));
  for (let i = 0; i < 3; i++) {
    const directory = path.join(root, `applications-demo/@test/demo${i}`); await fs.mkdir(directory, { recursive: true });
    await fs.writeFile(path.join(directory, 'catalog-entry.json'), JSON.stringify({ name: `Template ${i}`, version: '2', description: 'Gateway demo', host_version_requirement: '0.4.1', source: { kind: 'application_template_release', locator: `applications-demo/@test/demo${i}` }, checksum: null, signature: null, download_locator: { kind: 'https', locator: 'https://example.test/template.zip' } }));
  }
  const output = buildCategoryCatalog({ repoRoot: root, category: 'applications-demo', pageSize: 2 });
  assert.equal(output.searchIndexDocument.entries.length, 3);
  assert.equal(output.pageDocuments.length, 2);
  assert.equal(output.pageDocuments[0].document.next_cursor, output.pageDocuments[1].document.cursor);
  assert.equal(output.pageDocuments[1].document.next_cursor, null);
  assert.match(JSON.stringify(output.searchIndexDocument), /gateway demo/);
});
