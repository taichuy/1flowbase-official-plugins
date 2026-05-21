import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');
const providerRoot = path.join(repoRoot, 'runtime-extensions/model-providers/openai');

function read(relativePath) {
  return fs.readFileSync(path.join(providerRoot, relativePath), 'utf8');
}

function extractParameterKeys(provider) {
  const match = provider.match(/parameter_form:\n[\s\S]*?config_schema:/);
  assert.ok(match, 'provider yaml should include parameter_form before config_schema');
  return [...match[0].matchAll(/^  - key: ([a-z0-9_]+)$/gm)].map((entry) => entry[1]);
}

function assertModelMetadata(modelId, label) {
  const model = read(`models/llm/${modelId}.yaml`);

  assert.match(model, new RegExp(`^model: ${modelId}$`, 'm'));
  assert.match(model, new RegExp(`^label: ${label}$`, 'm'));
  assert.doesNotMatch(model, /^display_name:/m);
  assert.match(model, /^family: llm$/m);
  assert.match(model, /^  - stream$/m);
  assert.match(model, /^  - tool_call$/m);
  assert.match(model, /^  - structured_output$/m);
  assert.match(model, /^  - reasoning$/m);
  assert.match(model, /^  owned_by: openai$/m);
  assert.match(model, /^  api: responses$/m);
  assert.match(model, /^  pricing_source: dynamic$/m);
  assert.doesNotMatch(model, /price_snapshot|as_of|million_tokens|input_price|output_price|unit_price/);
}

test('openai provider declares responses identity and runtime entry', () => {
  const manifest = read('manifest.yaml');
  const provider = read('provider/openai.yaml');

  assert.match(manifest, /^plugin_id: openai$/m);
  assert.match(manifest, /^display_name: OpenAI$/m);
  assert.match(manifest, /^execution_mode: stateful_provider_worker$/m);
  assert.match(manifest, /^  protocol: stdio_json_worker$/m);
  assert.match(manifest, /^  entry: bin\/openai-provider$/m);
  assert.match(manifest, /^  - model_provider$/m);
  assert.match(manifest, /^trust_level: verified_official$/m);
  assert.match(manifest, /^consumption_kind: runtime_extension$/m);

  assert.match(provider, /^provider_code: openai$/m);
  assert.match(provider, /^protocol: openai_responses$/m);
  assert.match(provider, /^default_base_url: https:\/\/api\.openai\.com\/v1$/m);
  assert.doesNotMatch(provider, /openai_compatible|chat_completions|default_headers|api_version/);
});

test('openai provider exposes responses parameters in order', () => {
  const provider = read('provider/openai.yaml');

  assert.deepEqual(extractParameterKeys(provider), [
    'reasoning_effort',
    'temperature',
    'top_p',
    'max_output_tokens',
    'response_format',
    'tool_choice',
    'store',
  ]);

  for (const field of ['n', 'max_tokens', 'max_completion_tokens', 'presence_penalty', 'frequency_penalty', 'stop']) {
    assert.doesNotMatch(provider, new RegExp(`^\\s+- key: ${field}$`, 'm'));
  }
});

test('openai provider static models declare current responses defaults', () => {
  const position = read('models/llm/_position.yaml');

  assert.match(position, /^  - gpt-5\.2$/m);
  assert.match(position, /^  - gpt-5\.1$/m);
  assert.match(position, /^  - gpt-5-mini$/m);
  assertModelMetadata('gpt-5.2', 'GPT-5.2');
  assertModelMetadata('gpt-5.1', 'GPT-5.1');
  assertModelMetadata('gpt-5-mini', 'GPT-5 mini');
});
