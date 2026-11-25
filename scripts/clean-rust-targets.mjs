#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { lstatSync, readdirSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const cacheSignature = 'Signature: 8a477f597d28d172789f06886806bc55';
const usage = `Usage: node scripts/clean-rust-targets.mjs [--dry-run | --help]

Remove Cargo build caches under the repository root, runtime plugins, and SDK crates.
External CARGO_TARGET_DIR paths are not used. Stop active builds before cleaning.
Use --dry-run to list the directories that would be removed.`;

function isDirectory(path) {
  return lstatSync(path, { throwIfNoEntry: false })?.isDirectory();
}

function isFile(path) {
  return lstatSync(path, { throwIfNoEntry: false })?.isFile();
}

function findTargets(root) {
  const candidates = [join(root, 'target')];
  for (const parent of [join(root, 'runtime-extensions', '@taichuy'), join(root, 'sdk')]) {
    // Do not enumerate crates through a symlink, including in a parent path.
    if (!isDirectory(parent) || realpathSync(parent) !== parent) continue;
    for (const crate of readdirSync(parent, { withFileTypes: true })) {
      if (!crate.isDirectory()) continue;
      const crateDir = join(parent, crate.name);
      if (isFile(join(crateDir, 'Cargo.toml'))) candidates.push(join(crateDir, 'target'));
    }
  }

  return candidates.filter((dir) => {
    if (!isDirectory(dir)) return false;
    const tag = join(dir, 'CACHEDIR.TAG');
    if (isFile(tag) && readFileSync(tag, 'utf8').split(/\r?\n/, 1)[0] === cacheSignature) return true;
    console.warn(`Skipping ${relative(root, dir)}: no valid Cargo cache tag.`);
    return false;
  }).sort();
}

function ensureUntracked(root, paths) {
  const env = { ...process.env };
  // Inspect this checkout's index even if called with Git environment overrides.
  for (const name of ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_COMMON_DIR', 'GIT_INDEX_FILE']) delete env[name];
  const git = (...args) => execFileSync('git', args, { cwd: root, env, encoding: 'utf8' });
  const gitRoot = git('rev-parse', '--show-toplevel').replace(/\r?\n$/, '');
  if (realpathSync(gitRoot) !== root) throw new Error('Expected a Git checkout at the script’s repository root.');
  const tracked = git('--literal-pathspecs', 'ls-files', '-z', '--', ...paths).split('\0')[0];
  if (tracked) throw new Error(`Refusing to clean: a target contains a Git-tracked path: ${tracked}`);
}

function main(args) {
  const [option] = args;
  if (args.length > 1 || (args.length === 1 && !['--dry-run', '--help', '-h'].includes(option))) {
    throw new Error(usage);
  }
  if (option === '--help' || option === '-h') {
    console.log(usage);
    return;
  }

  const root = realpathSync(fileURLToPath(new URL('..', import.meta.url)));
  const targets = findTargets(root);
  if (targets.length === 0) {
    console.log('No Cargo build caches found.');
    return;
  }

  const paths = targets.map((dir) => relative(root, dir).split(sep).join('/'));
  ensureUntracked(root, paths);
  for (const [i, dir] of targets.entries()) {
    if (option === '--dry-run') {
      console.log(`Would remove: ${paths[i]}`);
    } else {
      rmSync(dir, { recursive: true, force: true });
      console.log(`Removed: ${paths[i]}`);
    }
  }
}

try {
  main(process.argv.slice(2));
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
