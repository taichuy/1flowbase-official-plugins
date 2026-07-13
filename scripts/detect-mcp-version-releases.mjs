import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const BUNDLE_PATH_PATTERN = /^mcp\/([^/]+)\/([^/]+)\//;

function manifestVersion(content) {
  if (!content) return '';
  return JSON.parse(content).bundle_version ?? '';
}

export function detectMcpVersionReleases(changes) {
  return changes
    .map(({ organization, bundleId, beforeManifest = '', afterManifest = '' }) => {
      const previousVersion = manifestVersion(beforeManifest);
      const version = manifestVersion(afterManifest);
      if (!version) throw new Error(`missing bundle_version for ${organization}/${bundleId}`);
      if (previousVersion === version) {
        throw new Error(
          `MCP bundle ${organization}/${bundleId} changed without a bundle_version bump`
        );
      }
      return {
        bundle_dir: `mcp/${organization}/${bundleId}`,
        organization,
        bundle_id: bundleId,
        version,
        release_tag: `mcp-${organization}-${bundleId}-v${version}`,
        asset_name: `${organization}-${bundleId}-v${version}.zip`
      };
    })
    .sort((left, right) => left.bundle_dir.localeCompare(right.bundle_dir));
}

function git(args) {
  return execFileSync('git', args, {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe']
  }).trim();
}

function refExists(ref) {
  if (!ref || /^0+$/.test(ref)) return false;
  try {
    git(['rev-parse', '--verify', ref]);
    return true;
  } catch {
    return false;
  }
}

function readAt(ref, filePath) {
  if (!refExists(ref)) return '';
  try {
    return git(['show', `${ref}:${filePath}`]);
  } catch {
    return '';
  }
}

export function detectMcpVersionReleasesBetweenRefs(baseRef, headRef) {
  const paths = refExists(baseRef)
    ? git(['diff', '--name-only', '--diff-filter=AMRT', baseRef, headRef, '--', 'mcp'])
    : git(['ls-tree', '-r', '--name-only', headRef, '--', 'mcp']);
  const bundleKeys = new Map();
  for (const filePath of paths.split('\n').filter(Boolean)) {
    const match = filePath.match(BUNDLE_PATH_PATTERN);
    if (match) bundleKeys.set(`${match[1]}/${match[2]}`, [match[1], match[2]]);
  }
  return detectMcpVersionReleases(
    [...bundleKeys.values()].map(([organization, bundleId]) => {
      const manifestPath = `mcp/${organization}/${bundleId}/manifest.json`;
      return {
        organization,
        bundleId,
        beforeManifest: readAt(baseRef, manifestPath),
        afterManifest: readAt(headRef, manifestPath)
      };
    })
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [baseRef, headRef] = process.argv.slice(2);
  if (!headRef) {
    throw new Error(
      'usage: node scripts/detect-mcp-version-releases.mjs <base-ref-or-empty> <head-ref>'
    );
  }
  process.stdout.write(
    `${JSON.stringify({ include: detectMcpVersionReleasesBetweenRefs(baseRef, headRef) })}\n`
  );
}
