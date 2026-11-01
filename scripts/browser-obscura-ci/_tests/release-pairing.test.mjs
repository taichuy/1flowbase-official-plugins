import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../../', import.meta.url));
const workflow = fs.readFileSync(path.join(root, '.github/workflows/provider-release.yml'), 'utf8');
const sha = 'a'.repeat(40);
const otherSha = 'b'.repeat(40);
const dependency = pin => `runtime-extension-sdk = { git = "https://github.com/taichuy/1flowbase.git", rev = "${pin}", features = ["calendar"] }\n`;
const selection = workflow.split('      - name: Select paired host source')[1]
  .split('        run: |\n')[1].split('\n      - name: Checkout host repo')[0];
const build = workflow.split('          # Keep the array nonempty:')[1]
  .split('\n          runtime_binary_name=')[0].split('\n').slice(1).join('\n');

function fixture(t, content = dependency(sha)) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'release-pairing-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  fs.writeFileSync(path.join(dir, 'Cargo.toml'), content);
  return dir;
}

function select(dir, provider, requested = '') {
  const output = path.join(dir, 'output');
  fs.writeFileSync(output, '');
  const result = spawnSync('bash', ['-c', selection], {
    cwd: root, encoding: 'utf8',
    env: { ...process.env, PLUGIN_DIR: dir, PROVIDER_CODE: provider, REQUESTED_HOST_SHA: requested, GITHUB_OUTPUT: output },
  });
  return { ...result, output: fs.readFileSync(output, 'utf8') };
}

test('calendar selects its fixture Cargo SDK SHA, with optional identical dispatch SHA', t => {
  const dir = fixture(t);
  for (const requested of ['', sha]) {
    const result = select(dir, 'taichuy-calendar', requested);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.output, `ref=${sha}\n`);
  }
});

test('calendar rejects dispatch mismatch, branches, abbreviated/unpinned SDK sources', t => {
  const dir = fixture(t);
  for (const requested of [otherSha, 'main']) {
    const result = select(dir, 'taichuy-calendar', requested);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /must equal the plugin SDK pin/);
    assert.equal(result.output, '');
  }
  for (const content of [dependency('main'), dependency('abcdef0'), dependency(`${sha}a`), 'runtime-extension-sdk = { path = "../sdk" }\n']) {
    fs.writeFileSync(path.join(dir, 'Cargo.toml'), content);
    const result = select(dir, 'taichuy-calendar');
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /exact official host git revision/);
    assert.equal(result.output, '');
  }
});

test('ordinary providers still default to main and accept an explicit host ref', t => {
  const dir = fixture(t, '');
  for (const requested of ['', 'release/host']) {
    const result = select(dir, 'openai', requested);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.output, `ref=${requested || 'main'}\n`);
  }
});

test('browser keeps its existing default manifest pin and mismatch rejection', t => {
  const dir = fixture(t, '');
  const existing = spawnSync(process.execPath, ['scripts/browser-obscura-ci/source.mjs'], {
    cwd: root, encoding: 'utf8', env: { ...process.env, REQUESTED_HOST_SHA: '' },
  });
  assert.equal(existing.status, 0, existing.stderr);
  const pin = existing.stdout.trim();
  for (const requested of ['', pin]) {
    const result = select(dir, 'browser-obscura', requested);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.output, `ref=${pin}\n`);
  }
  assert.notEqual(select(dir, 'browser-obscura', `${pin}0`).status, 0);
});

test('normal and musl calendar builds lock dependencies and stop before building on host mismatch', t => {
  const dir = fixture(t);
  const bin = path.join(dir, 'bin');
  fs.mkdirSync(bin);
  for (const name of ['cargo', 'cross']) {
    fs.writeFileSync(path.join(bin, name), `#!/bin/bash\nprintf '%s\\n' '${name}' "$@" > "$BUILD_LOG"\n`, { mode: 0o755 });
  }
  fs.writeFileSync(path.join(bin, 'git'), '#!/bin/bash\nprintf "%s\\n" "$ACTUAL_HOST_SHA"\n', { mode: 0o755 });
  const log = path.join(dir, 'build.log');
  for (const libc of ['gnu', 'musl']) {
    for (const actual of [sha, otherSha]) {
      fs.rmSync(log, { force: true });
      const script = build.replaceAll('${{ matrix.libc }}', libc).replaceAll('${{ matrix.rust_target }}', 'fixture-target');
      const result = spawnSync('bash', ['-c', `set -euo pipefail\n${script}`], {
        cwd: root, encoding: 'utf8',
        env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, PROVIDER_CODE: 'taichuy-calendar', PLUGIN_DIR: dir, ACTUAL_HOST_SHA: actual, BUILD_LOG: log },
      });
      if (actual !== sha) {
        assert.notEqual(result.status, 0);
        assert.equal(fs.existsSync(log), false);
      } else {
        assert.equal(result.status, 0, result.stderr);
        assert.deepEqual(fs.readFileSync(log, 'utf8').trim().split('\n'), [libc === 'musl' ? 'cross' : 'cargo', 'build', '--manifest-path', `${dir}/Cargo.toml`, '--release', '--target', 'fixture-target', '--locked']);
      }
    }
  }
});

test('ordinary build arguments stay unlocked and browser keeps its locked native build', t => {
  const dir = fixture(t);
  const bin = path.join(dir, 'bin');
  fs.mkdirSync(bin);
  for (const name of ['cargo', 'cross']) {
    fs.writeFileSync(path.join(bin, name), `#!/bin/bash\nprintf '%s\\n' '${name}' "$@" > "$BUILD_LOG"\n`, { mode: 0o755 });
  }
  fs.writeFileSync(path.join(bin, 'git'), '#!/bin/bash\nprintf "%s\\n" "$ACTUAL_HOST_SHA"\n', { mode: 0o755 });
  const log = path.join(dir, 'build.log');
  for (const [provider, libc] of [['openai', 'gnu'], ['openai', 'musl'], ['browser-obscura', 'gnu']]) {
    const script = build.replaceAll('${{ matrix.libc }}', libc).replaceAll('${{ matrix.rust_target }}', 'fixture-target');
    const result = spawnSync('bash', ['-c', `set -euo pipefail\n${script}`], {
      cwd: root, encoding: 'utf8',
      env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, PROVIDER_CODE: provider, PLUGIN_DIR: dir, ACTUAL_HOST_SHA: provider === 'browser-obscura' ? sha : otherSha, BUILD_LOG: log },
    });
    assert.equal(result.status, 0, result.stderr);
    const expected = [libc === 'musl' ? 'cross' : 'cargo', 'build', '--manifest-path', `${dir}/Cargo.toml`, '--release', '--target', 'fixture-target'];
    if (provider === 'browser-obscura') expected.push('--locked');
    assert.deepEqual(fs.readFileSync(log, 'utf8').trim().split('\n'), expected);
  }
});
