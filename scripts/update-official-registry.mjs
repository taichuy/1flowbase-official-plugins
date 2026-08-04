import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

function normalizeRegistryEntry(entry) {
  if (typeof entry?.publisher_namespace !== 'string' || entry.publisher_namespace.length === 0) {
    throw new Error('publisher_namespace must be a non-empty string');
  }
  if (typeof entry?.manifest_locator !== 'string' || entry.manifest_locator.length === 0) {
    throw new Error('manifest_locator must be a non-empty string');
  }
  if (typeof entry?.provider_code !== 'string' || entry.provider_code.length === 0) {
    throw new Error('provider_code must be a non-empty string');
  }
  return {
    ...entry,
    plugin_type: entry?.plugin_type || 'model_provider',
    i18n_summary: entry?.i18n_summary || {
      default_locale: null,
      available_locales: [],
      bundles: {},
    },
    artifacts: Array.isArray(entry?.artifacts) ? entry.artifacts : [],
    slot_codes: normalizeRegistryList(entry?.slot_codes, 'slot_codes'),
    keywords: normalizeRegistryList(entry?.keywords, 'keywords'),
  };
}

function normalizeRegistryList(value, field) {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.some((item) => typeof item !== 'string' || item.trim().length === 0)) {
    throw new Error(`${field} must be an array of non-empty strings`);
  }
  return [...new Set(value.map((item) => item.trim()))]
    .sort(compareText);
}

function compareText(left, right) {
  if (left < right) return -1;
  if (left > right) return 1;
  return 0;
}

function registryIdentityKey(entry) {
  return `${entry?.publisher_namespace || ''}\u0000${entry?.provider_code || ''}`;
}

export function upsertRegistryEntry(registry, entry) {
  const plugins = Array.isArray(registry?.plugins) ? registry.plugins : [];
  const normalizedEntry = normalizeRegistryEntry(entry);

  return {
    version: 1,
    generated_at: new Date().toISOString(),
    plugins: [
      ...plugins.filter((item) => registryIdentityKey(item) !== registryIdentityKey(normalizedEntry)),
      normalizedEntry,
    ].sort((left, right) => compareText(registryIdentityKey(left), registryIdentityKey(right))),
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const filePath = process.argv[2];
  const entryJson = process.argv[3];

  if (!filePath || !entryJson) {
    throw new Error('用法：node scripts/update-official-registry.mjs <registry-file> <entry-json>');
  }

  const registry = JSON.parse(fs.readFileSync(filePath, 'utf8'));
  const entry = JSON.parse(entryJson);
  fs.writeFileSync(filePath, `${JSON.stringify(upsertRegistryEntry(registry, entry), null, 2)}\n`);
}
