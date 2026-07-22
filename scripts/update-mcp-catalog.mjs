import { createHash } from 'node:crypto';
import {
  existsSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync
} from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCHEMA_VERSIONS = new Set([
  '1flowbase.mcp.bundle/v1',
  '1flowbase.mcp.bundle/v2'
]);
const REPOSITORY = 'taichuy/1flowbase-official-plugins';
const LOCALES = new Set(['zh_Hans', 'en_US']);

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, 'utf8'));
}

function sha256(bytes) {
  return `sha256:${createHash('sha256').update(bytes).digest('hex')}`;
}

function assertSemver(value, field) {
  if (typeof value !== 'string' || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(value)) {
    throw new Error(`${field} must be semantic version`);
  }
}

function jsonFiles(root, relativeRoot) {
  if (!existsSync(root)) return [];
  return readdirSync(root, { withFileTypes: true })
    .flatMap((entry) => {
      const absolute = path.join(root, entry.name);
      if (entry.isDirectory()) return jsonFiles(absolute, relativeRoot);
      if (!entry.isFile() || !entry.name.endsWith('.json')) return [];
      return [path.relative(relativeRoot, absolute).split(path.sep).join('/')];
    })
    .sort();
}

export function buildMcpBundleSource(bundleRoot) {
  const manifestPath = path.join(bundleRoot, 'manifest.json');
  const manifest = readJson(manifestPath);
  if (!SCHEMA_VERSIONS.has(manifest.schema_version)) {
    throw new Error(`unsupported MCP bundle schema in ${manifestPath}`);
  }
  const expectedOrganization = path.basename(path.dirname(bundleRoot));
  const expectedBundleId = path.basename(bundleRoot);
  if (manifest.organization !== expectedOrganization || manifest.bundle_id !== expectedBundleId) {
    throw new Error(`MCP bundle path identity mismatch in ${manifestPath}`);
  }
  assertSemver(manifest.bundle_version, 'bundle_version');
  assertSemver(manifest.minimum_host_version, 'minimum_host_version');
  assertSemver(manifest.exported_from_system_version, 'exported_from_system_version');
  if (!LOCALES.has(manifest.locale)) {
    throw new Error(`unsupported MCP bundle locale ${manifest.locale}`);
  }

  const toolPaths = jsonFiles(path.join(bundleRoot, 'tools'), bundleRoot);
  const instancePaths = jsonFiles(path.join(bundleRoot, 'instances'), bundleRoot);
  const connectionPaths = jsonFiles(path.join(bundleRoot, 'connections'), bundleRoot);
  const toolIds = new Set();
  const upstreamConnectionIds = new Set();
  for (const relativePath of toolPaths) {
    const tool = readJson(path.join(bundleRoot, relativePath));
    const isV1Target = manifest.schema_version === '1flowbase.mcp.bundle/v1' && tool.interface_id;
    const isInterfaceTarget =
      tool.execution_target?.kind === 'interface_wrapper' && tool.execution_target.interface_id;
    const isProxyTarget =
      tool.execution_target?.kind === 'mcp_proxy' &&
      tool.execution_target.upstream_connection_id &&
      tool.execution_target.remote_tool_name &&
      tool.execution_target.source_schema_hash;
    if (!tool.tool_id || (!isV1Target && !isInterfaceTarget && !isProxyTarget) || toolIds.has(tool.tool_id)) {
      throw new Error(`invalid or duplicate MCP tool identity in ${relativePath}`);
    }
    toolIds.add(tool.tool_id);
    if (isProxyTarget) upstreamConnectionIds.add(tool.execution_target.upstream_connection_id);
  }
  const instanceIds = new Set();
  for (const relativePath of instancePaths) {
    const instance = readJson(path.join(bundleRoot, relativePath));
    if (!instance.instance_id || instanceIds.has(instance.instance_id)) {
      throw new Error(`invalid or duplicate MCP instance identity in ${relativePath}`);
    }
    instanceIds.add(instance.instance_id);
    for (const binding of instance.bindings ?? []) {
      if (!toolIds.has(binding.tool_id)) {
        throw new Error(`MCP binding ${binding.tool_id} is not declared by this bundle`);
      }
    }
  }
  const connectionIds = new Set();
  for (const relativePath of connectionPaths) {
    const connection = readJson(path.join(bundleRoot, relativePath));
    if (!connection.connection_id || connectionIds.has(connection.connection_id)) {
      throw new Error(`invalid or duplicate MCP connection identity in ${relativePath}`);
    }
    connectionIds.add(connection.connection_id);
  }
  for (const connectionId of upstreamConnectionIds) {
    if (!connectionIds.has(connectionId)) {
      throw new Error(`MCP proxy references undeclared connection ${connectionId}`);
    }
  }

  const files = [
    ...toolPaths.map((relativePath) => ({ path: relativePath, kind: 'tool' })),
    ...instancePaths.map((relativePath) => ({ path: relativePath, kind: 'instance' })),
    ...connectionPaths.map((relativePath) => ({ path: relativePath, kind: 'connection' }))
  ].map((entry) => ({
    ...entry,
    sha256: sha256(readFileSync(path.join(bundleRoot, entry.path)))
  }));

  return { manifest: { ...manifest, files }, bundleRoot };
}

