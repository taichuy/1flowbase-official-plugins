import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');
const providerRoot = path.join(
  repoRoot,
  'runtime-extensions/model-providers/openai_compatible'
);

function readProviderFile(relativePath) {
  return fs.readFileSync(path.join(providerRoot, relativePath), 'utf8');
}

const nodeAdaptationFields = [
  'response_format',
  'user',
  'tools',
  'tool_choice',
  'parallel_tool_calls',
  'store',
  'metadata',
  'audio',
  'modalities',
];

test('openai compatible parameter form exposes only direct LLM request tuning fields', () => {
  const providerYaml = readProviderFile('provider/openai_compatible.yaml');

  assert.match(providerYaml, /^\s+- key: temperature$/m);
  assert.match(providerYaml, /^\s+- key: max_completion_tokens$/m);
  assert.match(providerYaml, /^\s+- key: reasoning_effort$/m);

  for (const field of nodeAdaptationFields) {
    assert.doesNotMatch(providerYaml, new RegExp(`^\\s+- key: ${field}$`, 'm'));
  }
});

test('openai compatible localized parameter bundles omit node adaptation fields', () => {
  for (const locale of ['en_US', 'zh_Hans']) {
    const bundle = JSON.parse(readProviderFile(`i18n/${locale}.json`));

    for (const field of nodeAdaptationFields) {
      assert.equal(bundle.parameters[field], undefined);
    }
  }
});
