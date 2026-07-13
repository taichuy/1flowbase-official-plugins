import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export function setMcpArtifactChecksum(repositoryRoot, releaseTag, checksum) {
  if (!/^sha256:[a-f0-9]{64}$/.test(checksum)) {
    throw new Error('artifact checksum must be sha256:<64 lowercase hex>');
  }
  const catalogPath = path.join(repositoryRoot, 'mcp', 'catalog.json');
  const catalog = JSON.parse(readFileSync(catalogPath, 'utf8'));
  const entry = catalog.bundles.find((candidate) => candidate.release_tag === releaseTag);
  if (!entry) throw new Error(`MCP release tag not found in catalog: ${releaseTag}`);
  entry.artifact_sha256 = checksum;
  writeFileSync(catalogPath, `${JSON.stringify(catalog, null, 2)}\n`);
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : null;
if (invokedPath === fileURLToPath(import.meta.url)) {
  const [releaseTag, checksum] = process.argv.slice(2);
  if (!releaseTag || !checksum) {
    throw new Error('usage: node scripts/set-mcp-artifact-checksum.mjs <release-tag> <sha256:...>');
  }
  const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  setMcpArtifactChecksum(repositoryRoot, releaseTag, checksum);
}
