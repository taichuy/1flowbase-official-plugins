import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

export function upsertRegistryEntry(registry, entry) {
  const plugins = Array.isArray(registry?.plugins) ? registry.plugins : [];

  return {
    version: 1,
    generated_at: new Date().toISOString(),
    plugins: [...plugins.filter((item) => item?.plugin_id !== entry.plugin_id), entry].sort(
      (left, right) => left.plugin_id.localeCompare(right.plugin_id)
    ),
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
