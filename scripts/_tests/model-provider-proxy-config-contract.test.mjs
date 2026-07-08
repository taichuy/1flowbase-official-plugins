import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');
const providersRoot = path.join(repoRoot, 'runtime-extensions', 'model-providers');

function toRepoPath(filePath) {
  return path.relative(repoRoot, filePath).split(path.sep).join('/');
}

function listProviderDirs() {
  return fs
    .readdirSync(providersRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => path.join(providersRoot, entry.name))
    .filter((providerDir) => fs.existsSync(path.join(providerDir, 'manifest.yaml')))
    .sort();
}

function providerYamlPath(providerDir) {
  const providerCode = path.basename(providerDir);
  return path.join(providerDir, 'provider', `${providerCode}.yaml`);
}

test('AC-001 official model providers expose advanced secret proxy_url config', () => {
  const failures = [];

  for (const providerDir of listProviderDirs()) {
    const yamlPath = providerYamlPath(providerDir);
    const provider = fs.readFileSync(yamlPath, 'utf8');
    const match = provider.match(/^- key: proxy_url\r?\n[\s\S]*?(?=\n- key:|\nmodels:|\n$)/m);

    if (!match) {
      failures.push(`${toRepoPath(yamlPath)} is missing config_schema key proxy_url`);
      continue;
    }

    assert.match(match[0], /^  type: secret$/m, `${toRepoPath(yamlPath)} proxy_url must be secret`);
    assert.match(match[0], /^  required: false$/m, `${toRepoPath(yamlPath)} proxy_url must be optional`);
    assert.match(match[0], /^  advanced: true$/m, `${toRepoPath(yamlPath)} proxy_url must be advanced`);
  }

  assert.deepEqual(failures, []);
});
