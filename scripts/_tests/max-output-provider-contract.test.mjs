import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');
const providerIds = [
  'aliyun_bailian',
  'anthropic',
  'deepseek',
  'gemini',
  'openai_compatible',
];

function parameterForm(providerId) {
  const providerPath = path.join(
    repoRoot,
    'runtime-extensions/model-providers',
    providerId,
    'provider',
    `${providerId}.yaml`
  );
  const provider = fs.readFileSync(providerPath, 'utf8');
  const match = provider.match(/parameter_form:\r?\n[\s\S]*?\r?\nconfig_schema:/);
  assert.ok(match, `${providerId} should declare parameter_form before config_schema`);
  return match[0];
}

test('AC-001 official provider inventory uses the Native max_output_tokens key', () => {
  for (const providerId of providerIds) {
    const form = parameterForm(providerId);
    assert.match(form, /^  - key: max_output_tokens$/m, providerId);
    assert.doesNotMatch(form, /^  - key: max_tokens$/m, providerId);
  }
});
