import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const SOURCE_SCHEMA_VERSION = '1flowbase.i18n-catalog-source/v1';
export const SEED_SCHEMA_VERSION = '1flowbase.i18n-catalog-seed/v1';

const MODULE_PATTERN = /^@[a-z0-9][a-z0-9._-]*\/[a-z0-9][a-z0-9._-]*(?:\/[a-z0-9][a-z0-9._-]*)+$/;
const LOCALE_PATTERN = /^[a-z]{2,3}(?:_[A-Z][A-Za-z]{1,7})?$/;
const CHECKSUM_PATTERN = /^sha256:[a-f0-9]{64}$/;
const NAMED_PLACEHOLDER = /\{([A-Za-z_][A-Za-z0-9_.-]*)\}/g;
const FORBIDDEN_CONTENT = /<\/?[A-Za-z][^>]*>|javascript\s*:|\$\{|=>|\bfunction\s*\(|\bon[A-Za-z]+\s*=|!?\[[^\]]*\]\([^)]+\)/i;

export function isCanonicalModuleId(moduleId) {
  return typeof moduleId === 'string' && MODULE_PATTERN.test(moduleId);
}

function fail(message) {
  throw new Error(`Invalid official i18n catalog: ${message}`);
}

function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function sortedRecord(record) {
  return Object.fromEntries(
    Object.entries(record).sort(([left], [right]) => left.localeCompare(right))
  );
}

export function stableJson(value) {
  return `${JSON.stringify(value, (_key, current) => {
    if (!isRecord(current)) return current;
    return sortedRecord(current);
  }, 2)}\n`;
}

function sha256(value) {
  return `sha256:${crypto.createHash('sha256').update(value).digest('hex')}`;
}

function readJson(filePath) {
  try {
    return JSON.parse(fs.readFileSync(filePath, 'utf8'));
  } catch (error) {
    fail(`${filePath} is not valid JSON (${error.message})`);
  }
}

function validateExactKeys(record, expectedKeys, label) {
  if (!isRecord(record)) fail(`${label} must be an object`);
  const actual = Object.keys(record).sort();
  const expected = [...expectedKeys].sort();
  if (actual.length !== expected.length || actual.some((key, index) => key !== expected[index])) {
    fail(`${label} fields must be exactly: ${expected.join(', ')}`);
  }
}

function validateText(value, label) {
  if (typeof value !== 'string' || value.length === 0) fail(`${label} must be a non-empty string`);
  if (FORBIDDEN_CONTENT.test(value)) fail(`${label} must be plain text without HTML, JavaScript, or rich text`);
  const withoutPlaceholders = value.replace(NAMED_PLACEHOLDER, '');
  if (/[{}]/.test(withoutPlaceholders)) fail(`${label} contains a non-named or malformed placeholder`);
}

function placeholders(value) {
  return [...new Set([...value.matchAll(NAMED_PLACEHOLDER)].map((match) => match[1]))].sort();
}

