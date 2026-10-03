import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { splitPackage, readPackage, buildArchive, digest } from '../archive.mjs';
export function fixture(version = 2) {
  return { schema_version: '1flowbase.portable-template/v1', release: { template_id: '@test/demo', release_version: version, name: 'Demo', description: '中文', exported_from_system_version: '0.4.1', exported_at: '2026-10-03T00:00:00Z' }, pages: [{ id: 'page', tabs: [{ id: 'tab', document_payload: { title: 'Unicode 中文' }, blocks: [] }] }], applications: [{ id: 'app', flow_document: { nodes: [] }, published: { flow_document: { nodes: [1] } } }], data_models: [{ id: 'model', fields: [] }], plugins: [{ plugin_id: 'test@1.0.0' }], mcp_bundle: { manifest: { name: 'MCP' }, tools: [{ tool_id: 'strange/id..', config: { value: 1 } }], instances: [{ instance_id: 'test', bindings: [] }], connections: [{ connection_id: 'connection' }] } };
}
async function temporary(t) { const root = await fs.mkdtemp(path.join(os.tmpdir(), 'application-archive-')); t.after(() => fs.rm(root, { recursive: true, force: true })); return root; }
test('split source preserves entire package and deterministic ZIP, excludes local metadata', async t => {
  const root = await temporary(t), p = fixture(); const manifest = await splitPackage(p, root);
  assert.deepEqual(await readPackage(root), p);
  assert.deepEqual(manifest.files.map(x => x.path), [...manifest.files.map(x => x.path)].sort());
  await fs.writeFile(path.join(root, 'export.config.json'), '{"local":true}');
  const a = await buildArchive(root); await fs.writeFile(path.join(root, 'README.md'), 'changed'); const b = await buildArchive(root);
  assert.deepEqual(a, b);
  let offset = 0; const names = []; const contents = new Map();
  while (a.readUInt32LE(offset) === 0x04034b50) {
    assert.equal(a.readUInt16LE(offset + 8), 0); assert.equal(a.readUInt16LE(offset + 10), 0); assert.equal(a.readUInt16LE(offset + 12), 33);
    const size = a.readUInt32LE(offset + 18), n = a.readUInt16LE(offset + 26), extra = a.readUInt16LE(offset + 28);
    const name = a.subarray(offset + 30, offset + 30 + n).toString(); const start = offset + 30 + n + extra;
    names.push(name); contents.set(name, a.subarray(start, start + size)); offset = start + size;
  }
  assert.deepEqual(names, ['manifest.json', ...manifest.files.map(x => x.path)].sort());
  assert.deepEqual(JSON.parse(contents.get('manifest.json')), manifest);
  for (const file of manifest.files) assert.equal(digest(contents.get(file.path)), file.sha256);
});
test('tampered file, traversal, unused reference, cycles and symlinks are rejected', async t => {
  const root = await temporary(t); const p = fixture();
  let manifest = await splitPackage(p, root);
  await fs.appendFile(path.join(root, manifest.files[0].path), ' ');
  await assert.rejects(readPackage(root), /hash mismatch/);
  manifest = await splitPackage(p, root); manifest.files[0].path = '../outside.json';
  await fs.writeFile(path.join(root, 'manifest.json'), JSON.stringify(manifest)); await assert.rejects(readPackage(root), /unsafe/);
  manifest = await splitPackage(p, root); manifest.package.plugins = [];
  await fs.writeFile(path.join(root, 'manifest.json'), JSON.stringify(manifest)); await assert.rejects(readPackage(root), /unreferenced/);
  manifest = await splitPackage(p, root); const entry = manifest.files.find(x => x.path === 'plugins/dependencies.json');
  const cycle = Buffer.from(JSON.stringify({ $file: entry.path })); entry.sha256 = digest(cycle);
  await fs.writeFile(path.join(root, entry.path), cycle); await fs.writeFile(path.join(root, 'manifest.json'), JSON.stringify(manifest));
  await assert.rejects(readPackage(root), /cycle/);
  manifest = await splitPackage(p, root); const file = manifest.files[0].path;
  await fs.rename(path.join(root, file), path.join(root, 'outside.json')); await fs.symlink(path.join(root, 'outside.json'), path.join(root, file));
  await assert.rejects(readPackage(root), /symlink/);
});
test('duplicate, undeclared and manifest self references are rejected', async t => {
  const root = await temporary(t);
  let manifest = await splitPackage(fixture(), root); manifest.files.push(manifest.files[0]);
  await fs.writeFile(path.join(root, 'manifest.json'), JSON.stringify(manifest)); await assert.rejects(readPackage(root), /duplicate/);
  manifest = await splitPackage(fixture(), root); manifest.package.plugins = { $file: 'missing.json' };
  await fs.writeFile(path.join(root, 'manifest.json'), JSON.stringify(manifest)); await assert.rejects(readPackage(root), /undeclared/);
  manifest.package.plugins = { $file: 'manifest.json' };
  await fs.writeFile(path.join(root, 'manifest.json'), JSON.stringify(manifest)); await assert.rejects(readPackage(root), /undeclared/);
});
