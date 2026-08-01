import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { CATALOG_CATEGORIES, updateExtensionCatalog } from './extension-catalog.mjs';

function parseArguments(argv) {
  const categories = [];
  let check = false;
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--check') check = true;
    else if (argument === '--category') categories.push(argv[++index]);
    else throw new Error(`unknown argument ${argument}`);
  }
  for (const category of categories) {
    if (!CATALOG_CATEGORIES.includes(category)) throw new Error(`unsupported category ${category}`);
  }
  return { check, categories: categories.length > 0 ? categories : CATALOG_CATEGORIES };
}

export function runExtensionCatalogUpdate(argv = [], repoRoot = path.resolve(import.meta.dirname, '..')) {
  const options = parseArguments(argv);
  const results = updateExtensionCatalog({ repoRoot, ...options });
  for (const result of results) {
    console.log(`${result.category}: ${result.totalEntries} entries, ${result.pageCount} pages${result.changed ? ' (updated)' : ''}`);
  }
  return results;
}

if (path.resolve(process.argv[1] || '') === fileURLToPath(import.meta.url)) {
  runExtensionCatalogUpdate(process.argv.slice(2));
}
