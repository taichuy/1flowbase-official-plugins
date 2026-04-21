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

test('provider-ci discovers package targets from repo scripts instead of a hardcoded provider list', () => {
  const workflow = readRepoFile('.github/workflows/provider-ci.yml');

  assert.match(
    workflow,
    /provider_matrix_json="\$\(node scripts\/list-provider-package-targets\.mjs --format github-matrix\)"/
  );
  assert.doesNotMatch(
    workflow,
    /matrix:\s*\n\s*provider:\s*\n\s*-\s*openai_compatible/
  );
});

test('provider workflows resolve runtime binary names from manifest metadata', () => {
  const ciWorkflow = readRepoFile('.github/workflows/provider-ci.yml');
  const releaseWorkflow = readRepoFile('.github/workflows/provider-release.yml');

  assert.match(
    ciWorkflow,
    /binary_name="\$\(node scripts\/list-provider-package-targets\.mjs --plugin-dir "\$\{plugin_dir\}" --field binary_name\)"/
  );
  assert.match(
    releaseWorkflow,
    /runtime_binary_name="\$\(node scripts\/list-provider-package-targets\.mjs --plugin-dir "\$\{PLUGIN_DIR\}" --rust-target "\$\{\{ matrix\.rust_target \}\}" --field runtime_binary_name\)"/
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

test('provider-release triggers only on runtime-extensions model provider manifests', () => {
  const workflow = readRepoFile('.github/workflows/provider-release.yml');

  assert.match(
    workflow,
    /paths:\n\s+- 'runtime-extensions\/model-providers\/\*\*\/manifest\.yaml'/
  );
  assert.doesNotMatch(workflow, /paths:\n\s+- 'models\/\*\*\/manifest\.yaml'/);
});

test('provider-release supports manual repair dispatches', () => {
  const workflow = readRepoFile('.github/workflows/provider-release.yml');

  assert.match(workflow, /workflow_dispatch:/);
  assert.match(workflow, /allow_existing_tag_repair:/);
  assert.match(workflow, /provider_code:/);
});

test('provider-release can reuse an existing tag during repair runs', () => {
  const workflow = readRepoFile('.github/workflows/provider-release.yml');

  assert.match(workflow, /ALLOW_EXISTING_TAG_REPAIR:/);
  assert.match(
    workflow,
    /if \[ "\$\{ALLOW_EXISTING_TAG_REPAIR\}" = "true" \]; then/
  );
  assert.match(
    workflow,
    /echo "Release tag \$\{TAG_NAME\} already exists on \$\{remote_tag_sha\}; continuing because allow_existing_tag_repair=true\."/
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
  const cargoToml = readRepoFile('runtime-extensions/model-providers/openai_compatible/Cargo.toml');
  const readme = readRepoFile('README.md');

  assert.match(
    cargoToml,
    /# Cargo requires a package version, but plugin release version is sourced from manifest\.yaml\./
  );
  assert.match(cargoToml, /^version\s*=\s*"0\.0\.0"$/m);
  assert.match(
    readme,
    /`runtime-extensions\/model-providers\/<provider_code>\/manifest\.yaml` 中的 `version:`/
  );
});
