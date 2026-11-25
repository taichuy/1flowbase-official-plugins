import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';

function fixture(t) {
  const base = mkdtempSync(join(tmpdir(), 'clean-rust-targets-'));
  t.after(() => rmSync(base, { recursive: true, force: true }));
  const root = join(base, 'checkout');
  const script = join(root, 'scripts', 'clean-rust-targets.mjs');
  mkdirSync(dirname(script), { recursive: true });
  copyFileSync(new URL('../clean-rust-targets.mjs', import.meta.url), script);
  execFileSync('git', ['init', '-q', root]);

  function write(relativePath, text = '') {
    const file = join(root, relativePath);
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, text);
    return file;
  }

  function cache(relativePath) {
    write(`${relativePath}/CACHEDIR.TAG`, 'Signature: 8a477f597d28d172789f06886806bc55\n');
    write(`${relativePath}/debug/artifact`);
    return join(root, relativePath);
  }

  function run(...args) {
    const result = spawnSync(process.execPath, [script, ...args], {
      cwd: base,
      env: { ...process.env, CARGO_TARGET_DIR: join(base, 'shared-target') },
      encoding: 'utf8',
    });
    assert.ifError(result.error);
    return result;
  }

  return { root, write, cache, run };
}

test('dry-run is read-only; default clears repository Rust targets only and is repeatable', (t) => {
  const { root, write, cache, run } = fixture(t);
  const manifest = write('runtime-extensions/@taichuy/provider/Cargo.toml');
  write('sdk/example/Cargo.toml');
  const targets = ['target', 'runtime-extensions/@taichuy/provider/target', 'sdk/example/target'];
  const dirs = targets.map(cache);

  write('sdk/untagged/Cargo.toml');
  const untagged = write('sdk/untagged/target/file');
  const notACrate = cache('sdk/not-a-crate/target');
  const sharedCache = cache('../shared-target');
  const dist = write('runtime-extensions/@taichuy/provider/dist/package');
  const tmp = write('tmp/keep');
  symlinkSync(sharedCache, join(root, 'sdk', 'linked-crate'), 'junction');
  write('sdk/linked-target/Cargo.toml');
  symlinkSync(sharedCache, join(root, 'sdk', 'linked-target', 'target'), 'junction');
  symlinkSync(sharedCache, join(root, 'target', 'linked-cache'), 'junction');

  const preview = run('--dry-run');
  assert.equal(preview.status, 0, preview.stderr);
  for (const [i, path] of targets.entries()) {
    assert.ok(preview.stdout.includes(`Would remove: ${path}\n`));
    assert.ok(existsSync(dirs[i]));
  }

  const cleanup = run();
  assert.equal(cleanup.status, 0, cleanup.stderr);
  for (const dir of dirs) assert.ok(!existsSync(dir), `not removed: ${dir}`);
  for (const path of [manifest, untagged, notACrate, sharedCache, dist, tmp]) {
    assert.ok(existsSync(path), `unexpectedly removed: ${path}`);
  }
  const repeat = run();
  assert.equal(repeat.status, 0, repeat.stderr);
  assert.match(repeat.stdout, /No Cargo build caches found/);
});

test('refuses to remove any targets when a candidate contains a Git-tracked file', (t) => {
  const { root, write, cache, run } = fixture(t);
  const rootTarget = cache('target');
  write('sdk/example/Cargo.toml');
  const crateTarget = cache('sdk/example/target');
  execFileSync('git', ['-C', root, 'add', '-f', '--', 'sdk/example/target/debug/artifact']);

  const cleanup = run();
  assert.equal(cleanup.status, 1);
  assert.match(cleanup.stderr, /Git-tracked.*sdk\/example\/target\/debug\/artifact/);
  assert.ok(existsSync(rootTarget));
  assert.ok(existsSync(crateTarget));
});

test('help and an unknown flag never remove targets', (t) => {
  const { cache, run } = fixture(t);
  const target = cache('target');
  const help = run('--help');
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /--dry-run/);
  const unknown = run('--dryrun');
  assert.equal(unknown.status, 1);
  assert.ok(existsSync(target));
});
