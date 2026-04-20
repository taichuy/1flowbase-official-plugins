import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

function normalizeRegistryEntry(entry) {
  return {
    ...entry,
    plugin_type: entry?.plugin_type || 'model_provider',
    i18n_summary: entry?.i18n_summary || {
      default_locale: null,
      available_locales: [],
      bundles: {},
    },
    artifacts: Array.isArray(entry?.artifacts) ? entry.artifacts : [],
  };
}

export function upsertRegistryEntry(registry, entry) {
  const plugins = Array.isArray(registry?.plugins) ? registry.plugins : [];
  const normalizedEntry = normalizeRegistryEntry(entry);

  return {
    version: 1,
    generated_at: new Date().toISOString(),
    plugins: [
      ...plugins.filter((item) => item?.provider_code !== normalizedEntry.provider_code),
      normalizedEntry,
    ].sort((left, right) => left.provider_code.localeCompare(right.provider_code)),
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
