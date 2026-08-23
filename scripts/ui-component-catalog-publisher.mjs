import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const UI_COMPONENT_SOURCE_SCHEMA_VERSION = '1flowbase.ui-component-source/v1';
export const UI_COMPONENT_CATALOG_SOURCE_SCHEMA_VERSION = '1flowbase.ui-component-catalog-source/v1';
export const UI_COMPONENT_SEED_SCHEMA_VERSION = '1flowbase.ui-component-catalog-seed/v1';
export const UI_COMPONENT_INDEX_SCHEMA_VERSION = '1flowbase.ui-component-catalog-index/v1';
export const UI_COMPONENT_PAGE_SCHEMA_VERSION = '1flowbase.ui-component-catalog-page/v1';
export const UI_COMPONENT_SEARCH_SCHEMA_VERSION = '1flowbase.ui-component-catalog-search/v1';
export const UI_COMPONENT_STATE_SCHEMA_VERSION = '1flowbase.ui-component-catalog-state/v1';

export const DEFAULT_UI_COMPONENT_RAW_BASE_URL =
  process.env.UI_COMPONENT_CATALOG_RAW_BASE_URL ||
  'https://raw.githubusercontent.com/taichuy/1flowbase-official-plugins/main';

const RELEASE_BASE_URL =
  'https://github.com/taichuy/1flowbase-official-plugins/releases/download';
const SHA256_PATTERN = /^sha256:[a-f0-9]{64}$/;
const SEMVER_PATTERN = /^\d+\.\d+\.\d+$/;
const CODE_PATTERN = /^[a-z0-9](?:[a-z0-9._-]*[a-z0-9])?$/;
const SOURCE_FIELDS = Object.freeze([
  'schema_version',
  'component_code',
  'name',
  'description',
  'import_code',
  'source_code',
  'origin',
  'source',
  'group',
  'upstream',
  'version',
  'keywords',
  'updated_at',
]);
const CATALOG_SOURCE_FIELDS = Object.freeze([
  'schema_version',
  'catalog_version',
  'generated_at',
  'page_size',
]);

function fail(message) {
  throw new Error(`Invalid official UI component catalog: ${message}`);
}

function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function compareText(left, right) {
  if (left < right) return -1;
  if (left > right) return 1;
  return 0;
}

function sortedRecord(record) {
  return Object.fromEntries(
    Object.entries(record).sort(([left], [right]) => compareText(left, right)),
  );
}

export function stableJson(value) {
  return `${JSON.stringify(value, (_key, current) => {
    if (!isRecord(current)) return current;
    return sortedRecord(current);
  }, 2)}\n`;
}

function sha256(bytes) {
  return `sha256:${crypto.createHash('sha256').update(bytes).digest('hex')}`;
}

function readJson(filePath) {
  try {
    return JSON.parse(fs.readFileSync(filePath, 'utf8'));
  } catch (error) {
    fail(`${filePath} is not valid JSON (${error.message})`);
  }
}

function validateExactKeys(record, expected, context) {
  if (!isRecord(record)) fail(`${context} must be an object`);
  const actualKeys = Object.keys(record).sort(compareText);
  const expectedKeys = [...expected].sort(compareText);
  if (actualKeys.length !== expectedKeys.length ||
      actualKeys.some((key, index) => key !== expectedKeys[index])) {
    fail(`${context} fields must be exactly: ${expectedKeys.join(', ')}`);
  }
}

function validateNonEmptyString(value, context) {
  if (typeof value !== 'string' || value.length === 0) {
    fail(`${context} must be a non-empty string`);
  }
}

function posix(relativePath) {
  return relativePath.split(path.sep).join('/');
}

function rawUrl(rawBaseUrl, relativePath) {
  return `${rawBaseUrl.replace(/\/+$/, '')}/${relativePath}`;
}

function releaseTag(version) {
  return `ui-component-catalog-v${version}`;
}

function releaseAssetName(version) {
  return `ui-component-catalog-v${version}.json`;
}

function catalogPaths(repoRoot) {
  const root = path.join(repoRoot, 'ui_components');
  const catalogRoot = path.join(root, 'catalog', 'v1');
  return {
    root,
    metadataPath: path.join(root, 'catalog-source.json'),
    catalogRoot,
    indexPath: path.join(catalogRoot, 'index.json'),
    searchIndexPath: path.join(catalogRoot, 'search-index.json'),
    pagesRoot: path.join(catalogRoot, 'pages'),
    statePath: path.join(root, '_maintenance', 'catalog-state.json'),
    seedPath: path.join(root, 'dist', 'catalog-seed.json'),
    releaseCatalogPath: path.join(root, 'releases', 'v1', 'catalog.json'),
  };
}

