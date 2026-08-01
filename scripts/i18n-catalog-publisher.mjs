import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const SOURCE_SCHEMA_VERSION = '1flowbase.i18n-catalog-source/v2';
export const SEED_SCHEMA_VERSION = '1flowbase.i18n-catalog-seed/v2';

const LOCALE_PATTERN = /^[a-z]{2,3}(?:_[A-Z][A-Za-z]{1,7})?$/;
const CHECKSUM_PATTERN = /^sha256:[a-f0-9]{64}$/;
const NAMED_PLACEHOLDER = /\{([A-Za-z_][A-Za-z0-9_.-]*)\}/g;
const FORBIDDEN_CONTENT = /<\/?[A-Za-z][^>]*>|javascript\s*:|\$\{|=>|\bfunction\s*\(|\bon[A-Za-z]+\s*=|!?\[[^\]]*\]\([^)]+\)/i;

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
    ['schema_version', 'catalog_version', 'source_locale', 'locales', 'files', 'generated_at'],
    'i18n/catalog.json'
  );
  if (source.schema_version !== SOURCE_SCHEMA_VERSION) fail(`unsupported schema_version ${source.schema_version}`);
  if (typeof source.catalog_version !== 'string' || !/^\d+\.\d+\.\d+$/.test(source.catalog_version)) {
    fail('catalog_version must be semantic version x.y.z');
  }
  if (source.source_locale !== 'en_US') fail('source_locale must be en_US');
  if (!Array.isArray(source.locales) || !source.locales.includes(source.source_locale) || !source.locales.includes('zh_Hans')) {
    fail('locales must contain en_US and zh_Hans');
  }
  if (new Set(source.locales).size !== source.locales.length || source.locales.some((locale) => !LOCALE_PATTERN.test(locale))) {
    fail('locales must be unique valid locale identifiers');
  }
  if (!Array.isArray(source.files) || source.files.length === 0) fail('files must be a non-empty array');
  if (typeof source.generated_at !== 'string' || Number.isNaN(Date.parse(source.generated_at))) {
    fail('generated_at must be an ISO timestamp');
  }
}

function normalizeLocaleFile(document, relativePath, locale) {
  if (!isRecord(document)) fail(`${relativePath} must be an object keyed by immutable English key`);
  const normalized = {};
  for (const key of Object.keys(document).sort((left, right) => left.localeCompare(right))) {
    validateText(key, `${relativePath} key ${JSON.stringify(key)}`);
    const translation = document[key];
    validateText(translation, `${relativePath} translation for ${JSON.stringify(key)}`);
    if (!sameStrings(placeholders(key), placeholders(translation))) {
      fail(`${relativePath} placeholder set mismatch for ${JSON.stringify(key)}`);
    }
    normalized[key] = translation;
  }
  if (Object.keys(normalized).length === 0) fail(`${relativePath} must contain at least one ${locale} translation`);
  return normalized;
}

function expectedLocaleFromPath(relativePath) {
  const match = relativePath.match(/^i18n\/(?!dist(?:\/|$))(.+)\/([^/]+)\.json$/);
  return match && LOCALE_PATTERN.test(match[2]) ? match[2] : null;
}

