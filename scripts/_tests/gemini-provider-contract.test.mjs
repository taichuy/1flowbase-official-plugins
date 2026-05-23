import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');
const providerRoot = path.join(repoRoot, 'runtime-extensions/model-providers/gemini');

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

  assert.match(model, new RegExp(`^model: ${modelId.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`, 'm'));
  assert.match(model, new RegExp(`^label: ${label.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`, 'm'));
  assert.doesNotMatch(model, /^display_name:/m);
  assert.match(model, /^family: llm$/m);
  assert.match(model, /^  - stream$/m);
  assert.match(model, /^  - tool_call$/m);
  assert.match(model, /^  - structured_output$/m);
  assert.match(model, /^  owned_by: google$/m);
  assert.match(model, /^  pricing_source: dynamic$/m);
}

test('gemini provider declares native v1beta identity and defaults', () => {
  const manifest = read('manifest.yaml');
  const provider = read('provider/gemini.yaml');

  assert.match(manifest, /^plugin_id: gemini$/m);
  assert.match(manifest, /^  entry: bin\/gemini-provider$/m);
  assert.match(manifest, /^  - model_provider$/m);
  assert.match(manifest, /^trust_level: verified_official$/m);
  assert.match(provider, /^provider_code: gemini$/m);
  assert.match(provider, /^protocol: gemini$/m);
  assert.match(provider, /^default_base_url: https:\/\/generativelanguage\.googleapis\.com$/m);
});

test('gemini provider exposes native generation parameters in order', () => {
  const provider = read('provider/gemini.yaml');

  assert.deepEqual(extractParameterKeys(provider), [
    'temperature',
    'top_p',
    'top_k',
    'max_tokens',
    'stop',
    'response_format',
    'thinking_budget',
    'include_thoughts',
    'tool_choice',
  ]);

  assert.doesNotMatch(provider, /^  - key: presence_penalty$/m);
  assert.doesNotMatch(provider, /^  - key: frequency_penalty$/m);
  assert.doesNotMatch(provider, /^  - key: logprobs$/m);
});

test('gemini provider config matches AI Studio and sub2api-compatible auth', () => {
  const provider = read('provider/gemini.yaml');

  assert.match(
    provider,
    /^- key: base_url\n  type: string\n  required: true\n  default: https:\/\/generativelanguage\.googleapis\.com$/m
  );
  assert.match(provider, /^- key: api_key\n  type: secret\n  required: true$/m);
  assert.doesNotMatch(provider, /^- key: auth_type$/m);
  assert.match(provider, /^- key: validate_model\n  type: boolean\n  required: false\n  advanced: true$/m);
});

test('gemini static models mirror the sub2api fallback catalog', () => {
  const position = read('models/llm/_position.yaml');
  const expected = new Map([
    ['gemini-2.0-flash', 'Gemini 2.0 Flash'],
    ['gemini-2.5-flash', 'Gemini 2.5 Flash'],
    ['gemini-2.5-flash-image', 'Gemini 2.5 Flash Image'],
    ['gemini-2.5-pro', 'Gemini 2.5 Pro'],
    ['gemini-3-flash-preview', 'Gemini 3 Flash Preview'],
    ['gemini-3-pro-preview', 'Gemini 3 Pro Preview'],
    ['gemini-3.1-pro-preview', 'Gemini 3.1 Pro Preview'],
    ['gemini-3.1-pro-preview-customtools', 'Gemini 3.1 Pro Preview Custom Tools'],
    ['gemini-3.1-flash-image', 'Gemini 3.1 Flash Image'],
  ]);

  for (const [modelId, label] of expected) {
    assert.match(position, new RegExp(`^  - ${modelId.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`, 'm'));
    assertModelMetadata(modelId, label);
  }
});