function sameStrings(left, right) {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function validateManifest(source) {
  validateExactKeys(
    source,
    ['schema_version', 'catalog_version', 'source_locale', 'locales', 'modules', 'files', 'generated_at'],
    'i18n/catalog.json'
  );
  if (source.schema_version !== SOURCE_SCHEMA_VERSION) fail(`unsupported schema_version ${source.schema_version}`);
  if (typeof source.catalog_version !== 'string' || !/^\d+\.\d+\.\d+$/.test(source.catalog_version)) {
    fail('catalog_version must be semantic version x.y.z');
  }
  if (source.source_locale !== 'en_US') fail('source_locale must be en_US for the first catalog version');
  if (!Array.isArray(source.locales) || !source.locales.includes(source.source_locale) || !source.locales.includes('zh_Hans')) {
    fail('locales must contain en_US and zh_Hans');
  }
  if (new Set(source.locales).size !== source.locales.length || source.locales.some((locale) => !LOCALE_PATTERN.test(locale))) {
    fail('locales must be unique valid locale identifiers');
  }
  if (!Array.isArray(source.modules) || source.modules.length === 0 || new Set(source.modules).size !== source.modules.length) {
    fail('modules must be a non-empty unique array');
  }
  if (source.modules.some((moduleId) => !isCanonicalModuleId(moduleId))) {
    fail('module identity must be normalized as @org/multi-level/module');
  }
  if (!Array.isArray(source.files)) fail('files must be an array');
  if (typeof source.generated_at !== 'string' || Number.isNaN(Date.parse(source.generated_at))) {
    fail('generated_at must be an ISO timestamp');
  }
}

function normalizeSourceFile(document, moduleId) {
  if (!Array.isArray(document)) fail(`${moduleId}/en_US.json must be an array of English msgids`);
  if (document.some((msgid) => typeof msgid !== 'string')) fail(`${moduleId}/en_US.json contains a non-string msgid`);
  if (new Set(document).size !== document.length) fail(`${moduleId} contains a duplicate English msgid`);
  for (const msgid of document) validateText(msgid, `${moduleId} msgid ${JSON.stringify(msgid)}`);
  return [...document].sort((left, right) => left.localeCompare(right));
}

function normalizeTargetFile(document, moduleId, locale, msgids) {
  if (!isRecord(document)) fail(`${moduleId}/${locale}.json must be an object keyed by English msgid`);
  const keys = Object.keys(document).sort((left, right) => left.localeCompare(right));
  if (!sameStrings(keys, msgids)) fail(`${moduleId}/${locale}.json keys must exactly match the English msgids`);
  const normalized = {};
  for (const msgid of msgids) {
    const translation = document[msgid];
    validateText(translation, `${moduleId}/${locale} translation for ${JSON.stringify(msgid)}`);
    if (!sameStrings(placeholders(msgid), placeholders(translation))) {
      fail(`${moduleId}/${locale} placeholder set mismatch for ${JSON.stringify(msgid)}`);
    }
    normalized[msgid] = translation;
  }
  return normalized;
}

function expectedFilePath(moduleId, locale) {
  return `i18n/${moduleId}/${locale}.json`;
}

function canonicalFileDocument(locale, sourceLocale, msgids, translations) {
  return locale === sourceLocale ? msgids : translations;
}

function discoverLocaleFiles(repoRoot) {
  const root = path.join(repoRoot, 'i18n');
  const discovered = [];
  function visit(directory) {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const filePath = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        if (filePath !== path.join(root, 'dist')) visit(filePath);
      } else if (entry.isFile() && entry.name.endsWith('.json') && filePath !== path.join(root, 'catalog.json')) {
        discovered.push(path.relative(repoRoot, filePath).split(path.sep).join('/'));
      }
    }
  }
  visit(root);
  return discovered.sort((left, right) => left.localeCompare(right));
}

export function buildCatalogSeed({ repoRoot, previousSeed = null } = {}) {
  if (!repoRoot) fail('repoRoot is required');
  const source = readJson(path.join(repoRoot, 'i18n', 'catalog.json'));
  validateManifest(source);

  const locales = [...source.locales].sort((left, right) => left.localeCompare(right));
  const moduleIds = [...source.modules].sort((left, right) => left.localeCompare(right));
  const declaredFiles = new Map();
  for (const file of source.files) {
    validateExactKeys(file, ['module', 'locale', 'path', 'sha256'], 'catalog file entry');
    if (!moduleIds.includes(file.module) || !locales.includes(file.locale)) fail(`unregistered file ${file.path}`);
    const expectedPath = expectedFilePath(file.module, file.locale);
    if (file.path !== expectedPath) fail(`file path must be ${expectedPath}`);
    if (!CHECKSUM_PATTERN.test(file.sha256)) fail(`invalid SHA-256 for ${file.path}`);
    if (declaredFiles.has(file.path)) fail(`duplicate file entry ${file.path}`);
    declaredFiles.set(file.path, file);
  }

  const normalizedModules = [];
  const normalizedFiles = [];
  for (const moduleId of moduleIds) {
    const sourcePath = expectedFilePath(moduleId, source.source_locale);
    const sourceEntry = declaredFiles.get(sourcePath);
    if (!sourceEntry) fail(`missing file entry ${sourcePath}`);
    const msgids = normalizeSourceFile(readJson(path.join(repoRoot, sourcePath)), moduleId);
    const translationsByLocale = {};

    for (const locale of locales) {
      const relativePath = expectedFilePath(moduleId, locale);
      const entry = declaredFiles.get(relativePath);
      if (!entry) fail(`missing file entry ${relativePath}`);
      const translations = locale === source.source_locale
        ? null
        : normalizeTargetFile(readJson(path.join(repoRoot, relativePath)), moduleId, locale, msgids);
      const checksum = sha256(stableJson(canonicalFileDocument(locale, source.source_locale, msgids, translations)));
      if (entry.sha256 !== checksum) fail(`checksum mismatch for ${relativePath}: expected ${entry.sha256}, got ${checksum}`);
      normalizedFiles.push({ module: moduleId, locale, path: relativePath, sha256: checksum });
      if (translations) translationsByLocale[locale] = translations;
    }

    normalizedModules.push({
      id: moduleId,
      messages: msgids.map((msgid) => ({
        msgid,
        translations: Object.fromEntries(locales
          .filter((locale) => locale !== source.source_locale)
          .map((locale) => [locale, translationsByLocale[locale][msgid]])),
      })),
    });
  }
  if (declaredFiles.size !== normalizedFiles.length) fail('files contains an entry outside the module/locale matrix');
  const discoveredFiles = discoverLocaleFiles(repoRoot);
  const registeredFiles = [...declaredFiles.keys()].sort((left, right) => left.localeCompare(right));
  if (!sameStrings(discoveredFiles, registeredFiles)) {
    fail('source tree locale files must exactly match the catalog file registry');
  }

  const semantic = {
    catalog_version: source.catalog_version,
    source_locale: source.source_locale,
    locales,
    modules: moduleIds,
    files: normalizedFiles,
    normalized_modules: normalizedModules,
  };
  const semanticDigest = sha256(stableJson(semantic));
  const previousDigest = previousSeed?.manifest?.semantic_sha256;
  const generatedAt = previousDigest === semanticDigest && typeof previousSeed?.manifest?.generated_at === 'string'
    ? previousSeed.manifest.generated_at
    : source.generated_at;

  return {
    manifest: {
      schema_version: SEED_SCHEMA_VERSION,
      catalog_version: source.catalog_version,
      source_locale: source.source_locale,
      locales,
      modules: moduleIds,
      files: normalizedFiles,
      generated_at: generatedAt,
      semantic_sha256: semanticDigest,
    },
    modules: normalizedModules,
  };
}