function bundleRoots(repositoryRoot) {
  const mcpRoot = path.join(repositoryRoot, 'mcp');
  if (!existsSync(mcpRoot)) return [];
  return readdirSync(mcpRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .flatMap((organization) => {
      const organizationRoot = path.join(mcpRoot, organization.name);
      return readdirSync(organizationRoot, { withFileTypes: true })
        .filter((entry) => entry.isDirectory())
        .map((entry) => path.join(organizationRoot, entry.name));
    })
    .filter((root) => statSync(path.join(root, 'manifest.json')).isFile())
    .sort();
}

export function buildMcpCatalog(repositoryRoot, options = {}) {
  const bundles = bundleRoots(repositoryRoot).map((bundleRoot) => {
    const { manifest } = buildMcpBundleSource(bundleRoot);
    const releaseTag = `mcp-${manifest.organization}-${manifest.bundle_id}-v${manifest.bundle_version}`;
    const assetName = `${manifest.organization}-${manifest.bundle_id}-v${manifest.bundle_version}.zip`;
    const entry = {
      organization: manifest.organization,
      bundle_id: manifest.bundle_id,
      latest_version: manifest.bundle_version,
      locale: manifest.locale,
      minimum_host_version: manifest.minimum_host_version,
      exported_from_system_version: manifest.exported_from_system_version,
      release_tag: releaseTag,
      download_url: `https://github.com/${REPOSITORY}/releases/download/${releaseTag}/${assetName}`
    };
    const artifactSha256 = options.artifactSha256ByReleaseTag?.get(releaseTag);
    return artifactSha256 ? { ...entry, artifact_sha256: artifactSha256 } : entry;
  });
  return {
    version: 1,
    generated_at: options.generatedAt ?? new Date().toISOString(),
    bundles
  };
}

export function updateMcpCatalog(repositoryRoot, options = {}) {
  const catalogPath = path.join(repositoryRoot, 'mcp', 'catalog.json');
  const artifactSha256ByReleaseTag = new Map();
  if (existsSync(catalogPath)) {
    for (const entry of readJson(catalogPath).bundles ?? []) {
      if (entry.release_tag && entry.artifact_sha256) {
        artifactSha256ByReleaseTag.set(entry.release_tag, entry.artifact_sha256);
      }
    }
  }
  for (const bundleRoot of bundleRoots(repositoryRoot)) {
    const { manifest } = buildMcpBundleSource(bundleRoot);
    writeFileSync(
      path.join(bundleRoot, 'manifest.json'),
      `${JSON.stringify(manifest, null, 2)}\n`
    );
  }
  const catalog = buildMcpCatalog(repositoryRoot, {
    ...options,
    artifactSha256ByReleaseTag
  });
  writeFileSync(
    catalogPath,
    `${JSON.stringify(catalog, null, 2)}\n`
  );
  return catalog;
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : null;
if (invokedPath === fileURLToPath(import.meta.url)) {
  const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  updateMcpCatalog(repositoryRoot);
}
