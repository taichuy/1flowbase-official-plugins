import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const MANIFEST_PATH_PATTERN = /^runtime-extensions\/@taichuy\/([^/]+)\/manifest\.yaml$/;
const PROVIDER_PATH_PATTERN = /^runtime-extensions\/@taichuy\/([^/]+)\/(.+)$/;
const NON_PACKAGE_PATH_PATTERN = /^(?:readme|demo|tests|target)\//;

function providerPackageInput(path) {
  const match = path.match(PROVIDER_PATH_PATTERN);
  return match && !NON_PACKAGE_PATH_PATTERN.test(match[2]);
}

export function parseManifestVersion(content) {
  if (!content) {
    return '';
  }

  const match = content.match(/^version:\s*(.+)$/m);
  return match ? match[1].trim() : '';
}

export function detectVersionReleases(changes) {
  const affectedProviders = new Set(
    changes.flatMap(({ path }) => {
      const match = path.match(PROVIDER_PATH_PATTERN);
      return match && providerPackageInput(path) ? [match[1]] : [];
    })
  );
  const manifestChanges = new Map(
    changes.flatMap((change) => {
      const match = change.path.match(MANIFEST_PATH_PATTERN);
      return match ? [[match[1], change]] : [];
    })
  );

  return [...affectedProviders]
    .flatMap((providerCode) => {
      const { beforeContent = '', afterContent = '' } =
        manifestChanges.get(providerCode) ?? {};
      const previousVersion = parseManifestVersion(beforeContent);
      const nextVersion = parseManifestVersion(afterContent);

      if (!nextVersion) {
        return [];
      }
      if (previousVersion === nextVersion) {
        throw new Error(
          `provider_version_bump_required: ${providerCode} changed without updating manifest version ${nextVersion}`
        );
      }

      return [
        {
          plugin_dir: `runtime-extensions/@taichuy/${providerCode}`,
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

function listChangedProviderPaths(baseRef, headRef) {
  if (refExists(baseRef)) {
    const output = runGit([
      'diff',
      '--name-only',
      '--diff-filter=ACMRTD',
      baseRef,
      headRef,
      '--',
      'runtime-extensions/@taichuy',
    ]);

    return output ? output.split('\n').filter(Boolean) : [];
  }

  const output = runGit([
    'ls-tree',
    '-r',
    '--name-only',
    headRef,
    '--',
    'runtime-extensions/@taichuy',
  ]);
  return output
    .split('\n')
    .filter((path) => providerPackageInput(path));
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
  const changedPaths = listChangedProviderPaths(baseRef, headRef);
  const providerCodes = new Set(
    changedPaths.flatMap((path) => {
      const match = path.match(PROVIDER_PATH_PATTERN);
      return match && providerPackageInput(path) ? [match[1]] : [];
    })
  );
  const manifestChanges = [...providerCodes].map((providerCode) => {
    const path = `runtime-extensions/@taichuy/${providerCode}/manifest.yaml`;
    return {
      path,
      beforeContent: readFileAtRef(baseRef, path),
      afterContent: readFileAtRef(headRef, path),
    };
  });

  return detectVersionReleases([
    ...changedPaths.map((path) => ({ path })),
    ...manifestChanges,
  ]);
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
