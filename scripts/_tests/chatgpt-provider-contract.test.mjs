import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');
const providerRoot = path.join(repoRoot, 'runtime-extensions/@taichuy/chatgpt');

function read(relativePath) {
  return fs.readFileSync(path.join(providerRoot, relativePath), 'utf8');
}

test('ChatGPT Subscription package exposes only the provider-owned OAuth contract', () => {
  const manifest = read('manifest.yaml');
  const provider = read('provider/chatgpt.yaml');

  assert.match(manifest, /^plugin_id: chatgpt$/m);
  assert.match(manifest, /^display_name: ChatGPT Subscription$/m);
  assert.match(manifest, /^  entry: bin\/chatgpt-provider$/m);
  assert.match(provider, /^provider_code: chatgpt$/m);
  assert.match(provider, /^default_base_url: https:\/\/chatgpt\.com\/backend-api\/codex$/m);
  assert.match(provider, /^model_discovery: dynamic$/m);
  assert.equal(fs.existsSync(path.join(providerRoot, 'models')), false);

  for (const action of ['device_code', 'pkce_callback']) {
    assert.match(provider, new RegExp(`- code: ${action}$`, 'm'));
  }
  for (const secret of [
    'access_token',
    'refresh_token',
    'id_token',
    'device_auth_id',
    'pkce_code_verifier',
    'instance_cookie_key'
  ]) {
    assert.match(provider, new RegExp(`^    - ${secret}$`, 'm'));
  }
});

test('ChatGPT provider retains generic parameter projection and no static model alias', () => {
  const provider = read('provider/chatgpt.yaml');

  assert.match(provider, /^  - key: use_responses_websocket$/m);
  assert.match(provider, /^      value: responses_websocket$/m);
  assert.match(provider, /^  - key: proxy_url$/m);
  assert.match(provider, /^    type: secret$/m);
  assert.doesNotMatch(provider, /^models:/m);
  assert.doesNotMatch(provider, /alpha\/search/);
});

test('ChatGPT plugin ships localized auth and transport schema labels', () => {
  for (const locale of ['en_US', 'zh_Hans']) {
    const catalog = JSON.parse(read(`i18n/${locale}.json`));
    assert.equal(typeof catalog.auth.actions.device_code.label, 'string');
    assert.equal(typeof catalog.auth.actions.pkce_callback.label, 'string');
    assert.equal(typeof catalog.parameters.use_responses_websocket.label, 'string');
    assert.equal(typeof catalog.fields.transport_mode.options.http_sse.label, 'string');
  }
});
