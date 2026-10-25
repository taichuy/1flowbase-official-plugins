import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// Existing packages retain exactly the original six-target matrix. A plugin can
// narrow or extend it using a release-only sidecar, never host manifest fields.
export const DEFAULT_RUST_TARGETS = Object.freeze([
  'x86_64-unknown-linux-musl', 'aarch64-unknown-linux-musl',
  'x86_64-apple-darwin', 'aarch64-apple-darwin',
  'x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc',
]);
const targets = {
  'x86_64-unknown-linux-musl': { runs_on: 'ubuntu-latest', os: 'linux', arch: 'amd64', libc: 'musl' },
  'aarch64-unknown-linux-musl': { runs_on: 'ubuntu-latest', os: 'linux', arch: 'arm64', libc: 'musl' },
  'x86_64-apple-darwin': { runs_on: 'macos-latest', os: 'darwin', arch: 'amd64', libc: '' },
  'aarch64-apple-darwin': { runs_on: 'macos-latest', os: 'darwin', arch: 'arm64', libc: '' },
  'x86_64-pc-windows-msvc': { runs_on: 'windows-latest', os: 'windows', arch: 'amd64', libc: 'msvc' },
  'aarch64-pc-windows-msvc': { runs_on: 'windows-latest', os: 'windows', arch: 'arm64', libc: 'msvc' },
  'x86_64-unknown-linux-gnu': { runs_on: 'ubuntu-22.04', os: 'linux', arch: 'amd64', libc: 'gnu' },
};
const repoRoot = path.resolve(import.meta.dirname, '../..');

export function readReleaseTargets(pluginDir, rootDir = repoRoot) {
  if (typeof pluginDir !== 'string' || !/^runtime-extensions\/@taichuy\/[A-Za-z0-9_-]+$/.test(pluginDir)) {
    throw new Error('Invalid release plugin directory');
  }
  const root = fs.realpathSync(rootDir);
  const directory = fs.realpathSync(path.join(root, pluginDir));
  if (path.relative(root, directory).split(path.sep).join('/') !== pluginDir) {
    throw new Error('Release plugin directory must not escape or alias its source path');
  }
  const sidecar = path.join(directory, 'release-targets.json');
  let selected = DEFAULT_RUST_TARGETS;
  const sidecarInfo = fs.lstatSync(sidecar, { throwIfNoEntry: false });
  if (sidecarInfo) {
    if (!sidecarInfo.isFile() || fs.realpathSync(sidecar) !== sidecar) throw new Error('Release target sidecar must not be a symlink');
    const value = JSON.parse(fs.readFileSync(sidecar, 'utf8'));
    if (!value || typeof value !== 'object' || Array.isArray(value)
        || value.schema_version !== 1
        || Object.keys(value).some(key => !['schema_version', 'rust_targets'].includes(key))
        || !Array.isArray(value.rust_targets) || value.rust_targets.length === 0
        || value.rust_targets.some(target => typeof target !== 'string' || !Object.hasOwn(targets, target))
        || new Set(value.rust_targets).size !== value.rust_targets.length) {
      throw new Error(`Invalid release-targets.json for ${pluginDir}`);
    }
    selected = value.rust_targets;
  }
  return selected.map(rust_target => ({ ...targets[rust_target], rust_target }));
}

export function buildPackageMatrix(releases, rootDir = repoRoot) {
  if (!Array.isArray(releases)) throw new Error('Expected a release array');
  return { include: releases.flatMap(release => readReleaseTargets(release.plugin_dir, rootDir).map(target => ({ ...release, ...target }))) };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const input = process.argv[2];
  if (!input) throw new Error('Usage: node scripts/provider-release/targets.mjs <release-matrix-json>');
  process.stdout.write(`${JSON.stringify(buildPackageMatrix(JSON.parse(input).include))}\n`);
}
