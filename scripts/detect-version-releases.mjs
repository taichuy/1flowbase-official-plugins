import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const MANIFEST_PATH_PATTERN = /^runtime-extensions\/@taichuy\/([^/]+)\/manifest\.yaml$/;
const PROVIDER_PATH_PATTERN = /^runtime-extensions\/@taichuy\/([^/]+)\/(.+)$/;
const OBSERVATION_CONSUMERS = ['openai', 'openai_compatible', 'anthropic', 'gemini', 'deepseek', 'aliyun_bailian', 'chatgpt-codex'];
const observationSdkInput = (path) => path.startsWith('sdk/provider-observation/') && !path.includes('/target/');

const NON_PACKAGE_PATH_PATTERN = /^(?:readme|demo|tests|target)\//;

function providerPackageInput(path) {
  const match = path.match(PROVIDER_PATH_PATTERN);
  return match && !NON_PACKAGE_PATH_PATTERN.test(match[2]);
}

export function parseManifestVersion(content) {
  if (!content) {
    return '';
  }
  if (content.trimStart().startsWith('{')) {
    return JSON.parse(content).version || '';
  }

  const match = content.match(/^version:\s*(.+)$/m);
  return match ? match[1].trim() : '';
}

function parseManifestSlotCodes(content) {
  if (content.trimStart().startsWith('{')) {
    return JSON.parse(content).slot_codes ?? null;
  }
  const lines = content.split(/\r?\n/);
  const fieldIndex = lines.findIndex((line) => /^slot_codes:\s*/.test(line));
  if (fieldIndex < 0) return null;

  const inline = lines[fieldIndex].replace(/^slot_codes:\s*/, '').trim();
  if (inline.startsWith('[') && inline.endsWith(']')) {
    return inline
      .slice(1, -1)
      .split(',')
      .map((value) => value.trim())
      .filter(Boolean);
  }

  const values = [];
  for (const line of lines.slice(fieldIndex + 1)) {
    const match = line.match(/^\s{2}-\s+(.+?)\s*$/);
    if (!match) break;
    values.push(match[1]);
  }
  return values;
}

function hasManagedServiceOperations(content) {
  if (content.trimStart().startsWith('{')) {
    const manifest = JSON.parse(content);
    if (manifest.manifest_version !== 2 || manifest.consumption_kind !== 'runtime_extension') return false;
    const contributions = manifest.managed?.module?.contributions;
    const bindings = manifest.managed?.execution_bindings;
    return Array.isArray(contributions) && Array.isArray(bindings)
      && contributions.some(contribution => contribution.point_id === '1flowbase.managed-service.operation'
        && typeof contribution.contribution_id === 'string'
        && bindings.some(binding => binding.contribution_id === contribution.contribution_id));
  }
  if (!/^manifest_version:\s*2\s*$/m.test(content)
      || !/^consumption_kind:\s*runtime_extension\s*$/m.test(content)) return false;
  const lines = content.split(/\r?\n/);
  const start = lines.findIndex(line => /^managed:\s*$/.test(line));
  if (start < 0) return false;
  const block = [];
  for (const line of lines.slice(start + 1)) {
    if (line.trim() && !/^\s/.test(line)) break;
    block.push(line);
  }
  // Match only the owned managed block; a settings/API declaration elsewhere
  // does not establish an executable managed-service contribution.
  const managed = block.join('\n');
  const module = managed.split(/^  execution_bindings:\s*$/m)[0];
  const bindings = managed.split(/^  execution_bindings:\s*$/m)[1] ?? '';
  const bindingIds = new Set([...bindings.matchAll(/^    - contribution_id:\s*(\S+)\s*$/gm)].map(match => match[1]));
  return /^  module:\s*$/m.test(module) && /^    contributions:\s*$/m.test(module)
    && module.split(/^      - contribution_id:\s*/m).slice(1).some(contribution => {
      const id = contribution.split(/\r?\n/, 1)[0].trim();
      return bindingIds.has(id) && /^        point_id:\s*1flowbase\.managed-service\.operation\s*$/m.test(contribution);
    });
}

function isPublishableRuntimeManifest(content) {
  if (hasManagedServiceOperations(content)) return true;
  // Managed notification contributors are runtime extensions without a provider slot.
  // Their existing managed binding, rather than a synthetic model-provider slot,
  // supplies the execution contract.
  const manifest = content.trimStart().startsWith('{') ? JSON.parse(content) : null;
  if (manifest?.manifest_version === 2
      && manifest.consumption_kind === 'runtime_extension'
      && Array.isArray(manifest.notification_types) && manifest.notification_types.length > 0
      && manifest.managed && typeof manifest.managed === 'object') {
    return true;
  }
  if (/^manifest_version:\s*2\s*$/m.test(content)
      && /^consumption_kind:\s*runtime_extension\s*$/m.test(content)
      && /^notification_types:\s*\n\s+-\s/m.test(content)
      && /^managed:\s*$/m.test(content)) {
    return true;
  }
  const slotCodes = parseManifestSlotCodes(content);
  return slotCodes === null || slotCodes.some((slotCode) =>
    ['model_provider', 'network_egress_provider', 'provider_distribution_rule'].includes(slotCode)
  );
}

export function detectVersionReleases(changes) {
  const affectedProviders = new Set(
    changes.flatMap(({ path }) => {
      const match = path.match(PROVIDER_PATH_PATTERN);
      return observationSdkInput(path) ? OBSERVATION_CONSUMERS : match && providerPackageInput(path) ? [match[1]] : [];
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
        if (changes.some(({ path }) => observationSdkInput(path))) {
          throw new Error(`provider_version_bump_required: shared observation SDK changed without manifest version for ${providerCode}`);
        }
        return [];
      }
      if (!isPublishableRuntimeManifest(afterContent)) {
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
    'sdk/provider-observation',
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
    'sdk/provider-observation',
  ]);
  return output
    .split('\n')
    .filter((path) => providerPackageInput(path) || observationSdkInput(path));
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
      return observationSdkInput(path) ? OBSERVATION_CONSUMERS : match && providerPackageInput(path) ? [match[1]] : [];
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
