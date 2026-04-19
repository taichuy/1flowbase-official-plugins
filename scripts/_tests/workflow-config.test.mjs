import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');

function readRepoFile(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), 'utf8');
}

test('provider-ci uses cross for musl dry-run builds', () => {
  const workflow = readRepoFile('.github/workflows/provider-ci.yml');

  assert.match(workflow, /- name: Install cross/);
  assert.match(
    workflow,
    /cross build --manifest-path "\$\{cargo_toml\}" --release --target x86_64-unknown-linux-musl/
  );

  assert.ok(
    workflow.indexOf('- name: Install cross') < workflow.indexOf('- name: Build provider binary for dry-run packaging')
  );
});

test('provider-release validates signing secrets before tagging releases', () => {
  const workflow = readRepoFile('.github/workflows/provider-release.yml');

  assert.match(workflow, /- name: Validate signing configuration/);
  assert.match(
    workflow,
    /OFFICIAL_PLUGIN_SIGNING_KEY_PEM: \$\{\{ secrets\.OFFICIAL_PLUGIN_SIGNING_KEY_PEM \|\| secrets\.OFFICIAL_PLUGIN_SIGNING_PRIVATE_KEY_PEM \}\}/
  );
  assert.match(workflow, /OFFICIAL_PLUGIN_SIGNING_KEY_ID: \$\{\{ secrets\.OFFICIAL_PLUGIN_SIGNING_KEY_ID \}\}/);

  assert.ok(
    workflow.indexOf('- name: Validate signing configuration') <
      workflow.indexOf('- name: Ensure release tag points at current commit')
  );
});

test('provider-release extracts package metadata from plugin CLI output instead of assuming JSON stdout', () => {
  const workflow = readRepoFile('.github/workflows/provider-release.yml');

  assert.match(workflow, /package_output="\$\(node host\/scripts\/node\/plugin\.js package/);
  assert.match(
    workflow,
    /package_file="\$\(printf '%s\\n' "\$\{package_output\}" \| sed -n 's\/\.\*Plugin package created at \/\/p' \| tail -n 1\)"/
  );
  assert.match(workflow, /checksum="\$\(sha256sum "\$\{package_file\}" \| awk '\{print \$1\}'\)"/);
  assert.match(workflow, /checksum: "sha256:" \+ process\.argv\[7\]/);
  assert.doesNotMatch(workflow, /checksum: `sha256:\$\{process\.argv\[7\]\}`/);
});

test('manifest.yaml is the single release version source for openai_compatible', () => {
  const cargoToml = readRepoFile('models/openai_compatible/Cargo.toml');
  const readme = readRepoFile('README.md');

  assert.match(
    cargoToml,
    /# Cargo requires a package version, but plugin release version is sourced from manifest\.yaml\./
  );
  assert.match(cargoToml, /^version\s*=\s*"0\.0\.0"$/m);
  assert.match(readme, /`manifest\.yaml` 是 provider 发布版本的唯一维护位置/);
});
