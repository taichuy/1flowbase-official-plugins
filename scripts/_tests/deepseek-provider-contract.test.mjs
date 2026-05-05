import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');
const providerRoot = path.join(repoRoot, 'runtime-extensions/model-providers/deepseek');

function read(relativePath) {
  return fs.readFileSync(path.join(providerRoot, relativePath), 'utf8');
}

function extractParameterKeys(provider) {
  const match = provider.match(/parameter_form:\n[\s\S]*?config_schema:/);
  assert.ok(match, 'provider yaml should include parameter_form before config_schema');
  return [...match[0].matchAll(/^  - key: ([a-z0-9_]+)$/gm)].map((entry) => entry[1]);
}

function assertModelMetadata(modelId) {
  const model = read(`models/llm/${modelId}.yaml`);

  assert.match(model, new RegExp(`^model: ${modelId}$`, 'm'));
  assert.match(model, /^label: DeepSeek V4 (Flash|Pro)$/m);
  assert.doesNotMatch(model, /^display_name:/m);
  assert.match(model, /^family: llm$/m);
  assert.match(model, /^  - stream$/m);
  assert.match(model, /^  - tool_call$/m);
  assert.match(model, /^  - structured_output$/m);
  assert.match(model, /^context_window: 1000000$/m);
  assert.match(model, /^max_output_tokens: 384000$/m);
  assert.match(model, /^  owned_by: deepseek$/m);
  assert.match(model, /^  reasoning: true$/m);
  assert.match(model, /^  pricing_source: dynamic$/m);
  assert.doesNotMatch(model, /price_snapshot|as_of|million_tokens|input_price|output_price|unit_price/);
}

test('deepseek provider declares dedicated identity and defaults', () => {
  const manifest = read('manifest.yaml');
  const provider = read('provider/deepseek.yaml');

  assert.match(manifest, /^plugin_id: deepseek$/m);
  assert.match(manifest, /^  entry: bin\/deepseek-provider$/m);
  assert.match(manifest, /^  - model_provider$/m);
  assert.match(manifest, /^trust_level: verified_official$/m);
  assert.match(manifest, /^consumption_kind: runtime_extension$/m);

  assert.match(provider, /^provider_code: deepseek$/m);
  assert.match(provider, /^default_base_url: https:\/\/api\.deepseek\.com$/m);
  assert.doesNotMatch(provider, /organization|project|api_version|default_headers/);
});

test('deepseek provider exposes only the requested deepseek parameter fields in order', () => {
  const provider = read('provider/deepseek.yaml');

  assert.deepEqual(extractParameterKeys(provider), [
    'thinking_type',
    'reasoning_effort',
    'temperature',
    'top_p',
    'max_tokens',
    'response_format',
    'stop',
    'tool_choice',
    'logprobs',
    'top_logprobs',
    'user_id',
  ]);

  assert.doesNotMatch(provider, /^  - key: frequency_penalty$/m);
  assert.doesNotMatch(provider, /^  - key: presence_penalty$/m);
  assert.doesNotMatch(provider, /pricing|price_snapshot|as_of|million_tokens/);
});

test('deepseek provider config keeps only api key base url and advanced validate model', () => {
  const provider = read('provider/deepseek.yaml');

  assert.match(provider, /^- key: base_url\n  type: string\n  required: true\n  default: https:\/\/api\.deepseek\.com$/m);
  assert.match(provider, /^- key: api_key\n  type: secret\n  required: true$/m);
  assert.match(provider, /^- key: validate_model\n  type: boolean\n  required: false\n  advanced: true$/m);
});

test('deepseek static models declare dynamic pricing metadata without static prices', () => {
  const position = read('models/llm/_position.yaml');

  assert.match(position, /^  - deepseek-v4-flash$/m);
  assert.match(position, /^  - deepseek-v4-pro$/m);
  assertModelMetadata('deepseek-v4-flash');
  assertModelMetadata('deepseek-v4-pro');
});
