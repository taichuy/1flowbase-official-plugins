import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');
const providerRoot = path.join(repoRoot, 'runtime-extensions/@taichuy/chatgpt-codex');

function read(relativePath) {
  return fs.readFileSync(path.join(providerRoot, relativePath), 'utf8');
}

test('ChatGPT Codex package exposes only the provider-owned OAuth contract', () => {
  const manifest = read('manifest.yaml');
  const provider = read('provider/chatgpt-codex.yaml');

  assert.match(manifest, /^plugin_id: chatgpt-codex$/m);
  assert.match(manifest, /^display_name: ChatGPT Codex$/m);
  assert.match(manifest, /^  entry: bin\/chatgpt-codex-provider$/m);
  assert.match(manifest, /^    - usage\.rate_limit_windows$/m);
  assert.match(manifest, /^    - reset_credits$/m);
  assert.match(provider, /^provider_code: chatgpt-codex$/m);
  assert.match(provider, /^default_base_url: https:\/\/chatgpt\.com\/backend-api\/codex$/m);
  assert.match(provider, /^model_discovery: dynamic$/m);
  const staticModelDirectory = path.join(providerRoot, 'models', 'llm');
  assert.equal(
    fs.existsSync(staticModelDirectory)
      ? fs.readdirSync(staticModelDirectory).some((entry) =>
          entry.endsWith('.yaml')
        )
      : false,
    false
  );

  assert.match(provider, /- code: pkce_callback$/m);
  assert.doesNotMatch(provider, /device_code/);
  for (const secret of [
    'access_token',
    'refresh_token',
    'id_token',
    'pkce_code_verifier',
    'pkce_expires_at',
    'instance_cookie_key'
  ]) {
    assert.match(provider, new RegExp(`^    - ${secret}$`, 'm'));
  }
});

test('ChatGPT provider retains generic parameter projection and no static model alias', () => {
  const provider = read('provider/chatgpt-codex.yaml');

  assert.match(provider, /^    - key: use_responses_websocket$/m);
  assert.match(provider, /^        value: responses_websocket$/m);
  assert.match(provider, /^  - key: proxy_url$/m);
  assert.match(provider, /^    type: secret$/m);
  assert.doesNotMatch(provider, /^models:/m);
  assert.doesNotMatch(provider, /alpha\/search/);
});

test('ChatGPT plugin ships localized auth and transport schema labels', () => {
  for (const locale of ['en_US', 'zh_Hans']) {
    const catalog = JSON.parse(read(`i18n/${locale}.json`));
    assert.equal(typeof catalog.auth.actions.pkce_callback.label, 'string');
    assert.equal(typeof catalog.parameters.use_responses_websocket.label, 'string');
    assert.equal(typeof catalog.fields.transport_mode.options.http_sse.label, 'string');
  }
});