function seedFileDocument(seed, file) {
  const module = seed.modules.find((candidate) => candidate.id === file.module);
  if (!module) fail(`seed is missing module ${file.module}`);
  const msgids = module.messages.map((message) => message.msgid).sort((left, right) => left.localeCompare(right));
  if (file.locale === seed.manifest.source_locale) return msgids;
  return Object.fromEntries(module.messages
    .map((message) => [message.msgid, message.translations?.[file.locale]])
    .sort(([left], [right]) => left.localeCompare(right)));
}

export function verifyCatalogSeed(seed) {
  if (!isRecord(seed) || !isRecord(seed.manifest) || !Array.isArray(seed.modules)) fail('seed shape is invalid');
  if (seed.manifest.schema_version !== SEED_SCHEMA_VERSION) fail('seed schema_version is invalid');
  if (!Array.isArray(seed.manifest.files)) fail('seed manifest files must be an array');
  for (const file of seed.manifest.files) {
    const actual = sha256(stableJson(seedFileDocument(seed, file)));
    if (actual !== file.sha256) fail(`tampered seed or checksum mismatch for ${file.path}`);
  }
  const semantic = {
    catalog_version: seed.manifest.catalog_version,
    source_locale: seed.manifest.source_locale,
    locales: seed.manifest.locales,
    modules: seed.manifest.modules,
    files: seed.manifest.files,
    normalized_modules: seed.modules,
  };
  const digest = sha256(stableJson(semantic));
  if (digest !== seed.manifest.semantic_sha256) fail('tampered seed or semantic checksum mismatch');
  return true;
}

export function publishCatalogSeed({ repoRoot, outputPath, check = false } = {}) {
  const root = repoRoot || path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  const target = outputPath || path.join(root, 'i18n', 'dist', 'catalog-seed.json');
  const previousSeed = fs.existsSync(target) ? readJson(target) : null;
  const seed = buildCatalogSeed({ repoRoot: root, previousSeed });
  verifyCatalogSeed(seed);
  const bytes = stableJson(seed);
  const current = fs.existsSync(target) ? fs.readFileSync(target, 'utf8') : null;
  if (check) {
    if (current !== bytes) throw new Error(`Official i18n seed is stale: ${target}`);
    return { changed: false, outputPath: target, seed, bytes };
  }
  if (current !== bytes) {
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, bytes);
  }
  return { changed: current !== bytes, outputPath: target, seed, bytes };
}

function parseCli(args) {
  const options = { check: false };
  for (let index = 0; index < args.length; index += 1) {
    if (args[index] === '--check') options.check = true;
    else if (args[index] === '--output' && args[index + 1]) options.outputPath = path.resolve(args[++index]);
    else throw new Error('usage: node scripts/i18n-catalog-publisher.mjs [--check] [--output PATH]');
  }
  return options;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const result = publishCatalogSeed(parseCli(process.argv.slice(2)));
  process.stdout.write(`${result.changed ? 'Published' : 'Verified'} ${result.outputPath}\n`);
}
