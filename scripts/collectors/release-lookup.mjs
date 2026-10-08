import { execFileSync, spawnSync } from 'node:child_process';

// Only an explicit tag-level 404 permits creating a new immutable release.
export function lookupCollectorRelease(repository, tag, { exec = execFileSync, spawn = spawnSync } = {}) {
  try {
    const release = JSON.parse(exec('gh', ['release', 'view', tag, '--repo', repository,
      '--json', 'targetCommitish,assets', '--jq', '{targetCommitish,assets:[.assets[]|{name}]}'],
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }));
    if (!release || typeof release.targetCommitish !== 'string' || !release.targetCommitish ||
        !Array.isArray(release.assets) || release.assets.some(asset => typeof asset?.name !== 'string' || !asset.name)) {
      throw new Error('Malformed collector release metadata');
    }
    return release;
  } catch {
    // Keep stdout to HTTP headers. Never read the repository's complete release history.
    const probe = spawn('gh', ['api', `repos/${repository}/releases/tags/${encodeURIComponent(tag)}`,
      '--include', '--silent'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
    const status = /^HTTP\/\S+\s+(\d{3})\b/m.exec(probe.stdout || '')?.[1];
    if (!probe.error && probe.status === 1 && status === '404') return undefined;
    throw new Error(`Could not read collector release ${tag}; tag lookup HTTP ${status || 'unavailable'}`);
  }
}