function validateCatalogSource(metadata) {
  validateExactKeys(metadata, CATALOG_SOURCE_FIELDS, 'ui_components/catalog-source.json');
  if (metadata.schema_version !== UI_COMPONENT_CATALOG_SOURCE_SCHEMA_VERSION) {
    fail(`unsupported catalog source schema_version ${metadata.schema_version}`);
  }
  if (!SEMVER_PATTERN.test(metadata.catalog_version)) {
    fail('catalog_version must be semantic version x.y.z');
  }
  if (typeof metadata.generated_at !== 'string' || Number.isNaN(Date.parse(metadata.generated_at))) {
    fail('generated_at must be an ISO timestamp');
  }
  if (!Number.isInteger(metadata.page_size) || metadata.page_size < 1) {
    fail('page_size must be a positive integer');
  }
}

function normalizeKeywords(value, context) {
  if (!Array.isArray(value) ||
      value.some((keyword) => typeof keyword !== 'string' || keyword.trim().length === 0)) {
    fail(`${context}.keywords must be an array of non-empty strings`);
  }
  const normalized = value.map((keyword) => keyword.trim());
  if (new Set(normalized).size !== normalized.length) {
    fail(`${context}.keywords must be unique`);
  }
  return normalized.sort(compareText);
}

function canonicalSourceRecord(record) {
  return Object.fromEntries(SOURCE_FIELDS.map((field) => [field, record[field]]));
}

function validateAndNormalizeSourceRecord(record, context, expectedSource, expectedGroup) {
  validateExactKeys(record, SOURCE_FIELDS, context);
  if (record.schema_version !== UI_COMPONENT_SOURCE_SCHEMA_VERSION) {
    fail(`${context}.schema_version must be ${UI_COMPONENT_SOURCE_SCHEMA_VERSION}`);
  }
  for (const field of ['component_code', 'name', 'description', 'import_code', 'source_code', 'source', 'group', 'version']) {
    validateNonEmptyString(record[field], `${context}.${field}`);
  }
  if (!CODE_PATTERN.test(record.component_code)) {
    fail(`${context}.component_code must be a stable lowercase code`);
  }
  if (!CODE_PATTERN.test(record.source) || !CODE_PATTERN.test(record.group)) {
    fail(`${context}.source and group must be lowercase codes`);
  }
  if (record.origin !== 'official') fail(`${context}.origin must be official`);
  if (record.source !== expectedSource) fail(`${context}.source must match canonical path @${expectedSource}`);
  if (record.group !== expectedGroup) fail(`${context}.group must match canonical path ${expectedGroup}`);
  if (!SEMVER_PATTERN.test(record.version)) fail(`${context}.version must be semantic version x.y.z`);
  if (typeof record.updated_at !== 'string' || Number.isNaN(Date.parse(record.updated_at))) {
    fail(`${context}.updated_at must be an ISO timestamp`);
  }
  validateExactKeys(record.upstream, ['identity', 'version'], `${context}.upstream`);
  validateNonEmptyString(record.upstream.identity, `${context}.upstream.identity`);
  validateNonEmptyString(record.upstream.version, `${context}.upstream.version`);

  const normalized = {
    ...record,
    keywords: normalizeKeywords(record.keywords, context),
    upstream: { identity: record.upstream.identity, version: record.upstream.version },
  };
  return canonicalSourceRecord(normalized);
}

function sourceDirectories(root) {
  if (!fs.existsSync(root)) return [];
  return fs.readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && entry.name.startsWith('@') && entry.name.length > 1)
    .sort((left, right) => compareText(left.name, right.name));
}

