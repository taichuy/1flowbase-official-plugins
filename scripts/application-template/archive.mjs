import fs from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';
export const ARCHIVE_SCHEMA = '1flowbase.application-template-archive/v1';
export const digest = bytes => `sha256:${createHash('sha256').update(bytes).digest('hex')}`;
const json = value => Buffer.from(`${JSON.stringify(value, null, 2)}\n`);
function safe(value) {
  if (typeof value !== 'string' || !value || value.includes('\\') || value.startsWith('/') || value.split('/').some(x => !x || x === '.' || x === '..') || /^[A-Za-z]:/.test(value)) throw new Error(`unsafe archive path: ${value}`);
  return value;
}
const component = value => encodeURIComponent(String(value)).replace(/\./g, '%2E');
export async function splitPackage(packageValue, directory) {
  if (!['1flowbase.portable-template/v1', '1flowbase.portable-template/v2'].includes(packageValue?.schema_version)) throw new Error('invalid portable template schema');
  const files = new Map();
  const put = (name, value) => { safe(name); if (files.has(name)) throw new Error(`duplicate path ${name}`); files.set(name, json(value)); return { $file: name }; };
  const p = structuredClone(packageValue);
  p.pages = p.pages.map(page => {
    const prefix = `pages/${component(page.id)}`;
    page.tabs = page.tabs.map(tab => {
      tab.document_payload = put(`${prefix}/tabs/${component(tab.id)}/document.json`, tab.document_payload);
      return put(`${prefix}/tabs/${component(tab.id)}.json`, tab);
    });
    return put(`${prefix}/page.json`, page);
  });
  p.applications = p.applications.map(app => {
    const prefix = `applications/${component(app.id)}`;
    app.flow_document = put(`${prefix}/flow-document.json`, app.flow_document);
    if (app.published) app.published.flow_document = put(`${prefix}/published-flow-document.json`, app.published.flow_document);
    return put(`${prefix}/application.json`, app);
  });
  p.data_models = p.data_models.map(model => put(`data-models/${component(model.id)}.json`, model));
  p.plugins = put('plugins/dependencies.json', p.plugins);
  if (p.i18n_entries !== undefined) {
    if (!Array.isArray(p.i18n_entries)) throw new Error('invalid template translations');
    p.i18n_entries = p.i18n_entries.map(entry => {
      if (!entry || typeof entry.key !== 'string' || !entry.key.trim() ||
          typeof entry.locale !== 'string' || !/^[a-z]{2,3}(_[A-Z][A-Za-z]{1,7})?$/.test(entry.locale) ||
          typeof entry.translation !== 'string') throw new Error('invalid template translation');
      const identity = createHash('sha256').update(JSON.stringify([entry.key, entry.locale])).digest('hex');
      return put(`i18n/${entry.locale}/${identity.slice(0, 2)}/${identity}.json`, entry);
    });
  }

  if (p.mcp_bundle) {
    p.mcp_bundle.manifest = put('mcp/manifest.json', p.mcp_bundle.manifest);
    for (const [kind, key] of [['tools', 'tool_id'], ['instances', 'instance_id'], ['connections', 'connection_id']]) {
      p.mcp_bundle[kind] = p.mcp_bundle[kind].map(value => {
        const prefix = kind === 'tools' ? `${createHash('sha256').update(value[key]).digest('hex').slice(0, 2)}/` : '';
        return put(`mcp/${kind}/${prefix}${component(value[key])}.json`, value);
      });
    }
  }
  const manifest = { schema_version: ARCHIVE_SCHEMA, package: p, files: [...files].sort(([a], [b]) => a < b ? -1 : 1).map(([name, bytes]) => ({ path: name, sha256: digest(bytes) })) };
  for (const [name, bytes] of files) { const target = path.join(directory, name); await fs.mkdir(path.dirname(target), { recursive: true }); await fs.writeFile(target, bytes); }
  await fs.mkdir(directory, { recursive: true });
  await fs.writeFile(path.join(directory, 'manifest.json'), json(manifest));
  return manifest;
}
async function load(directory) {
  const root = await fs.realpath(directory);
  const read = async name => {
    safe(name);
    const target = path.join(root, name);
    let current = root;
    for (const part of name.split('/')) { current = path.join(current, part); if ((await fs.lstat(current)).isSymbolicLink()) throw new Error('symlink not allowed'); }
    const stat = await fs.stat(target); if (!stat.isFile() || stat.size > 32 * 1024 * 1024) throw new Error('invalid archive file');
    return fs.readFile(target);
  };
  const manifestBytes = await read('manifest.json');
  const manifest = JSON.parse(manifestBytes);
  if (manifest.schema_version !== ARCHIVE_SCHEMA || !Array.isArray(manifest.files) || manifest.files.length > 10000) throw new Error('invalid archive manifest');
  const files = new Map([['manifest.json', manifestBytes]]);
  let total = manifestBytes.length;
  for (const entry of manifest.files) {
    safe(entry.path);
    if (files.has(entry.path)) throw new Error('duplicate archive file');
    const bytes = await read(entry.path);
    total += bytes.length; if (total > 128 * 1024 * 1024) throw new Error('archive size limit');
    if (digest(bytes) !== entry.sha256) throw new Error(`hash mismatch: ${entry.path}`);
    files.set(entry.path, bytes);
  }
  const used = new Set();
  function resolve(value, stack = [], depth = 0) {
    if (depth > 128) throw new Error('archive depth limit');
    if (Array.isArray(value)) return value.map(v => resolve(v, stack, depth + 1));
    if (!value || typeof value !== 'object') return value;
    if (Object.keys(value).length === 1 && '$file' in value) {
      const name = safe(value.$file);
      if (name === 'manifest.json' || !files.has(name)) throw new Error('undeclared reference');
      if (stack.includes(name)) throw new Error('reference cycle');
      used.add(name);
      return resolve(JSON.parse(files.get(name)), [...stack, name], depth + 1);
    }
    return Object.fromEntries(Object.entries(value).map(([key, v]) => [key, resolve(v, stack, depth + 1)]));
  }
  const packageValue = resolve(manifest.package);
  if (!['1flowbase.portable-template/v1', '1flowbase.portable-template/v2'].includes(packageValue?.schema_version)) throw new Error('invalid portable template schema');
  if (used.size !== manifest.files.length) throw new Error('unreferenced archive file');
  return { files, packageValue };
}
export async function readPackage(directory) { return (await load(directory)).packageValue; }
function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) { crc ^= byte; for (let i = 0; i < 8; i++) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1)); }
  return (crc ^ 0xffffffff) >>> 0;
}
export async function buildArchive(directory) {
  const { files } = await load(directory);
  const local = [], central = []; let offset = 0;
  for (const [name, bytes] of [...files].sort(([a], [b]) => a < b ? -1 : 1)) {
    const filename = Buffer.from(name); const crc = crc32(bytes);
    const h = Buffer.alloc(30); h.writeUInt32LE(0x04034b50); h.writeUInt16LE(20, 4); h.writeUInt16LE(0x800, 6); h.writeUInt16LE(33, 12); h.writeUInt32LE(crc, 14); h.writeUInt32LE(bytes.length, 18); h.writeUInt32LE(bytes.length, 22); h.writeUInt16LE(filename.length, 26);
    local.push(h, filename, bytes);
    const c = Buffer.alloc(46); c.writeUInt32LE(0x02014b50); c.writeUInt16LE(0x0314, 4); c.writeUInt16LE(20, 6); c.writeUInt16LE(0x800, 8); c.writeUInt16LE(33, 14); c.writeUInt32LE(crc, 16); c.writeUInt32LE(bytes.length, 20); c.writeUInt32LE(bytes.length, 24); c.writeUInt16LE(filename.length, 28); c.writeUInt32LE((0o100644 << 16) >>> 0, 38); c.writeUInt32LE(offset, 42);
    central.push(c, filename); offset += h.length + filename.length + bytes.length;
  }
  const cbytes = Buffer.concat(central); const end = Buffer.alloc(22); end.writeUInt32LE(0x06054b50); end.writeUInt16LE(files.size, 8); end.writeUInt16LE(files.size, 10); end.writeUInt32LE(cbytes.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...local, cbytes, end]);
}
