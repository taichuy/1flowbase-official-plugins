import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

export const CATALOG_CATEGORIES = Object.freeze([
  'agent-flow',
  'capability-plugins',
  'host-extensions',
  'i18n',
  'mcp',
  'runtime-extensions',
]);

export const CATALOG_SCHEMA_VERSION = '1flowbase.extension-catalog/v1';
export const CATALOG_STATE_SCHEMA_VERSION = '1flowbase.extension-catalog-state/v1';
export const DEFAULT_PAGE_SIZE = 100;
export const DEFAULT_RAW_BASE_URL =
  process.env.EXTENSION_CATALOG_RAW_BASE_URL ||
  'https://raw.githubusercontent.com/taichuy/1flowbase-official-plugins/main';

const ENTRY_FILE = 'catalog-entry.json';
const SOURCE_ENTRY_FIELDS = Object.freeze([
  'checksum',
  'description',
  'download_locator',
  'host_version_requirement',
  'name',
  'signature',
  'source',
  'version',
]);

function json(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function sha256(bytes) {
  return `sha256:${crypto.createHash('sha256').update(bytes).digest('hex')}`;
}

function posix(relativePath) {
  return relativePath.split(path.sep).join('/');
}

function relative(repoRoot, filePath) {
  return posix(path.relative(repoRoot, filePath));
}

function rawUrl(rawBaseUrl, relativePath) {
  return `${rawBaseUrl.replace(/\/+$/, '')}/${relativePath}`;
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function readJsonIfExists(filePath) {
  return fs.existsSync(filePath) ? readJson(filePath) : null;
}

function directoryNames(root) {
  if (!fs.existsSync(root)) return [];
  return fs.readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort(compareText);
}

function compareText(left, right) {
  if (left < right) return -1;
  if (left > right) return 1;
  return 0;
}

function cursorFor(entries, offset) {
  if (offset === 0) return 'start';
  return Buffer.from(`after:${entries[offset - 1].id}`, 'utf8').toString('base64url');
}

function catalogPaths(repoRoot, category) {
  const categoryRoot = path.join(repoRoot, category);
  const catalogRoot = path.join(categoryRoot, 'catalog', 'v1');
  return {
    categoryRoot,
    catalogRoot,
    pagesRoot: path.join(catalogRoot, 'pages'),
    indexPath: path.join(catalogRoot, 'index.json'),
    maintenanceRoot: path.join(categoryRoot, '_maintenance'),
    statePath: path.join(categoryRoot, '_maintenance', 'catalog-state.json'),
  };
}

function assertString(value, field, { allowEmpty = false } = {}) {
  if (typeof value !== 'string' || (!allowEmpty && value.length === 0)) {
    throw new Error(`${field} must be a${allowEmpty ? '' : ' non-empty'} string`);
  }
}

function normalizeEntry(category, organization, artifact, source) {
  const entry = {
    id: `${category}:${organization}/${artifact}`,
    name: source.name,
    category,
    organization,
    artifact,
    version: source.version,
    description: source.description,
    host_version_requirement: source.host_version_requirement,
    source: source.source,
    signature: source.signature ?? null,
    checksum: source.checksum ?? null,
    download_locator: source.download_locator,
  };
  for (const field of ['name', 'version', 'host_version_requirement']) {
    assertString(entry[field], `${entry.id}.${field}`);
  }
  assertString(entry.description, `${entry.id}.description`, { allowEmpty: true });
  if (!entry.source || typeof entry.source !== 'object') {
    throw new Error(`${entry.id}.source must be an object`);
  }
  assertString(entry.source.kind, `${entry.id}.source.kind`);
  assertString(entry.source.locator, `${entry.id}.source.locator`);
  if (!entry.download_locator || typeof entry.download_locator !== 'object') {
    throw new Error(`${entry.id}.download_locator must be an object`);
  }
  assertString(entry.download_locator.kind, `${entry.id}.download_locator.kind`);
  if (entry.checksum !== null && !/^sha256:[a-f0-9]{64}$/.test(entry.checksum)) {
    throw new Error(`${entry.id}.checksum must be null or a sha256 digest`);
  }
  return entry;
}

function discoverCanonicalEntries(repoRoot, category) {
  const root = path.join(repoRoot, category);
  const entries = [];
  for (const organizationDirectory of directoryNames(root).filter((name) => name.startsWith('@'))) {
    const organization = organizationDirectory.slice(1);
    for (const artifact of directoryNames(path.join(root, organizationDirectory))) {
      const manifestPath = path.join(root, organizationDirectory, artifact, ENTRY_FILE);
      if (!fs.existsSync(manifestPath)) continue;
      const source = readJson(manifestPath);
      const fields = Object.keys(source).sort();
      if (JSON.stringify(fields) !== JSON.stringify(SOURCE_ENTRY_FIELDS)) {
        throw new Error(`${relative(repoRoot, manifestPath)} fields must exactly match the v1 source entry contract`);
      }
      entries.push(normalizeEntry(category, organization, artifact, source));
    }
  }
  return entries;
}

function agentFlowEntries(repoRoot, rawBaseUrl) {
  const root = path.join(repoRoot, 'agent-flow', 'workflows');
  return directoryNames(root).flatMap((artifact) => {
    const templatePath = path.join(root, artifact, 'template.json');
    if (!fs.existsSync(templatePath)) return [];
    const bytes = fs.readFileSync(templatePath);
    const template = JSON.parse(bytes.toString('utf8'));
    if (template?.schema_version !== '1flowbase.application-template/v1' ||
        template?.application?.application_type !== 'agent_flow') {
      throw new Error(`invalid AgentFlow template ${relative(repoRoot, templatePath)}`);
    }
    const sourcePath = relative(repoRoot, templatePath);
    return [normalizeEntry('agent-flow', 'taichuy', artifact, {
      name: template.application.name || artifact,
      version: '1.0.0',
      description: template.application.description || '',
      host_version_requirement: '*',
      source: { kind: 'legacy_agent_flow_template', locator: sourcePath },
      signature: null,
      checksum: sha256(bytes),
      download_locator: { kind: 'repository_file', locator: rawUrl(rawBaseUrl, sourcePath) },
    })];
  });
}

function mcpEntries(repoRoot) {
  const catalogPath = path.join(repoRoot, 'mcp', 'catalog.json');
  const catalog = readJsonIfExists(catalogPath);
  if (!catalog) return [];
  return (catalog.bundles || []).map((bundle) => normalizeEntry(
    'mcp', bundle.organization, bundle.bundle_id, {
      name: bundle.bundle_id,
      version: bundle.latest_version,
      description: `Official ${bundle.locale} MCP bundle`,
      host_version_requirement: `>=${bundle.minimum_host_version}`,
      source: {
        kind: 'legacy_mcp_catalog',
        locator: 'mcp/catalog.json',
        release_tag: bundle.release_tag,
      },
      signature: null,
      checksum: bundle.artifact_sha256 ?? null,
      download_locator: { kind: 'release_asset', locator: bundle.download_url },
    }
  ));
}

function i18nEntries(repoRoot) {
  const catalogPath = path.join(repoRoot, 'i18n', 'catalog.json');
  const seedPath = path.join(repoRoot, 'i18n', 'dist', 'catalog-seed.json');
  if (!fs.existsSync(catalogPath) || !fs.existsSync(seedPath)) return [];
  const catalog = readJson(catalogPath);
  const seedBytes = fs.readFileSync(seedPath);
  const version = catalog.catalog_version;
  return [normalizeEntry('i18n', 'taichuy', 'platform', {
    name: '1flowbase platform translations',
    version,
    description: 'Official 1flowbase platform translation catalog',
    host_version_requirement: '*',
    source: { kind: 'legacy_i18n_catalog', locator: 'i18n/catalog.json' },
    signature: null,
    checksum: sha256(seedBytes),
    download_locator: {
      kind: 'release_asset',
      locator: `https://github.com/taichuy/1flowbase-official-plugins/releases/download/i18n-catalog-v${version}/i18n-catalog-seed-v${version}.json`,
    },
  })];
}

function runtimeEntries(repoRoot) {
  const registryPath = path.join(repoRoot, 'official-registry.json');
  const registry = readJsonIfExists(registryPath);
  if (!registry) return [];
  return (registry.plugins || []).map((plugin) => {
    const artifacts = (plugin.artifacts || []).map((artifact) => ({
      os: artifact.os,
      arch: artifact.arch,
      libc: artifact.libc ?? null,
      locator: artifact.download_url,
      checksum: artifact.checksum,
      signature: artifact.signature_algorithm && artifact.signing_key_id ? {
        algorithm: artifact.signature_algorithm,
        key_id: artifact.signing_key_id,
      } : null,
    }));
    const signaturePairs = new Set(artifacts
      .filter((artifact) => artifact.signature)
      .map((artifact) => `${artifact.signature.algorithm}:${artifact.signature.key_id}`));
    const description = plugin.i18n_summary?.bundles?.en_US?.plugin?.description || '';
    return normalizeEntry('runtime-extensions', '1flowbase', plugin.provider_code, {
      name: plugin.display_name,
      version: plugin.latest_version,
      description,
      host_version_requirement: `>=${plugin.minimum_host_version}`,
      source: {
        kind: 'legacy_official_registry',
        locator: 'official-registry.json',
        plugin_id: plugin.plugin_id,
      },
      signature: signaturePairs.size === 1 ? (() => {
        const [algorithm, keyId] = [...signaturePairs][0].split(':');
        return { algorithm, key_id: keyId };
      })() : null,
      checksum: null,
      download_locator: { kind: 'platform_release_assets', artifacts },
    });
  });
}

function legacyEntries(repoRoot, category, rawBaseUrl) {
  switch (category) {
    case 'agent-flow': return agentFlowEntries(repoRoot, rawBaseUrl);
    case 'i18n': return i18nEntries(repoRoot);
    case 'mcp': return mcpEntries(repoRoot);
    case 'runtime-extensions': return runtimeEntries(repoRoot);
    default: return [];
  }
}

export function discoverCatalogEntries({ repoRoot, category, rawBaseUrl = DEFAULT_RAW_BASE_URL }) {
  if (!CATALOG_CATEGORIES.includes(category)) throw new Error(`unsupported category ${category}`);
  const byId = new Map();
  // Legacy publishers remain readable, while the target @organization/artifact layout wins.
  for (const entry of legacyEntries(repoRoot, category, rawBaseUrl)) byId.set(entry.id, entry);
  for (const entry of discoverCanonicalEntries(repoRoot, category)) byId.set(entry.id, entry);
  return [...byId.values()].sort((left, right) => compareText(left.id, right.id));
}

function sourceFingerprint(entries) {
  return sha256(json(entries.map(({ catalog_page: _catalogPage, ...entry }) => entry)));
}

export function buildCategoryCatalog({
  repoRoot,
  category,
  pageSize = DEFAULT_PAGE_SIZE,
  rawBaseUrl = DEFAULT_RAW_BASE_URL,
  now = new Date(),
} = {}) {
  if (!repoRoot) throw new Error('repoRoot is required');
  if (!Number.isInteger(pageSize) || pageSize < 1) throw new Error('pageSize must be a positive integer');
  const paths = catalogPaths(repoRoot, category);
  const previousState = readJsonIfExists(paths.statePath);
  const entries = discoverCatalogEntries({ repoRoot, category, rawBaseUrl });
  const fingerprint = sourceFingerprint(entries);
  const changed = previousState?.source_fingerprint !== fingerprint || previousState?.page_size !== pageSize;
  const generatedAt = !changed && typeof previousState?.updated_at === 'string'
    ? previousState.updated_at
    : (typeof now === 'string' ? now : now.toISOString());
  const pageCount = Math.max(1, Math.ceil(entries.length / pageSize));
  const pageDocuments = [];
  const stateEntries = {};

  for (let page = 1; page <= pageCount; page += 1) {
    const offset = (page - 1) * pageSize;
    const pageEntries = entries.slice(offset, offset + pageSize).map((entry, index) => {
      const catalogEntry = { ...entry, catalog_page: page };
      stateEntries[entry.id] = {
        page,
        position: index + 1,
        source_fingerprint: sha256(json(entry)),
        checksum: entry.checksum,
      };
      return catalogEntry;
    });
    const cursor = cursorFor(entries, offset);
    const nextOffset = offset + pageSize;
    const nextCursor = nextOffset < entries.length ? cursorFor(entries, nextOffset) : null;
    const nextPagePath = nextCursor ? `${category}/catalog/v1/pages/${page + 1}.json` : null;
    const document = {
      schema_version: CATALOG_SCHEMA_VERSION,
      category,
      page,
      cursor,
      next_cursor: nextCursor,
      next_page_locator: nextPagePath ? rawUrl(rawBaseUrl, nextPagePath) : null,
      entries: pageEntries,
    };
    const bytes = json(document);
    pageDocuments.push({
      page,
      cursor,
      entry_count: pageEntries.length,
      checksum: sha256(bytes),
      locator: rawUrl(rawBaseUrl, `${category}/catalog/v1/pages/${page}.json`),
      filePath: path.join(paths.pagesRoot, `${page}.json`),
      document,
      bytes,
    });
  }

  const indexDocument = {
    schema_version: CATALOG_SCHEMA_VERSION,
    category,
    generated_at: generatedAt,
    page_size: pageSize,
    total_entries: entries.length,
    first_page: {
      page: 1,
      cursor: 'start',
      locator: pageDocuments[0].locator,
    },
    pages: pageDocuments.map(({ page, cursor, entry_count, checksum, locator }) => ({
      page, cursor, entry_count, checksum, locator,
    })),
  };
  const stateDocument = {
    schema_version: CATALOG_STATE_SCHEMA_VERSION,
    category,
    updated_at: generatedAt,
    page_size: pageSize,
    source_fingerprint: fingerprint,
    entries: stateEntries,
  };
  return { paths, indexDocument, pageDocuments, stateDocument };
}

function expectedFiles(catalog) {
  return new Map([
    [catalog.paths.indexPath, json(catalog.indexDocument)],
    ...catalog.pageDocuments.map((page) => [page.filePath, page.bytes]),
    [catalog.paths.statePath, json(catalog.stateDocument)],
  ]);
}

function stalePageFiles(catalog) {
  if (!fs.existsSync(catalog.paths.pagesRoot)) return [];
  const expected = new Set(catalog.pageDocuments.map((page) => path.resolve(page.filePath)));
  return fs.readdirSync(catalog.paths.pagesRoot, { withFileTypes: true })
    .filter((entry) => entry.isFile() && /^\d+\.json$/.test(entry.name))
    .map((entry) => path.join(catalog.paths.pagesRoot, entry.name))
    .filter((filePath) => !expected.has(path.resolve(filePath)));
}

export function updateCategoryCatalog(options = {}) {
  const catalog = buildCategoryCatalog(options);
  const changedFiles = [];
  for (const [filePath, bytes] of expectedFiles(catalog)) {
    if (!fs.existsSync(filePath) || fs.readFileSync(filePath, 'utf8') !== bytes) {
      changedFiles.push(filePath);
      if (!options.check) {
        fs.mkdirSync(path.dirname(filePath), { recursive: true });
        fs.writeFileSync(filePath, bytes);
      }
    }
  }
  for (const filePath of stalePageFiles(catalog)) {
    changedFiles.push(filePath);
    if (!options.check) fs.unlinkSync(filePath);
  }
  if (options.check && changedFiles.length > 0) {
    throw new Error(`${options.category} catalog drift: ${changedFiles.map((file) => relative(options.repoRoot, file)).join(', ')}`);
  }
  return {
    category: options.category,
    changed: changedFiles.length > 0,
    changedFiles,
    totalEntries: catalog.indexDocument.total_entries,
    pageCount: catalog.pageDocuments.length,
  };
}

export function updateExtensionCatalog({
  repoRoot,
  categories = CATALOG_CATEGORIES,
  ...options
} = {}) {
  return categories.map((category) => updateCategoryCatalog({
    ...options,
    repoRoot,
    category,
  }));
}
