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
  if (!fs.existsSync(providersRoot)) {
    return [];
  }

  return fs
    .readdirSync(providersRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => path.join(providersRoot, entry.name))
    .filter((providerDir) => fs.existsSync(path.join(providerDir, 'manifest.yaml')))
    .sort();
}

function listModelFiles(modelsDir) {
  if (!fs.existsSync(modelsDir)) {
    return [];
  }

  return fs
    .readdirSync(modelsDir, { withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => entry.name)
    .filter((fileName) => fileName !== '_position.yaml')
    .filter((fileName) => /\.ya?ml$/.test(fileName))
    .sort()
    .map((fileName) => path.join(modelsDir, fileName));
}

function parsePositionItems(positionYaml) {
  return [...positionYaml.matchAll(/^\s*-\s*([A-Za-z0-9._:-]+)\s*$/gm)].map(
    (match) => match[1]
  );
}

function hasTopLevelField(yaml, fieldName) {
  return new RegExp(`^${fieldName}:\\s*\\S.*$`, 'm').test(yaml);
}

test('model provider static LLM descriptors use host loader input fields', () => {
  const failures = [];

  for (const providerDir of listProviderDirs()) {
    const modelsDir = path.join(providerDir, 'models', 'llm');
    const modelFiles = listModelFiles(modelsDir);
    const modelFileNames = new Set(modelFiles.map((filePath) => path.basename(filePath)));
    const positionPath = path.join(modelsDir, '_position.yaml');

    if (fs.existsSync(positionPath)) {
      const position = fs.readFileSync(positionPath, 'utf8');

      for (const modelId of parsePositionItems(position)) {
        if (
          !modelFileNames.has(`${modelId}.yaml`) &&
          !modelFileNames.has(`${modelId}.yml`)
        ) {
          failures.push(
            `${toRepoPath(positionPath)} lists ${modelId}, but no matching model file exists`
          );
        }
      }
    }

    for (const modelPath of modelFiles) {
      const model = fs.readFileSync(modelPath, 'utf8');

      if (!hasTopLevelField(model, 'model')) {
        failures.push(`${toRepoPath(modelPath)} is missing top-level model`);
      }
      if (!hasTopLevelField(model, 'label')) {
        failures.push(`${toRepoPath(modelPath)} is missing top-level label`);
      }
      if (/^display_name:\s*\S.*$/m.test(model)) {
        failures.push(`${toRepoPath(modelPath)} must use label, not top-level display_name`);
      }
    }
  }

  assert.deepEqual(failures, []);
});
