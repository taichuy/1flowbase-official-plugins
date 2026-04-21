import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const MANIFEST_PATH_PATTERN = /^runtime-extensions\/model-providers\/([^/]+)\/manifest\.yaml$/;

export function parseManifestVersion(content) {
  if (!content) {
    return '';
  }

  const match = content.match(/^version:\s*(.+)$/m);
  return match ? match[1].trim() : '';
}

export function detectVersionReleases(manifestChanges) {
  return manifestChanges
    .flatMap(({ path, beforeContent = '', afterContent = '' }) => {
      const match = path.match(MANIFEST_PATH_PATTERN);
      if (!match) {
        return [];
      }

      const providerCode = match[1];
      const previousVersion = parseManifestVersion(beforeContent);
      const nextVersion = parseManifestVersion(afterContent);

      if (!nextVersion || previousVersion === nextVersion) {
        return [];
      }

      return [
        {
          plugin_dir: `runtime-extensions/model-providers/${providerCode}`,
          provider_code: providerCode,
          release_tag: `${providerCode}-v${nextVersion}`,
          version: nextVersion,
        },
      ];
    })
    .sort((left, right) => left.provider_code.localeCompare(right.provider_code));
}

function runGit(args) {
  return execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
}

function refExists(ref) {
  if (!ref || /^0+$/.test(ref)) {
    return false;
  }

  try {
    runGit(['rev-parse', '--verify', ref]);
    return true;
  } catch {
    return false;
  }
}

function listManifestPaths(baseRef, headRef) {
  if (refExists(baseRef)) {
    const output = runGit([
      'diff',
      '--name-only',
      '--diff-filter=AMRT',
      baseRef,
      headRef,
      '--',
      'runtime-extensions/model-providers/*/manifest.yaml',
    ]);

    return output ? output.split('\n').filter(Boolean) : [];
  }

  const output = runGit([
    'ls-tree',
    '-r',
    '--name-only',
    headRef,
    '--',
    'runtime-extensions/model-providers',
  ]);
  return output
    .split('\n')
    .filter((path) => MANIFEST_PATH_PATTERN.test(path));
}

function readFileAtRef(ref, path) {
  if (!refExists(ref)) {
    return '';
  }

  try {
    return runGit(['show', `${ref}:${path}`]);
  } catch {
    return '';
  }
}

export function detectVersionReleasesBetweenRefs(baseRef, headRef) {
  const manifestChanges = listManifestPaths(baseRef, headRef).map((path) => ({
    path,
    beforeContent: readFileAtRef(baseRef, path),
    afterContent: readFileAtRef(headRef, path),
  }));

  return detectVersionReleases(manifestChanges);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const baseRef = process.argv[2];
  const headRef = process.argv[3];

  if (!headRef) {
    throw new Error(
      'Usage: node scripts/detect-version-releases.mjs <base-ref-or-empty> <head-ref>'
    );
  }

  const releases = detectVersionReleasesBetweenRefs(baseRef, headRef);
  process.stdout.write(`${JSON.stringify({ include: releases })}\n`);
}
