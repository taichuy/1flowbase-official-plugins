import fs from 'node:fs/promises';
import path from 'node:path';
import { createPrivateKey, createPublicKey, sign, verify } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { buildArchive, readPackage, digest } from './archive.mjs';
import { updateExtensionCatalog } from '../extension-catalog.mjs';
export const RELEASE_SCHEMA = '1flowbase.application-template-catalog/v1';
const repository = 'taichuy/1flowbase-official-plugins';
const json = value => `${JSON.stringify(value, null, 2)}\n`;
const historyPath = root => path.join(root, 'applications-demo/releases/v1/catalog.json');
async function readHistory(root) {
  let value;
  try { value = JSON.parse(await fs.readFile(historyPath(root), 'utf8')); } catch (error) { if (error.code === 'ENOENT') return { schema_version: RELEASE_SCHEMA, generated_at: null, templates: [] }; throw error; }
  if (value.schema_version !== RELEASE_SCHEMA || !Array.isArray(value.templates)) throw new Error('invalid release history');
  const ids = new Set(); const paths = new Set();
  for (const entry of value.templates) {
    if (ids.has(entry.template_id) || paths.has(entry.source_path)) throw new Error('duplicate history identity');
    ids.add(entry.template_id); paths.add(entry.source_path);
    let previous = 0;
    for (const record of entry.versions) { validateRecord(record); if (record.template_id !== entry.template_id || record.release_version <= previous) throw new Error('invalid history order'); previous = record.release_version; }
  }
  return value;
}
function validateRelease(release) {
  if (!release || !/^@[a-zA-Z0-9_-]+\/[a-zA-Z0-9_-]+$/.test(release.template_id) || !Number.isSafeInteger(release.release_version) || release.release_version < 1) throw new Error('invalid template release identity');
  for (const field of ['name', 'description', 'exported_from_system_version', 'exported_at']) if (typeof release[field] !== 'string') throw new Error(`missing release ${field}`);
  if (Number.isNaN(Date.parse(release.exported_at))) throw new Error('invalid exported_at');
}
function validateRecord(record) {
  validateRelease(record);
  if (!/^sha256:[a-f0-9]{64}$/.test(record.checksum) || record.algorithm !== 'ed25519' || !record.key_id || !record.signature || Buffer.from(record.signature, 'base64').length !== 64 || Buffer.from(record.signature, 'base64').toString('base64') !== record.signature) throw new Error('invalid signature record');
  if (!/^https:\/\/github\.com\/taichuy\/1flowbase-official-plugins\/releases\/download\/[^/]+\/[^/]+\.zip$/.test(record.download_url)) throw new Error('release URL must be immutable ZIP');
}
export async function discoverTemplates(repoRoot) {
  const root = path.join(repoRoot, 'applications-demo'); const result = [];
  for (const organization of await fs.readdir(root, { withFileTypes: true })) {
    if (!organization.isDirectory() || !organization.name.startsWith('@')) continue;
    for (const artifact of await fs.readdir(path.join(root, organization.name), { withFileTypes: true })) {
      if (!artifact.isDirectory()) continue;
      const directory = path.join(root, organization.name, artifact.name);
      const packageValue = await readPackage(directory); validateRelease(packageValue.release);
      if (packageValue.release.template_id !== `${organization.name}/${artifact.name}`) throw new Error('template identity must match source directory');
      const bytes = await buildArchive(directory);
      result.push({ organization: organization.name.slice(1), artifact: artifact.name, source_path: path.relative(repoRoot, directory).split(path.sep).join('/'), directory, release: packageValue.release, checksum: digest(bytes) });
    }
  }
  return result.sort((a, b) => a.source_path.localeCompare(b.source_path));
}
export async function buildReleasePlan({ repoRoot }) {
  const history = await readHistory(repoRoot); const pending = [];
  for (const source of await discoverTemplates(repoRoot)) {
    const { template_id, release_version } = source.release;
    const entry = history.templates.find(x => x.template_id === template_id || x.source_path === source.source_path);
    if (entry && (entry.template_id !== template_id || entry.source_path !== source.source_path)) throw new Error('source identity cannot change');
    const previous = entry?.versions ?? [];
    const existing = previous.find(x => x.release_version === release_version);
    if (existing) { if (existing.checksum !== source.checksum) throw new Error('immutable release conflict'); continue; }
    if (previous.some(x => x.release_version >= release_version)) throw new Error('release version must increase');
    const stem = `${source.organization}-${source.artifact}-v${release_version}`;
    const release_tag = `application-template-${stem}`; const asset_name = `${stem}.zip`;
    pending.push({ ...source, template_id, release_version, release_tag, asset_name, download_url: `https://github.com/${repository}/releases/download/${release_tag}/${asset_name}` });
  }
  return pending;
}
export async function signArchive({ directory, privateKeyPem, keyId, downloadUrl }) {
  if (!keyId) throw new Error('keyId is required');
  const bytes = await buildArchive(directory); const release = (await readPackage(directory)).release; validateRelease(release);
  const key = createPrivateKey(privateKeyPem); if (key.asymmetricKeyType !== 'ed25519') throw new Error('Ed25519 key required');
  const record = { ...release, download_url: downloadUrl, checksum: digest(bytes), algorithm: 'ed25519', key_id: keyId, signature: sign(null, bytes, key).toString('base64') };
  validateRecord(record);
  if (!verifyArchive({ bytes, record, publicKeyPem: createPublicKey(key) })) throw new Error('signature verification failed');
  return { bytes, record };
}
export function verifyArchive({ bytes, record, publicKeyPem }) {
  try { validateRecord(record); const key = publicKeyPem?.type === 'public' ? publicKeyPem : createPublicKey(publicKeyPem); return key.asymmetricKeyType === 'ed25519' && digest(bytes) === record.checksum && verify(null, bytes, key, Buffer.from(record.signature, 'base64')); } catch { return false; }
}
export function catalogEntry(source, record) {
  return { name: record.name, version: String(record.release_version), description: record.description, host_version_requirement: record.exported_from_system_version, source: { kind: 'application_template_release', locator: source.source_path, metadata: { template_id: record.template_id, release_version: record.release_version } }, signature: { algorithm: record.algorithm, key_id: record.key_id, signature: record.signature }, checksum: record.checksum, download_locator: { kind: 'https', locator: record.download_url }, slot_codes: [], keywords: ['application', 'template'] };
}
export async function updateCatalog({ repoRoot, records, publicKeyPem, generatedAt = new Date().toISOString() }) {
  const history = await readHistory(repoRoot); const sources = await discoverTemplates(repoRoot); const writes = [];
  for (const record of records) {
    validateRecord(record);
    const source = sources.find(x => x.release.template_id === record.template_id);
    if (!source || source.release.release_version !== record.release_version || source.checksum !== record.checksum || Object.entries(source.release).some(([key, value]) => JSON.stringify(record[key]) !== JSON.stringify(value))) throw new Error('signed record does not match source');
    if (!verifyArchive({ bytes: await buildArchive(source.directory), record, publicKeyPem })) throw new Error('invalid release signature');
    const expectedStem = `${source.organization}-${source.artifact}-v${record.release_version}`;
    if (record.download_url !== `https://github.com/${repository}/releases/download/application-template-${expectedStem}/${expectedStem}.zip`) throw new Error('release asset identity mismatch');
    let entry = history.templates.find(x => x.template_id === record.template_id || x.source_path === source.source_path);
    if (entry && (entry.template_id !== record.template_id || entry.source_path !== source.source_path)) throw new Error('source identity cannot change');
    if (!entry) { entry = { template_id: record.template_id, organization: source.organization, artifact: source.artifact, source_path: source.source_path, versions: [] }; history.templates.push(entry); }
    const existing = entry.versions.find(x => x.release_version === record.release_version);
    if (existing) { if (existing.checksum !== record.checksum) throw new Error('immutable release conflict'); }
    else { if (entry.versions.some(x => x.release_version >= record.release_version)) throw new Error('release version must increase'); entry.versions.push(record); }
    writes.push([path.join(source.directory, 'catalog-entry.json'), catalogEntry(source, existing ?? record)]);
  }
  history.generated_at = generatedAt; history.templates.sort((a, b) => a.template_id.localeCompare(b.template_id));
  writes.push([historyPath(repoRoot), history]);
  for (const [filename, value] of writes) { await fs.mkdir(path.dirname(filename), { recursive: true }); await fs.writeFile(`${filename}.tmp`, json(value)); await fs.rename(`${filename}.tmp`, filename); }
  updateExtensionCatalog({ repoRoot, categories: ['applications-demo'] });
  return history;
}
function option(name, fallback) { const index = process.argv.indexOf(name); if (index < 0) { if (fallback !== undefined) return fallback; throw new Error(`missing ${name}`); } return process.argv[index + 1]; }
if (path.resolve(process.argv[1] || '') === fileURLToPath(import.meta.url)) {
  const command = process.argv[2];
  if (command === 'plan') process.stdout.write(JSON.stringify(await buildReleasePlan({ repoRoot: path.resolve(option('--repo-root', '.')) })));
  else if (command === 'sign') {
    const result = await signArchive({ directory: option('--directory'), privateKeyPem: await fs.readFile(option('--private-key'), 'utf8'), keyId: option('--key-id'), downloadUrl: option('--download-url') });
    await fs.writeFile(option('--archive'), result.bytes); await fs.writeFile(option('--output'), json(result.record));
  } else if (command === 'update') {
    const records = []; const dir = option('--records-dir');
    for (const name of (await fs.readdir(dir)).filter(x => x.endsWith('.json')).sort()) records.push(JSON.parse(await fs.readFile(path.join(dir, name), 'utf8')));
    await updateCatalog({ repoRoot: path.resolve(option('--repo-root', '.')), records, publicKeyPem: await fs.readFile(option('--public-key'), 'utf8') });
  } else throw new Error('expected plan, sign, or update');
}