export function discoverUiComponentRecords(repoRoot) {
  if (!repoRoot) fail('repoRoot is required');
  const paths = catalogPaths(repoRoot);
  const records = [];
  const componentCodes = new Set();

  for (const sourceDirectory of sourceDirectories(paths.root)) {
    const source = sourceDirectory.name.slice(1);
    const sourceRoot = path.join(paths.root, sourceDirectory.name);
    const groupDirectories = fs.readdirSync(sourceRoot, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .sort((left, right) => compareText(left.name, right.name));
    for (const groupDirectory of groupDirectories) {
      const group = groupDirectory.name;
      const groupRoot = path.join(sourceRoot, group);
      const files = fs.readdirSync(groupRoot, { withFileTypes: true })
        .filter((entry) => entry.isFile() && entry.name.endsWith('.json'))
        .sort((left, right) => compareText(left.name, right.name));
      for (const file of files) {
        const filePath = path.join(groupRoot, file.name);
        const locator = posix(path.relative(repoRoot, filePath));
        const sourceRecord = validateAndNormalizeSourceRecord(
          readJson(filePath),
          locator,
          source,
          group,
        );
        if (componentCodes.has(sourceRecord.component_code)) {
          fail(`duplicate component_code ${sourceRecord.component_code}`);
        }
        componentCodes.add(sourceRecord.component_code);
        records.push({
          ...sourceRecord,
          source_locator: locator,
          source_checksum: sha256(stableJson(sourceRecord)),
        });
      }
    }
  }
  return records.sort((left, right) => compareText(left.component_code, right.component_code));
}

function cursorFor(components, offset) {
  if (offset === 0) return 'start';
  return Buffer.from(`after:${components[offset - 1].component_code}`, 'utf8').toString('base64url');
}

function normalizeSearchText(value) {
  return value.normalize('NFKC').trim().replace(/\s+/g, ' ').toLocaleLowerCase('en-US');
}

function seedSemanticInput(seed) {
  return {
    catalog_version: seed.manifest.catalog_version,
    generated_at: seed.manifest.generated_at,
    page_size: seed.manifest.page_size,
    total_components: seed.manifest.total_components,
    components_sha256: seed.manifest.components_sha256,
    components: seed.components,
  };
}

function sourceRecordFromPublished(component) {
  return Object.fromEntries(SOURCE_FIELDS.map((field) => [field, component[field]]));
}

export function verifyUiComponentSeed(seed) {
  if (!isRecord(seed) || !isRecord(seed.manifest) || !Array.isArray(seed.components)) {
    fail('Seed shape is invalid');
  }
  validateExactKeys(seed.manifest, [
    'schema_version',
    'catalog_version',
    'generated_at',
    'page_size',
    'total_components',
    'components_sha256',
    'semantic_sha256',
  ], 'Seed manifest');
  if (seed.manifest.schema_version !== UI_COMPONENT_SEED_SCHEMA_VERSION ||
      !SEMVER_PATTERN.test(seed.manifest.catalog_version) ||
      typeof seed.manifest.generated_at !== 'string' ||
      Number.isNaN(Date.parse(seed.manifest.generated_at)) ||
      !Number.isInteger(seed.manifest.page_size) || seed.manifest.page_size < 1 ||
      seed.manifest.total_components !== seed.components.length ||
      !SHA256_PATTERN.test(seed.manifest.components_sha256) ||
      !SHA256_PATTERN.test(seed.manifest.semantic_sha256)) {
    fail('Seed manifest is invalid');
  }

  let previousCode = null;
  const componentCodes = new Set();
  for (const component of seed.components) {
    validateExactKeys(component, [...SOURCE_FIELDS, 'source_locator', 'source_checksum'], 'Seed component');
    const normalizedSource = validateAndNormalizeSourceRecord(
      sourceRecordFromPublished(component),
      `Seed component ${component.component_code}`,
      component.source,
      component.group,
    );
    if (stableJson(normalizedSource) !== stableJson(sourceRecordFromPublished(component))) {
      fail(`tampered Seed normalization for ${component.component_code}`);
    }
    if (componentCodes.has(component.component_code)) fail('tampered Seed contains duplicate component_code');
    if (previousCode !== null && compareText(previousCode, component.component_code) >= 0) {
      fail('tampered Seed component order is invalid');
    }
    componentCodes.add(component.component_code);
    previousCode = component.component_code;
    const expectedLocatorPrefix = `ui_components/@${component.source}/${component.group}/`;
    if (typeof component.source_locator !== 'string' ||
        !component.source_locator.startsWith(expectedLocatorPrefix) ||
        !component.source_locator.endsWith('.json') ||
        !SHA256_PATTERN.test(component.source_checksum)) {
      fail(`tampered Seed metadata for ${component.component_code}`);
    }
    const sourceChecksum = sha256(stableJson(sourceRecordFromPublished(component)));
    if (sourceChecksum !== component.source_checksum) {
      fail(`tampered Seed source checksum for ${component.component_code}`);
    }
  }

  const componentsChecksum = sha256(stableJson(seed.components));
  if (componentsChecksum !== seed.manifest.components_sha256) {
    fail('tampered Seed components checksum mismatch');
  }
  const semanticChecksum = sha256(stableJson(seedSemanticInput(seed)));
  if (semanticChecksum !== seed.manifest.semantic_sha256) {
    fail('tampered Seed semantic checksum mismatch');
  }
  return true;
}

export function buildUiComponentCatalog({
  repoRoot,
  rawBaseUrl = DEFAULT_UI_COMPONENT_RAW_BASE_URL,
} = {}) {
  if (!repoRoot) fail('repoRoot is required');
  const paths = catalogPaths(repoRoot);
  const metadata = readJson(paths.metadataPath);
  validateCatalogSource(metadata);
  const components = discoverUiComponentRecords(repoRoot);
  const componentsChecksum = sha256(stableJson(components));
  const seed = {
    manifest: {
      schema_version: UI_COMPONENT_SEED_SCHEMA_VERSION,
      catalog_version: metadata.catalog_version,
      generated_at: metadata.generated_at,
      page_size: metadata.page_size,
      total_components: components.length,
      components_sha256: componentsChecksum,
      semantic_sha256: '',
    },
    components,
  };
  seed.manifest.semantic_sha256 = sha256(stableJson(seedSemanticInput(seed)));
  verifyUiComponentSeed(seed);
  const seedBytes = stableJson(seed);
  const seedChecksum = sha256(seedBytes);

  const pageCount = Math.max(1, Math.ceil(components.length / metadata.page_size));
  const pages = [];
  const componentPages = new Map();
  for (let page = 1; page <= pageCount; page += 1) {
    const offset = (page - 1) * metadata.page_size;
    const pageComponents = components.slice(offset, offset + metadata.page_size);
    const cursor = cursorFor(components, offset);
    const nextOffset = offset + metadata.page_size;
    const nextCursor = nextOffset < components.length ? cursorFor(components, nextOffset) : null;
    for (const component of pageComponents) componentPages.set(component.component_code, page);
    const document = {
      schema_version: UI_COMPONENT_PAGE_SCHEMA_VERSION,
      catalog_version: metadata.catalog_version,
      page,
      cursor,
      next_cursor: nextCursor,
      next_page_locator: nextCursor
        ? rawUrl(rawBaseUrl, `ui_components/catalog/v1/pages/${page + 1}.json`)
        : null,
      components: pageComponents,
    };
    const bytes = stableJson(document);
    pages.push({
      page,
      cursor,
      component_count: pageComponents.length,
      checksum: sha256(bytes),
      locator: rawUrl(rawBaseUrl, `ui_components/catalog/v1/pages/${page}.json`),
      filePath: path.join(paths.pagesRoot, `${page}.json`),
      document,
      bytes,
    });
  }

  const pageByNumber = new Map(pages.map((page) => [page.page, page]));
  const search = {
    schema_version: UI_COMPONENT_SEARCH_SCHEMA_VERSION,
    catalog_version: metadata.catalog_version,
    generated_at: metadata.generated_at,
    source_fingerprint: componentsChecksum,
    entries: components.map((component) => {
      const page = pageByNumber.get(componentPages.get(component.component_code));
      return {
        component_code: component.component_code,
        name: normalizeSearchText(component.name),
        description: normalizeSearchText(component.description),
        origin: component.origin,
        source: component.source,
        group: component.group,
        upstream: component.upstream,
        version: component.version,
        keywords: component.keywords.map(normalizeSearchText),
        catalog_page: {
          page: page.page,
          cursor: page.cursor,
          checksum: page.checksum,
          locator: page.locator,
        },
      };
    }),
  };
  const searchBytes = stableJson(search);
  const searchChecksum = sha256(searchBytes);
  const tag = releaseTag(metadata.catalog_version);
  const asset = releaseAssetName(metadata.catalog_version);
  const index = {
    schema_version: UI_COMPONENT_INDEX_SCHEMA_VERSION,
    catalog_version: metadata.catalog_version,
    generated_at: metadata.generated_at,
    page_size: metadata.page_size,
    total_components: components.length,
    source_fingerprint: componentsChecksum,
    first_page: {
      page: 1,
      cursor: 'start',
      locator: pages[0].locator,
    },
    search_index: {
      schema_version: UI_COMPONENT_SEARCH_SCHEMA_VERSION,
      entry_count: components.length,
      checksum: searchChecksum,
      locator: rawUrl(rawBaseUrl, 'ui_components/catalog/v1/search-index.json'),
    },
    download: {
      schema_version: UI_COMPONENT_SEED_SCHEMA_VERSION,
      checksum: seedChecksum,
      locator: rawUrl(rawBaseUrl, 'ui_components/dist/catalog-seed.json'),
      release_tag: tag,
      release_locator: `${RELEASE_BASE_URL}/${tag}/${asset}`,
      release_catalog_locator: rawUrl(rawBaseUrl, 'ui_components/releases/v1/catalog.json'),
    },
    update: {
      strategy: 'authoritative_source_group_replace',
      identity_field: 'component_code',
      source_field: 'source',
      group_field: 'group',
      version_field: 'version',
    },
    pages: pages.map(({ page, cursor, component_count, checksum, locator }) => ({
      page,
      cursor,
      component_count,
      checksum,
      locator,
    })),
  };
  const state = {
    schema_version: UI_COMPONENT_STATE_SCHEMA_VERSION,
    catalog_version: metadata.catalog_version,
    updated_at: metadata.generated_at,
    page_size: metadata.page_size,
    source_fingerprint: componentsChecksum,
    components: Object.fromEntries(components.map((component) => [component.component_code, {
      source: component.source,
      group: component.group,
      version: component.version,
      source_locator: component.source_locator,
      source_checksum: component.source_checksum,
      page: componentPages.get(component.component_code),
    }])),
  };
  return {
    paths,
    seed,
    seedBytes,
    seedChecksum,
    index,
    pages,
    search,
    searchBytes,
    searchChecksum,
    state,
  };
}

function expectedFiles(catalog) {
  return new Map([
    [catalog.paths.seedPath, catalog.seedBytes],
    [catalog.paths.indexPath, stableJson(catalog.index)],
    [catalog.paths.searchIndexPath, catalog.searchBytes],
    [catalog.paths.statePath, stableJson(catalog.state)],
    ...catalog.pages.map((page) => [page.filePath, page.bytes]),
  ]);
}

function stalePageFiles(catalog) {
  if (!fs.existsSync(catalog.paths.pagesRoot)) return [];
  const expected = new Set(catalog.pages.map((page) => path.resolve(page.filePath)));
  return fs.readdirSync(catalog.paths.pagesRoot, { withFileTypes: true })
    .filter((entry) => entry.isFile() && /^\d+\.json$/.test(entry.name))
    .map((entry) => path.join(catalog.paths.pagesRoot, entry.name))
    .filter((filePath) => !expected.has(path.resolve(filePath)));
}

export function updateUiComponentCatalog(options = {}) {
  const catalog = buildUiComponentCatalog(options);
  const changedFiles = [];
  for (const [filePath, expectedBytes] of expectedFiles(catalog)) {
    const actualBytes = fs.existsSync(filePath) ? fs.readFileSync(filePath, 'utf8') : null;
    if (actualBytes !== expectedBytes) {
      changedFiles.push(filePath);
      if (!options.check) {
        fs.mkdirSync(path.dirname(filePath), { recursive: true });
        fs.writeFileSync(filePath, expectedBytes);
      }
    }
  }
  for (const filePath of stalePageFiles(catalog)) {
    changedFiles.push(filePath);
    if (!options.check) fs.unlinkSync(filePath);
  }
  if (options.check && changedFiles.length > 0) {
    throw new Error(`UI component catalog drift: ${changedFiles
      .map((filePath) => posix(path.relative(options.repoRoot, filePath)))
      .join(', ')}`);
  }
  return {
    changed: changedFiles.length > 0,
    changedFiles,
    totalComponents: catalog.index.total_components,
    pageCount: catalog.pages.length,
  };
}

function parseCli(argv) {
  const options = { check: false };
  for (const argument of argv) {
    if (argument === '--check') options.check = true;
    else throw new Error('usage: node scripts/ui-component-catalog-publisher.mjs [--check]');
  }
  return options;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  const result = updateUiComponentCatalog({ repoRoot, ...parseCli(process.argv.slice(2)) });
  process.stdout.write(
    `ui-components: ${result.totalComponents} components, ${result.pageCount} pages${result.changed ? ' (updated)' : ''}\n`,
  );
}
