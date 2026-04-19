import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');

function readRepoFile(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), 'utf8');
}

function readVersionFromYaml(content) {
  const match = content.match(/^version:\s*(.+)$/m);
  return match ? match[1].trim() : null;
}

function readVersionFromCargoToml(content) {
  const packageSection = content.match(/\[package\][\s\S]*?(?=\n\[|$)/);
  if (!packageSection) {
    return null;
  }

  const match = packageSection[0].match(/^version\s*=\s*"(.+)"$/m);
  return match ? match[1] : null;
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
  assert.match(workflow, /OFFICIAL_PLUGIN_SIGNING_KEY_PEM: \$\{\{ secrets\.OFFICIAL_PLUGIN_SIGNING_KEY_PEM \}\}/);
  assert.match(workflow, /OFFICIAL_PLUGIN_SIGNING_KEY_ID: \$\{\{ secrets\.OFFICIAL_PLUGIN_SIGNING_KEY_ID \}\}/);

  assert.ok(
    workflow.indexOf('- name: Validate signing configuration') <
      workflow.indexOf('- name: Ensure release tag points at current commit')
  );
});

test('openai_compatible Cargo package version matches manifest version', () => {
  const manifestVersion = readVersionFromYaml(readRepoFile('models/openai_compatible/manifest.yaml'));
  const cargoVersion = readVersionFromCargoToml(readRepoFile('models/openai_compatible/Cargo.toml'));

  assert.equal(cargoVersion, manifestVersion);
});
