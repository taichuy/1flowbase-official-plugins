import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { listCollectors, PLATFORM_TARGETS, archiveName } from './catalog.mjs';
import { lookupCollectorRelease } from './release-lookup.mjs';
import { distributionName } from './distribution.mjs';

// These files produce the native binary or signed bundle. Catalog output and
// release orchestration do not change the bytes of an already published version.
export function collectorPayloadPaths(collector) {
  return [collector.plugin_dir, `:(exclude)${collector.plugin_dir}/catalog-entry.json`,
    `:(exclude)${collector.plugin_dir}/_assets/icon.svg`,
    'sdk/agent-logs-collector', 'installers/client-collectors',
    'scripts/collectors/catalog.mjs', 'scripts/collectors/package.mjs', 'scripts/collectors/distribution.mjs'];
}

export function planCollectorRelease({ repoRoot, collector, repository, sourceSha,
  lookup = lookupCollectorRelease, exec = execFileSync }) {
  const existing = lookup(repository, collector.release_tag);
  if (!existing) return { action: 'build', sourceSha };
  if (!/^[0-9a-f]{40}$/i.test(existing.targetCommitish) || !/^[0-9a-f]{40}$/i.test(sourceSha || '')) {
    throw new Error('Collector release requires immutable source commit IDs');
  }
  const name = distributionName(collector);
  if (existing.assets.length !== 1 || existing.assets[0].name !== name) throw new Error('Published collector asset identity differs');
  const changes = exec('git', ['diff', '--name-only', existing.targetCommitish, sourceSha,
    '--', ...collectorPayloadPaths(collector)], { cwd: repoRoot, encoding: 'utf8' });
  if (changes.trim()) throw new Error(`Collector ${collector.collector_code} payload changed; bump its version before publishing`);
  const catalog = path.join(repoRoot, collector.plugin_dir, 'catalog-entry.json');
  const entry = fs.existsSync(catalog) ? JSON.parse(fs.readFileSync(catalog, 'utf8')) : null;
  if (entry?.version === collector.version) {
    const locator = `https://github.com/${repository}/releases/download/${collector.release_tag}/${name}`;
    if (entry.download_locator?.kind !== 'release_asset' || entry.download_locator.locator !== locator ||
        typeof entry.checksum !== 'string' || entry.signature?.algorithm !== 'ed25519' ||
        !entry.signature.key_id || !entry.signature.signature) throw new Error('Published collector catalog identity is invalid');
    return { action: 'reuse', sourceSha: existing.targetCommitish, existing, entry };
  }
  // Preserve recovery after a release was uploaded but its catalog commit failed:
  // rebuild the same source identity and verify the published bytes before use.
  return { action: 'build', sourceSha: existing.targetCommitish, existing };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const repoRoot = path.resolve(import.meta.dirname, '../..');
  const repository = process.env.GITHUB_REPOSITORY || 'taichuy/1flowbase-official-plugins';
  const sourceSha = process.env.GITHUB_SHA || execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repoRoot, encoding: 'utf8' }).trim();
  const pending = listCollectors(repoRoot).filter(collector => planCollectorRelease({ repoRoot, collector, repository, sourceSha }).action === 'build');
  process.stdout.write(JSON.stringify({ include: pending.flatMap(collector => PLATFORM_TARGETS.map(platform => ({
    ...collector, ...platform, archive_name: archiveName(collector, platform),
  }))) }));
}
