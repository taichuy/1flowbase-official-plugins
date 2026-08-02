import {
  createHash,
  createPrivateKey,
  createPublicKey,
  sign,
  verify,
} from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const MCP_CATALOG_SCHEMA = '1flowbase.mcp-catalog/v2';
export const MCP_HISTORY_SCHEMA = '1flowbase.mcp-release-history/v1';
const SCHEMA_VERSIONS = new Set(['1flowbase.mcp.bundle/v1', '1flowbase.mcp.bundle/v2']);
const RELEASE_REPOSITORY = 'taichuy/1flowbase-official-plugins';
const LOCALES = new Set(['zh_Hans', 'en_US']);

const json = (value) => `${JSON.stringify(value, null, 2)}\n`;
const sha256 = (bytes) => `sha256:${createHash('sha256').update(bytes).digest('hex')}`;
const posix = (value) => value.split(path.sep).join('/');
const compareText = (left, right) => left < right ? -1 : left > right ? 1 : 0;

function assertString(value, field) {
  if (typeof value !== 'string' || !value) throw new Error(`${field} must be a non-empty string`);
}

function assertSemver(value, field) {
  if (typeof value !== 'string' || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(value)) {
    throw new Error(`${field} must be semantic version`);
  }
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function jsonFiles(root, relativeRoot) {
  if (!fs.existsSync(root)) return [];
  return fs.readdirSync(root, { withFileTypes: true }).flatMap((entry) => {
    const absolute = path.join(root, entry.name);
    if (entry.isDirectory()) return jsonFiles(absolute, relativeRoot);
    if (!entry.isFile() || !entry.name.endsWith('.json')) return [];
    return [posix(path.relative(relativeRoot, absolute))];
  }).sort(compareText);
}

export function buildMcpBundleSource(bundleRoot) {
  const manifestPath = path.join(bundleRoot, 'manifest.json');
  const manifest = readJson(manifestPath);
  if (!SCHEMA_VERSIONS.has(manifest.schema_version)) throw new Error(`unsupported MCP bundle schema in ${manifestPath}`);
  const organizationDirectory = path.basename(path.dirname(bundleRoot));
  if (!organizationDirectory.startsWith('@')) throw new Error(`MCP bundle must use an @organization directory: ${manifestPath}`);
  if (manifest.organization !== organizationDirectory.slice(1) || manifest.bundle_id !== path.basename(bundleRoot)) {
    throw new Error(`MCP bundle path identity mismatch in ${manifestPath}`);
  }
  assertSemver(manifest.bundle_version, 'bundle_version');
  assertSemver(manifest.minimum_host_version, 'minimum_host_version');
  assertSemver(manifest.exported_from_system_version, 'exported_from_system_version');
  if (manifest.minimum_host_version !== manifest.exported_from_system_version) {
    throw new Error('minimum_host_version must equal exported_from_system_version');
  }
  if (!LOCALES.has(manifest.locale)) throw new Error(`unsupported MCP bundle locale ${manifest.locale}`);

  const toolPaths = jsonFiles(path.join(bundleRoot, 'tools'), bundleRoot);
  const instancePaths = jsonFiles(path.join(bundleRoot, 'instances'), bundleRoot);
  const connectionPaths = jsonFiles(path.join(bundleRoot, 'connections'), bundleRoot);
  const toolIds = new Set();
  const upstreamConnectionIds = new Set();
  for (const relativePath of toolPaths) {
    const tool = readJson(path.join(bundleRoot, relativePath));
    const v1 = manifest.schema_version === '1flowbase.mcp.bundle/v1' && tool.interface_id;
    const wrapper = tool.execution_target?.kind === 'interface_wrapper' && tool.execution_target.interface_id;
    const proxy = tool.execution_target?.kind === 'mcp_proxy' && tool.execution_target.upstream_connection_id && tool.execution_target.remote_tool_name && tool.execution_target.source_schema_hash;
    if (!tool.tool_id || (!v1 && !wrapper && !proxy) || toolIds.has(tool.tool_id)) throw new Error(`invalid or duplicate MCP tool identity in ${relativePath}`);
    toolIds.add(tool.tool_id);
    if (proxy) upstreamConnectionIds.add(tool.execution_target.upstream_connection_id);
  }
  const instanceIds = new Set();
  for (const relativePath of instancePaths) {
    const instance = readJson(path.join(bundleRoot, relativePath));
    if (!instance.instance_id || instanceIds.has(instance.instance_id)) throw new Error(`invalid or duplicate MCP instance identity in ${relativePath}`);
    instanceIds.add(instance.instance_id);
    for (const binding of instance.bindings ?? []) {
      if (!toolIds.has(binding.tool_id)) throw new Error(`MCP binding ${binding.tool_id} is not declared by this bundle`);
    }
  }
  const connectionIds = new Set();
  for (const relativePath of connectionPaths) {
    const connection = readJson(path.join(bundleRoot, relativePath));
    if (!connection.connection_id || connectionIds.has(connection.connection_id)) throw new Error(`invalid or duplicate MCP connection identity in ${relativePath}`);
    connectionIds.add(connection.connection_id);
  }
  for (const connectionId of upstreamConnectionIds) {
    if (!connectionIds.has(connectionId)) throw new Error(`MCP proxy references undeclared connection ${connectionId}`);
  }
  const files = [
    ...toolPaths.map((relativePath) => ({ path: relativePath, kind: 'tool' })),
    ...instancePaths.map((relativePath) => ({ path: relativePath, kind: 'instance' })),
    ...connectionPaths.map((relativePath) => ({ path: relativePath, kind: 'connection' })),
  ].map((entry) => ({ ...entry, sha256: sha256(fs.readFileSync(path.join(bundleRoot, entry.path))) }));
  if (!Array.isArray(manifest.files) || JSON.stringify(manifest.files) !== JSON.stringify(files)) {
    throw new Error(`manifest files/checksums do not match source bytes in ${manifestPath}`);
  }
  return { manifest: { ...manifest, files }, bundleRoot };
}

export function signMcpArtifact({ artifactPath, bundleRoot, privateKeyPem, keyId, downloadUrl }) {
  const { manifest } = buildMcpBundleSource(bundleRoot);
  assertString(keyId, 'key_id');
  assertString(downloadUrl, 'download_url');
  const bytes = fs.readFileSync(artifactPath);
  const privateKey = createPrivateKey(privateKeyPem);
  if (privateKey.asymmetricKeyType !== 'ed25519') throw new Error('MCP signing key must be Ed25519');
  const signature = sign(null, bytes, privateKey).toString('base64');
  if (!verify(null, bytes, createPublicKey(privateKey), Buffer.from(signature, 'base64'))) {
    throw new Error('generated MCP signature failed verification');
  }
  return {
    bundle_version: manifest.bundle_version,
    locale: manifest.locale,
    minimum_host_version: manifest.minimum_host_version,
    exported_from_system_version: manifest.exported_from_system_version,
    release_tag: `mcp-${manifest.organization}-${manifest.bundle_id}-v${manifest.bundle_version}`,
    download_url: downloadUrl,
    checksum: sha256(bytes),
    algorithm: 'ed25519',
    key_id: keyId,
    signature,
  };
}

export function verifyMcpArtifact({ artifactBytes, record, publicKeyPem }) {
  return record.algorithm === 'ed25519' && sha256(artifactBytes) === record.checksum && verify(
    null, artifactBytes, createPublicKey(publicKeyPem), Buffer.from(record.signature, 'base64'),
  );
}

function validateRecord(record) {
  for (const field of ['bundle_version', 'minimum_host_version', 'exported_from_system_version']) assertSemver(record[field], field);
  if (!LOCALES.has(record.locale)) throw new Error(`unsupported MCP bundle locale ${record.locale}`);
  if (record.minimum_host_version !== record.exported_from_system_version) throw new Error('minimum_host_version must equal exported_from_system_version');
  if (!/^mcp-[^-]+-.+-v\d+\.\d+\.\d+/.test(record.release_tag)) throw new Error('invalid MCP release_tag');
  if (!/^https:\/\/github\.com\/[^/]+\/[^/]+\/releases\/download\/[^/]+\/[^/]+$/.test(record.download_url)) throw new Error('download_url must be an immutable GitHub Release asset URL');
  if (!/^sha256:[0-9a-f]{64}$/.test(record.checksum)) throw new Error('checksum must be sha256');
  if (record.algorithm !== 'ed25519') throw new Error("algorithm must be 'ed25519'");
  assertString(record.key_id, 'key_id');
  assertString(record.signature, 'signature');
  if (Buffer.from(record.signature, 'base64').toString('base64') !== record.signature) throw new Error('signature must be canonical base64');
}

function emptyHistory() {
  return { schema_version: MCP_HISTORY_SCHEMA, bundles: [] };
}

function readHistory(repoRoot) {
  const historyPath = path.join(repoRoot, 'mcp', '_maintenance', 'release-history.json');
  if (fs.existsSync(historyPath)) {
    const history = readJson(historyPath);
    if (history.schema_version !== MCP_HISTORY_SCHEMA || !Array.isArray(history.bundles)) throw new Error('unsupported MCP release history');
    return history;
  }
  // The former latest-only catalog remains useful as a version floor during the first signed release.
  const legacyPath = path.join(repoRoot, 'mcp', 'catalog.json');
  const legacy = fs.existsSync(legacyPath) ? readJson(legacyPath) : { bundles: [] };
  return { ...emptyHistory(), legacy_version_floor: Object.fromEntries((legacy.bundles ?? []).map((entry) => [`${entry.organization}/${entry.bundle_id}`, entry.latest_version])) };
}

export function updateMcpReleaseCatalog({ repoRoot, records, generatedAt = new Date().toISOString() }) {
  const history = readHistory(repoRoot);
  for (const { organization, bundle_id: bundleId, record } of records) {
    validateRecord(record);
    const sourceRoot = path.join(repoRoot, 'mcp', `@${organization}`, bundleId);
    const { manifest } = buildMcpBundleSource(sourceRoot);
    const tag = `mcp-${organization}-${bundleId}-v${record.bundle_version}`;
    const asset = `${organization}-${bundleId}-v${record.bundle_version}.zip`;
    const downloadUrl = `https://github.com/${RELEASE_REPOSITORY}/releases/download/${tag}/${asset}`;
    if (record.release_tag !== tag || record.download_url !== downloadUrl || manifest.bundle_version !== record.bundle_version || manifest.locale !== record.locale || manifest.minimum_host_version !== record.minimum_host_version || manifest.exported_from_system_version !== record.exported_from_system_version) {
      throw new Error(`release record does not match source manifest: ${organization}/${bundleId}`);
    }
    let bundle = history.bundles.find((entry) => entry.organization === organization && entry.bundle_id === bundleId);
    if (!bundle) {
      bundle = { organization, bundle_id: bundleId, source_path: posix(path.relative(repoRoot, path.join(sourceRoot, 'manifest.json'))), versions: [] };
      history.bundles.push(bundle);
    }
    const existing = bundle.versions.find((entry) => entry.bundle_version === record.bundle_version);
    if (existing) {
      if (existing.checksum !== record.checksum || existing.signature !== record.signature) throw new Error(`immutable MCP release conflict for ${organization}/${bundleId} v${record.bundle_version}`);
      continue;
    }
    const floor = history.legacy_version_floor?.[`${organization}/${bundleId}`];
    const versions = [...bundle.versions.map((entry) => entry.bundle_version), floor].filter(Boolean);
    if (versions.some((version) => version.localeCompare(record.bundle_version, undefined, { numeric: true }) >= 0)) {
      throw new Error(`bundle_version for ${organization}/${bundleId} must exceed published history`);
    }
    bundle.versions.push(record);
    bundle.versions.sort((left, right) => left.bundle_version.localeCompare(right.bundle_version, undefined, { numeric: true }));
  }
  history.bundles.sort((left, right) => compareText(`${left.organization}/${left.bundle_id}`, `${right.organization}/${right.bundle_id}`));
  const catalog = { schema_version: MCP_CATALOG_SCHEMA, generated_at: generatedAt, bundles: history.bundles };
  const historyPath = path.join(repoRoot, 'mcp', '_maintenance', 'release-history.json');
  fs.mkdirSync(path.dirname(historyPath), { recursive: true });
  for (const [filePath, document] of [[historyPath, history], [path.join(repoRoot, 'mcp', 'catalog.json'), catalog]]) {
    const temporary = `${filePath}.tmp-${process.pid}`;
    fs.writeFileSync(temporary, json(document));
    fs.renameSync(temporary, filePath);
  }
  return catalog;
}

function option(name) {
  const index = process.argv.indexOf(name);
  return index === -1 ? null : process.argv[index + 1];
}
function required(name) {
  const value = option(name);
  if (!value) throw new Error(`missing ${name}`);
  return value;
}

if (path.resolve(process.argv[1] || '') === fileURLToPath(import.meta.url)) {
  const command = process.argv[2];
  if (command === 'validate') {
    buildMcpBundleSource(path.resolve(required('--bundle-root')));
  } else if (command === 'sign') {
    const record = signMcpArtifact({
      artifactPath: path.resolve(required('--artifact')),
      bundleRoot: path.resolve(required('--bundle-root')),
      privateKeyPem: fs.readFileSync(required('--private-key'), 'utf8'),
      keyId: required('--key-id'),
      downloadUrl: required('--download-url'),
    });
    fs.writeFileSync(required('--output'), json(record));
  } else if (command === 'update') {
    const repoRoot = path.resolve(required('--repo-root'));
    const recordsRoot = path.resolve(required('--records-dir'));
    const records = fs.readdirSync(recordsRoot).filter((name) => name.endsWith('.json')).sort(compareText).map((name) => readJson(path.join(recordsRoot, name)));
    updateMcpReleaseCatalog({ repoRoot, records });
  } else {
    throw new Error('usage: update-mcp-catalog.mjs <validate|sign|update> [options]');
  }
}
