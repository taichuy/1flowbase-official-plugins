import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');
const providerRoot = path.join(repoRoot, 'runtime-extensions/model-providers/anthropic');

function read(relativePath) {
  return fs.readFileSync(path.join(providerRoot, relativePath), 'utf8');
}

function extractParameterKeys(provider) {
  const match = provider.match(/parameter_form:\n[\s\S]*?config_schema:/);
  assert.ok(match, 'provider yaml should include parameter_form before config_schema');
  return [...match[0].matchAll(/^  - key: ([a-z0-9_]+)$/gm)].map((entry) => entry[1]);
}

function extractParameter(provider, key) {
  const match = provider.match(
    new RegExp(`^  - key: ${key}\\n[\\s\\S]*?(?=^  - key: |^config_schema:)`, 'm')
  );
  assert.ok(match, `provider yaml should declare ${key}`);
  return match[0];
}

function assertModelMetadata(modelId, label, maxOutputTokens) {
  const model = read(`models/llm/${modelId}.yaml`);

  assert.match(model, new RegExp(`^model: ${modelId}$`, 'm'));
  assert.match(model, new RegExp(`^label: ${label}$`, 'm'));
  assert.doesNotMatch(model, /^display_name:/m);
  assert.match(model, /^family: llm$/m);
  assert.match(model, /^  - stream$/m);
  assert.match(model, /^  - tool_call$/m);
  assert.match(model, /^  - reasoning$/m);
  assert.match(model, /^context_window: 200000$/m);
  assert.match(model, new RegExp(`^max_output_tokens: ${maxOutputTokens}$`, 'm'));
  assert.match(model, /^  owned_by: anthropic$/m);
  assert.match(model, /^  api: messages$/m);
  assert.match(model, /^  pricing_source: dynamic$/m);
  assert.doesNotMatch(model, /price_snapshot|as_of|million_tokens|input_price|output_price|unit_price/);
}

test('anthropic provider declares messages identity and runtime entry', () => {
  const manifest = read('manifest.yaml');
  const provider = read('provider/anthropic.yaml');

  assert.match(manifest, /^plugin_id: anthropic$/m);
  assert.match(manifest, /^display_name: Anthropic$/m);
  assert.match(manifest, /^  entry: bin\/anthropic-provider$/m);
  assert.match(manifest, /^    invoke_timeout_ms: 300000$/m);
  assert.match(manifest, /^  - model_provider$/m);
  assert.match(manifest, /^trust_level: verified_official$/m);
  assert.match(manifest, /^consumption_kind: runtime_extension$/m);

  assert.match(provider, /^provider_code: anthropic$/m);
  assert.match(provider, /^protocol: anthropic_messages$/m);
  assert.match(provider, /^default_base_url: https:\/\/api\.anthropic\.com$/m);
  assert.match(provider, /^model_discovery: hybrid$/m);
  assert.match(provider, /^help_url: https:\/\/docs\.anthropic\.com\/en\/api\/messages$/m);
  assert.doesNotMatch(provider, /deepseek|openai_compatible|chat_completions/);
});

test('anthropic provider supports hybrid model discovery configuration', () => {
  const provider = read('provider/anthropic.yaml');

  assert.match(provider, /^supports_model_fetch_without_credentials: false$/m);
  assert.match(provider, /^- key: validate_model\n  type: boolean\n  required: false\n  advanced: true$/m);
  assert.match(provider, /^- key: anthropic_version\n  type: string\n  required: false\n  default: "2023-06-01"\n  advanced: true$/m);
});

test('anthropic provider exposes messages parameters in order', () => {
  const provider = read('provider/anthropic.yaml');

  assert.deepEqual(extractParameterKeys(provider), [
    'thinking_type',
    'thinking_budget_tokens',
    'temperature',
    'top_p',
    'top_k',
    'max_output_tokens',
    'tool_choice',
  ]);

  assert.doesNotMatch(provider, /^  - key: response_format$/m);
  assert.doesNotMatch(provider, /^  - key: reasoning_effort$/m);
});

test('AC-001 anthropic max_output_tokens is optional and disabled by default', () => {
  const provider = read('provider/anthropic.yaml');
  const maxOutputTokens = extractParameter(provider, 'max_output_tokens');

  assert.match(maxOutputTokens, /^    send_mode: optional$/m);
  assert.match(maxOutputTokens, /^    enabled_by_default: false$/m);
  assert.match(maxOutputTokens, /^    default_value: 4096$/m);
});

test('anthropic static models declare api model names', () => {
  const position = read('models/llm/_position.yaml');

  assert.match(position, /^  - claude-opus-4-1-20250805$/m);
  assert.match(position, /^  - claude-sonnet-4-20250514$/m);
  assert.match(position, /^  - claude-3-5-haiku-20241022$/m);
  assertModelMetadata('claude-opus-4-1-20250805', 'Claude Opus 4.1', 32000);
  assertModelMetadata('claude-sonnet-4-20250514', 'Claude Sonnet 4', 64000);
  assertModelMetadata('claude-3-5-haiku-20241022', 'Claude Haiku 3.5', 8192);
});
