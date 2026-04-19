import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const provider = require('../openai_compatible.js');

function jsonResponse(payload, status = 200, statusText = 'OK') {
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText,
    async text() {
      return JSON.stringify(payload);
    }
  };
}

function createProviderConfig() {
  return {
    base_url: 'https://api.example.com/v1',
    api_key: 'test-key'
  };
}

test('listModels returns parameter_form for discovered chat models', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () =>
    jsonResponse({
      data: [
        {
          id: 'gpt-4.1-mini',
          owned_by: 'openai'
        }
      ]
    });

  try {
    const models = await provider.listModels(createProviderConfig());

    assert.equal(models.length, 1);
    assert.equal(models[0].model_id, 'gpt-4.1-mini');
    assert.equal(models[0].parameter_form?.schema_version, '1.0.0');
    assert.deepEqual(
      models[0].parameter_form?.fields.map((field) => field.key),
      ['temperature', 'top_p', 'max_tokens', 'seed']
    );
    assert.equal(models[0].parameter_form?.fields[0].enabled_by_default, true);
    assert.equal(models[0].parameter_form?.fields[1].enabled_by_default, false);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('invoke forwards model_parameters and response_format to chat completions', async () => {
  const originalFetch = globalThis.fetch;
  let capturedBody = null;

  globalThis.fetch = async (_url, options) => {
    capturedBody = JSON.parse(options.body);
    return jsonResponse({
      id: 'chatcmpl_fixture',
      model: 'gpt-4.1-mini',
      created: 123,
      choices: [
        {
          finish_reason: 'stop',
          message: {
            content: 'ok'
          }
        }
      ],
      usage: {
        prompt_tokens: 12,
        completion_tokens: 8,
        total_tokens: 20
      }
    });
  };

  try {
    const output = await provider.invoke({
      provider_config: createProviderConfig(),
      model: 'gpt-4.1-mini',
      messages: [{ role: 'user', content: 'hello' }],
      model_parameters: {
        temperature: 0.7,
        top_p: 0.9,
        max_tokens: 256,
        seed: 42
      },
      response_format: {
        mode: 'json_schema',
        schema: {
          type: 'object'
        }
      }
    });

    assert.equal(capturedBody.model, 'gpt-4.1-mini');
    assert.equal(capturedBody.temperature, 0.7);
    assert.equal(capturedBody.top_p, 0.9);
    assert.equal(capturedBody.max_tokens, 256);
    assert.equal(capturedBody.seed, 42);
    assert.deepEqual(capturedBody.response_format, {
      mode: 'json_schema',
      schema: {
        type: 'object'
      }
    });
    assert.equal(output.result.final_content, 'ok');
    assert.equal(output.result.usage.total_tokens, 20);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