function sourceGroupPath(relativePath) {
  return relativePath.slice(0, relativePath.lastIndexOf('/'));
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
        const relativePath = path.relative(repoRoot, filePath).split(path.sep).join('/');
        if (expectedLocaleFromPath(relativePath)) discovered.push(relativePath);
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
  const declaredFiles = new Map();
  for (const file of source.files) {
    validateExactKeys(file, ['locale', 'path', 'sha256'], 'catalog file entry');
    if (!locales.includes(file.locale)) fail(`unregistered locale for ${file.path}`);
    const pathLocale = expectedLocaleFromPath(file.path);
    if (pathLocale !== file.locale) fail(`file path locale must be ${file.locale}: ${file.path}`);
    if (!CHECKSUM_PATTERN.test(file.sha256)) fail(`invalid SHA-256 for ${file.path}`);
    if (declaredFiles.has(file.path)) fail(`duplicate file entry ${file.path}`);
    declaredFiles.set(file.path, file);
  }

  const groups = new Map();
  for (const file of declaredFiles.values()) {
    const group = sourceGroupPath(file.path);
    if (!groups.has(group)) groups.set(group, new Map());
    const filesByLocale = groups.get(group);
    if (filesByLocale.has(file.locale)) fail(`duplicate ${file.locale} file in source group ${group}`);
    filesByLocale.set(file.locale, file);
  }

  const globalTranslations = new Map();
  const normalizedFiles = [];
  for (const group of [...groups.keys()].sort((left, right) => left.localeCompare(right))) {
    const filesByLocale = groups.get(group);
    const groupDocuments = new Map();
    for (const locale of locales) {
      const file = filesByLocale.get(locale);
      if (!file) fail(`source group ${group} is missing locale ${locale}`);
      const document = normalizeLocaleFile(readJson(path.join(repoRoot, file.path)), file.path, locale);
      const checksum = sha256(stableJson(document));
      if (file.sha256 !== checksum) fail(`checksum mismatch for ${file.path}: expected ${file.sha256}, got ${checksum}`);
      groupDocuments.set(locale, document);
      normalizedFiles.push({ locale, path: file.path, sha256: checksum, keys: Object.keys(document) });
    }

    const sourceKeys = Object.keys(groupDocuments.get(source.source_locale));
    for (const locale of locales) {
      const keys = Object.keys(groupDocuments.get(locale));
      if (!sameStrings(keys, sourceKeys)) fail(`${group}/${locale}.json keys must exactly match ${source.source_locale}`);
      for (const key of keys) {
        if (!globalTranslations.has(key)) globalTranslations.set(key, new Map());
        const translations = globalTranslations.get(key);
        const value = groupDocuments.get(locale)[key];
        if (translations.has(locale) && translations.get(locale).value !== value) {
          const previous = translations.get(locale);
          fail(`conflicting global key ${JSON.stringify(key)} for ${locale}: ${previous.path} and ${group}/${locale}.json`);
        }
        if (!translations.has(locale)) translations.set(locale, { value, path: `${group}/${locale}.json` });
      }
    }
  }

  if (declaredFiles.size !== normalizedFiles.length) fail('files contains an entry outside the source locale matrix');
  const discoveredFiles = discoverLocaleFiles(repoRoot);
  const registeredFiles = [...declaredFiles.keys()].sort((left, right) => left.localeCompare(right));
  if (!sameStrings(discoveredFiles, registeredFiles)) {
    fail('source tree locale files must exactly match the catalog file registry');
  }

  const messages = [...globalTranslations.keys()]
    .sort((left, right) => left.localeCompare(right))
    .map((key) => ({
      key,
      translations: Object.fromEntries(locales.map((locale) => {
        const translation = globalTranslations.get(key).get(locale);
        if (!translation) fail(`global key ${JSON.stringify(key)} is missing locale ${locale}`);
        return [locale, translation.value];
      })),
    }));
  const semantic = {
    catalog_version: source.catalog_version,
    source_locale: source.source_locale,
    locales,
    files: normalizedFiles,
    messages,
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
      files: normalizedFiles,
      generated_at: generatedAt,
      semantic_sha256: semanticDigest,
    },
    messages,
  };
}

function seedFileDocument(seed, file) {
  const messages = new Map(seed.messages.map((message) => [message.key, message]));
  return Object.fromEntries(file.keys.map((key) => {
    const message = messages.get(key);
    if (!message) fail(`seed is missing global key ${JSON.stringify(key)} for ${file.path}`);
    const translation = message.translations?.[file.locale];
    if (typeof translation !== 'string') fail(`seed is missing ${file.locale} translation for ${JSON.stringify(key)}`);
    return [key, translation];
  }));
}

export function verifyCatalogSeed(seed) {
  if (!isRecord(seed) || !isRecord(seed.manifest) || !Array.isArray(seed.messages)) fail('seed shape is invalid');
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
    files: seed.manifest.files,
    messages: seed.messages,
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
