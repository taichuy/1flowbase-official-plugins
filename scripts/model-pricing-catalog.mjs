import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const MODEL_PRICING_SCHEMA_VERSION = '1flowbase.model-pricing/v1';
export const MODEL_PRICING_SOURCE_SCHEMA_VERSION = '1flowbase.model-pricing-source/v1';
export const MODEL_PRICING_INDEX_SCHEMA_VERSION = '1flowbase.model-pricing-index/v1';
export const MODEL_PRICING_PAGE_SCHEMA_VERSION = '1flowbase.model-pricing-page/v1';
export const MODEL_PRICING_SEARCH_SCHEMA_VERSION = '1flowbase.model-pricing-search/v1';
export const MODEL_PRICING_STATE_SCHEMA_VERSION = '1flowbase.model-pricing-state/v1';
export const DEFAULT_MODEL_PRICING_PAGE_SIZE = 100;
export const DEFAULT_MODEL_PRICING_RAW_BASE_URL =
  process.env.MODEL_PRICING_CATALOG_RAW_BASE_URL ||
  'https://raw.githubusercontent.com/taichuy/1flowbase-official-plugins/main';

function json(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function sha256(bytes) {
  return `sha256:${crypto.createHash('sha256').update(bytes).digest('hex')}`;
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function readJsonIfExists(filePath) {
  return fs.existsSync(filePath) ? readJson(filePath) : null;
}

function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function rawUrl(rawBaseUrl, relativePath) {
  return `${rawBaseUrl.replace(/\/+$/, '')}/${relativePath}`;
}

function cursorFor(rules, offset) {
  if (offset === 0) return 'start';
  return Buffer.from(`after:${rules[offset - 1].id}`, 'utf8').toString('base64url');
}

function catalogPaths(repoRoot) {
  const root = path.join(repoRoot, 'model-pricing');
  const catalogRoot = path.join(root, 'catalog', 'v1');
  return {
    root,
    metadataPath: path.join(root, 'catalog-source.json'),
    catalogRoot,
    indexPath: path.join(catalogRoot, 'index.json'),
    aggregatePath: path.join(catalogRoot, 'catalog.json'),
    searchIndexPath: path.join(catalogRoot, 'search-index.json'),
    pagesRoot: path.join(catalogRoot, 'pages'),
    statePath: path.join(root, '_maintenance', 'catalog-state.json'),
    distPath: path.join(root, 'dist', 'catalog-seed.json'),
  };
}

function providerDirectories(root) {
  return fs.readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && entry.name.startsWith('@'))
    .sort((left, right) => compareText(left.name, right.name));
}

function ruleOrder(left, right) {
  return compareText(left.provider_code, right.provider_code) ||
    compareText(left.upstream_model_id, right.upstream_model_id) ||
    compareText(left.effective_from, right.effective_from) ||
    left.priority - right.priority ||
    compareText(left.id, right.id);
}

export function discoverModelPricingRules(repoRoot) {
  const paths = catalogPaths(repoRoot);
  const rules = [];
  const ids = new Set();
  for (const providerDirectory of providerDirectories(paths.root)) {
    const providerCode = providerDirectory.name.slice(1);
    const providerRoot = path.join(paths.root, providerDirectory.name);
    for (const modelDirectory of fs.readdirSync(providerRoot, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .sort((left, right) => compareText(left.name, right.name))) {
      const sourcePath = path.join(providerRoot, modelDirectory.name, 'pricing.json');
      if (!fs.existsSync(sourcePath)) continue;
      const source = readJson(sourcePath);
      if (source.schema_version !== MODEL_PRICING_SOURCE_SCHEMA_VERSION ||
          source.currency_code !== 'USD' || !Array.isArray(source.rules)) {
        throw new Error(`${path.relative(repoRoot, sourcePath)} is not a USD model-pricing source`);
      }
      if (source.provider_code !== providerCode) {
        throw new Error(`${path.relative(repoRoot, sourcePath)} provider_code must match @${providerCode}`);
      }
      if (typeof source.upstream_model_id !== 'string' || source.upstream_model_id.length === 0) {
        throw new Error(`${path.relative(repoRoot, sourcePath)} upstream_model_id is required`);
      }
      for (const sourceRule of source.rules) {
        if (!sourceRule.id || ids.has(sourceRule.id)) {
          throw new Error('model pricing rule ids must be unique');
        }
        ids.add(sourceRule.id);
        const sourceChecksum = sha256(json({
          provider_code: providerCode,
          upstream_model_id: source.upstream_model_id,
          rule: sourceRule,
        }));
        rules.push({
          ...sourceRule,
          provider_code: providerCode,
          upstream_model_id: source.upstream_model_id,
          currency_code: 'USD',
          source_kind: 'official',
          source_catalog_id: sourceRule.id,
          source_checksum: sourceChecksum,
        });
      }
    }
  }
  return rules.sort(ruleOrder);
}

export function canonicalRulesBytes(catalog) {
  return Buffer.from(JSON.stringify(catalog.rules));
}

export function verifyModelPricingCatalog(catalog) {
  if (catalog?.schema_version !== MODEL_PRICING_SCHEMA_VERSION) {
    throw new Error('unsupported model pricing catalog schema');
  }
  if (catalog.currency_code !== 'USD' || !Array.isArray(catalog.rules)) {
    throw new Error('model pricing catalog must contain USD rules');
  }
  const actual = sha256(canonicalRulesBytes(catalog));
  if (catalog.rules_checksum !== actual) {
    throw new Error('model pricing rules checksum mismatch');
  }
  const ids = new Set();
  for (const rule of catalog.rules) {
    if (!rule.id || ids.has(rule.id)) throw new Error('model pricing rule ids must be unique');
    ids.add(rule.id);
    if (rule.currency_code !== 'USD' || rule.source_kind !== 'official') {
      throw new Error('official model pricing rules must use USD and source_kind=official');
    }
  }
  return true;
}

export function buildSignedModelPricingCatalog(catalog, privateKey, keyId) {
  verifyModelPricingCatalog(catalog);
  if (privateKey.asymmetricKeyType !== 'ed25519') {
    throw new Error('model pricing signing key must be Ed25519');
  }
  return {
    ...catalog,
    signature: {
      algorithm: 'ed25519',
      key_id: keyId,
      signature: crypto.sign(null, canonicalRulesBytes(catalog), privateKey).toString('base64'),
    },
  };
}

export function buildModelPricingCatalog({
  repoRoot,
  pageSize = DEFAULT_MODEL_PRICING_PAGE_SIZE,
  rawBaseUrl = DEFAULT_MODEL_PRICING_RAW_BASE_URL,
  now = new Date(),
} = {}) {
  if (!repoRoot) throw new Error('repoRoot is required');
  if (!Number.isInteger(pageSize) || pageSize < 1) throw new Error('pageSize must be a positive integer');
  const paths = catalogPaths(repoRoot);
  const metadata = readJson(paths.metadataPath);
  if (metadata.schema_version !== MODEL_PRICING_SOURCE_SCHEMA_VERSION ||
      typeof metadata.catalog_version !== 'string' || metadata.currency_code !== 'USD') {
    throw new Error('invalid model-pricing/catalog-source.json');
  }
  const discovered = discoverModelPricingRules(repoRoot);
  const rules = discovered.map((rule) => ({ ...rule, source_version: metadata.catalog_version }));
  const fingerprint = sha256(json(rules));
  const previousState = readJsonIfExists(paths.statePath);
  const changed = previousState?.source_fingerprint !== fingerprint || previousState?.page_size !== pageSize;
  const generatedAt = !changed && typeof previousState?.updated_at === 'string'
    ? previousState.updated_at
    : (typeof now === 'string' ? now : now.toISOString());
  const aggregate = {
    schema_version: MODEL_PRICING_SCHEMA_VERSION,
    catalog_version: metadata.catalog_version,
    generated_at: generatedAt,
    currency_code: 'USD',
    rules_checksum: sha256(Buffer.from(JSON.stringify(rules))),
    signature: null,
    rules,
  };
  const pageCount = Math.max(1, Math.ceil(rules.length / pageSize));
  const pages = [];
  const stateRules = {};
  for (let page = 1; page <= pageCount; page += 1) {
    const offset = (page - 1) * pageSize;
    const pageRules = rules.slice(offset, offset + pageSize);
    const cursor = cursorFor(rules, offset);
    const nextOffset = offset + pageSize;
    const nextCursor = nextOffset < rules.length ? cursorFor(rules, nextOffset) : null;
    for (const [position, rule] of pageRules.entries()) {
      stateRules[rule.id] = { page, position: position + 1, source_checksum: rule.source_checksum };
    }
    const document = {
      schema_version: MODEL_PRICING_PAGE_SCHEMA_VERSION,
      catalog_version: metadata.catalog_version,
      currency_code: 'USD',
      page,
      cursor,
      next_cursor: nextCursor,
      next_page_locator: nextCursor
        ? rawUrl(rawBaseUrl, `model-pricing/catalog/v1/pages/${page + 1}.json`)
        : null,
      rules: pageRules,
    };
    const bytes = json(document);
    pages.push({
      page,
      cursor,
      rule_count: pageRules.length,
      checksum: sha256(bytes),
      locator: rawUrl(rawBaseUrl, `model-pricing/catalog/v1/pages/${page}.json`),
      filePath: path.join(paths.pagesRoot, `${page}.json`),
      document,
      bytes,
    });
  }
  const pageByNumber = new Map(pages.map((page) => [page.page, page]));
  const search = {
    schema_version: MODEL_PRICING_SEARCH_SCHEMA_VERSION,
    catalog_version: metadata.catalog_version,
    generated_at: generatedAt,
    source_fingerprint: fingerprint,
    entries: rules.map((rule) => {
      const page = pageByNumber.get(stateRules[rule.id].page);
      return {
        id: rule.id,
        provider_code: rule.provider_code.toLocaleLowerCase('en-US'),
        upstream_model_id: rule.upstream_model_id.toLocaleLowerCase('en-US'),
        effective_from: rule.effective_from,
        effective_to: rule.effective_to,
        priority: rule.priority,
        enabled: rule.enabled,
        catalog_page: {
          page: page.page,
          cursor: page.cursor,
          checksum: page.checksum,
          locator: page.locator,
        },
      };
    }),
  };
  const searchBytes = json(search);
  const index = {
    schema_version: MODEL_PRICING_INDEX_SCHEMA_VERSION,
    catalog_version: metadata.catalog_version,
    generated_at: generatedAt,
    currency_code: 'USD',
    page_size: pageSize,
    total_rules: rules.length,
    first_page: { page: 1, cursor: 'start', locator: pages[0].locator },
    search_index: {
      schema_version: MODEL_PRICING_SEARCH_SCHEMA_VERSION,
      entry_count: rules.length,
      checksum: sha256(searchBytes),
      locator: rawUrl(rawBaseUrl, 'model-pricing/catalog/v1/search-index.json'),
    },
    pages: pages.map(({ page, cursor, rule_count, checksum, locator }) => ({
      page, cursor, rule_count, checksum, locator,
    })),
  };
  const state = {
    schema_version: MODEL_PRICING_STATE_SCHEMA_VERSION,
    updated_at: generatedAt,
    page_size: pageSize,
    source_fingerprint: fingerprint,
    rules: stateRules,
  };
  return { paths, aggregate, index, pages, search, searchBytes, state };
}

function expectedFiles(catalog) {
  const aggregateBytes = json(catalog.aggregate);
  return new Map([
    [catalog.paths.aggregatePath, aggregateBytes],
    [catalog.paths.distPath, aggregateBytes],
    [catalog.paths.indexPath, json(catalog.index)],
    [catalog.paths.searchIndexPath, catalog.searchBytes],
    [catalog.paths.statePath, json(catalog.state)],
    ...catalog.pages.map((page) => [page.filePath, page.bytes]),
  ]);
}

export function updateModelPricingCatalog(options = {}) {
  const catalog = buildModelPricingCatalog(options);
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
  if (fs.existsSync(catalog.paths.pagesRoot)) {
    const expectedPages = new Set(catalog.pages.map((page) => path.resolve(page.filePath)));
    for (const entry of fs.readdirSync(catalog.paths.pagesRoot, { withFileTypes: true })) {
      const filePath = path.join(catalog.paths.pagesRoot, entry.name);
      if (entry.isFile() && /^\d+\.json$/.test(entry.name) && !expectedPages.has(path.resolve(filePath))) {
        changedFiles.push(filePath);
        if (!options.check) fs.unlinkSync(filePath);
      }
    }
  }
  if (options.check && changedFiles.length > 0) {
    throw new Error(`model pricing catalog drift: ${changedFiles.map((file) => path.relative(options.repoRoot, file)).join(', ')}`);
  }
  return { changed: changedFiles.length > 0, changedFiles, totalRules: catalog.index.total_rules, pageCount: catalog.pages.length };
}

function parseCli(argv) {
  const options = { check: false };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--check') options.check = true;
    else if (argument === '--private-key-pem-file') options.privateKeyPath = path.resolve(argv[++index]);
    else if (argument === '--key-id') options.keyId = argv[++index];
    else if (argument === '--output') options.output = path.resolve(argv[++index]);
    else throw new Error('usage: model-pricing-catalog.mjs [--check] | --private-key-pem-file PATH --key-id ID --output PATH');
  }
  return options;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const options = parseCli(process.argv.slice(2));
  const repositoryRoot = path.resolve(import.meta.dirname, '..');
  if (options.privateKeyPath || options.keyId || options.output) {
    if (!options.privateKeyPath || !options.keyId || !options.output) throw new Error('signing requires private key, key id, and output');
    const sourcePath = path.join(repositoryRoot, 'model-pricing', 'catalog', 'v1', 'catalog.json');
    const catalog = readJson(sourcePath);
    const privateKey = crypto.createPrivateKey(fs.readFileSync(options.privateKeyPath, 'utf8'));
    fs.writeFileSync(options.output, json(buildSignedModelPricingCatalog(catalog, privateKey, options.keyId)));
  } else {
    const result = updateModelPricingCatalog({ repoRoot: repositoryRoot, check: options.check });
    console.log(`model-pricing: ${result.totalRules} rules, ${result.pageCount} pages${result.changed ? ' (updated)' : ''}`);
  }
}
