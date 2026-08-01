import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  buildCategoryCatalog,
  discoverCatalogEntries,
  updateCategoryCatalog,
} from './extension-catalog.mjs';

// Compatibility entry point for the existing AgentFlow publisher workflow.
export function discoverAgentFlowTemplates(repoRoot) {
  return discoverCatalogEntries({ repoRoot, category: 'agent-flow' });
}

export function buildAgentFlowCatalog(options = {}) {
  return buildCategoryCatalog({ ...options, category: 'agent-flow' });
}

export function updateAgentFlowCatalog(options = {}) {
  return updateCategoryCatalog({
    ...options,
    repoRoot: options.repoRoot || path.resolve(import.meta.dirname, '..'),
    category: 'agent-flow',
  });
}

if (path.resolve(process.argv[1] || '') === fileURLToPath(import.meta.url)) {
  const result = updateAgentFlowCatalog();
  console.log(`AgentFlow catalog: ${result.totalEntries} entries across ${result.pageCount} pages${result.changed ? ' (updated)' : ''}.`);
}
