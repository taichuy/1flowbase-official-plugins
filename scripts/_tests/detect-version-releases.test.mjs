import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

import { detectVersionReleases } from '../detect-version-releases.mjs';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');

const notificationManifest = (version) => `manifest_version: 2
plugin_id: wechat-chat-channels
version: ${version}
consumption_kind: runtime_extension
slot_codes: []
notification_types:
  - type_code: 1flowbase/wechat-chat-channels/private
managed:
  module: {}
`;

test('managed notification runtime releases without a model provider slot', () => {
  const releases = detectVersionReleases([{
    path: 'runtime-extensions/@taichuy/wechat-chat-channels/manifest.yaml',
    beforeContent: '',
    afterContent: notificationManifest('1.0.0'),
  }]);
  assert.deepEqual(releases, [{
    plugin_dir: 'runtime-extensions/@taichuy/wechat-chat-channels',
    provider_code: 'wechat-chat-channels',
    release_tag: 'wechat-chat-channels-v1.0.0',
    version: '1.0.0',
  }]);
});

test('managed notification implementation changes require a version bump', () => {
  assert.throws(() => detectVersionReleases([
    { path: 'runtime-extensions/@taichuy/wechat-chat-channels/src/lib.rs' },
    {
      path: 'runtime-extensions/@taichuy/wechat-chat-channels/manifest.yaml',
      beforeContent: notificationManifest('1.0.0'),
      afterContent: notificationManifest('1.0.0'),
    },
  ]), /provider_version_bump_required/);
});

test('empty managed notification declaration does not become a publishable provider', () => {
  const releases = detectVersionReleases([{
    path: 'runtime-extensions/@taichuy/wechat-chat-channels/manifest.yaml',
    beforeContent: '',
    afterContent: notificationManifest('1.0.0').replace(/notification_types:\n  - .+/, 'notification_types: []'),
  }]);
  assert.deepEqual(releases, []);
});

test('managed notification JSON manifest uses its real version and empty slots', () => {
  const manifest = JSON.stringify({
    manifest_version: 2, version: '1.0.0', consumption_kind: 'runtime_extension',
    slot_codes: [], notification_types: [{ type_code: '1flowbase/wechat-chat-channels/private' }],
    managed: { module: {} },
  });
  const change = { path: 'runtime-extensions/@taichuy/wechat-chat-channels/manifest.yaml', afterContent: manifest };
  assert.equal(detectVersionReleases([change])[0].version, '1.0.0');
  assert.throws(() => detectVersionReleases([{ ...change, beforeContent: manifest }]), /version_bump_required/);
  const empty = JSON.stringify({ ...JSON.parse(manifest), notification_types: [] });
  assert.deepEqual(detectVersionReleases([{ ...change, afterContent: empty }]), []);
});

test('publisher cutover releases keep an explicit publisher namespace', () => {
  const providerCodes = [
    'aliyun_bailian',
    'anthropic',
    'deepseek',
    'gemini',
    'openai',
    'openai_compatible',
  ];

  for (const providerCode of providerCodes) {
    const manifest = fs.readFileSync(
      path.join(
        repoRoot,
        'runtime-extensions',
        '@taichuy',
        providerCode,
        'manifest.yaml'
      ),
      'utf8'
    );
    assert.match(
      manifest,
      /^publisher_namespace:\s*1flowbase$/m,
      `${providerCode} must publish an explicit canonical identity`
    );
  }
});

test('detectVersionReleases returns release metadata when a provider version changes', () => {
  const releases = detectVersionReleases([
    {
      path: 'runtime-extensions/@taichuy/openai_compatible/manifest.yaml',
      beforeContent: `plugin_code: openai_compatible
display_name: OpenAI-Compatible API Provider
version: 0.1.0
`,
      afterContent: `plugin_code: openai_compatible
display_name: OpenAI-Compatible API Provider
version: 0.2.0
`,
    },
  ]);

  assert.deepEqual(releases, [
    {
      plugin_dir: 'runtime-extensions/@taichuy/openai_compatible',
      provider_code: 'openai_compatible',
      release_tag: 'openai_compatible-v0.2.0',
      version: '0.2.0',
    },
  ]);
});

test('detectVersionReleases rejects provider changes when version is unchanged', () => {
  assert.throws(
    () =>
      detectVersionReleases([
        {
          path: 'runtime-extensions/@taichuy/openai_compatible/src/main.rs',
        },
        {
          path: 'runtime-extensions/@taichuy/openai_compatible/manifest.yaml',
          beforeContent: `plugin_code: openai_compatible
display_name: OpenAI-Compatible API Provider
version: 0.1.0
`,
          afterContent: `plugin_code: openai_compatible
display_name: OpenAI-Compatible API Provider Updated
version: 0.1.0
`,
        },
      ]),
    /provider_version_bump_required: openai_compatible changed without updating manifest version 0\.1\.0/
  );
});

test('detectVersionReleases ignores documentation and test-only provider changes', () => {
  const releases = detectVersionReleases([
    {
      path: 'runtime-extensions/@taichuy/openai_compatible/readme/README_en_US.md',
    },
    {
      path: 'runtime-extensions/@taichuy/openai_compatible/tests/stdio_worker.rs',
    },
  ]);

  assert.deepEqual(releases, []);
});

test('detectVersionReleases releases provider distribution runtime extensions', () => {
  const releases = detectVersionReleases([
    {
      path: 'runtime-extensions/@taichuy/session-retry-distribution/src/main.rs',
    },
    {
      path: 'runtime-extensions/@taichuy/session-retry-distribution/manifest.yaml',
      beforeContent: `version: 0.9.0
slot_codes: [provider_distribution_rule]
`,
      afterContent: `version: 1.0.0
slot_codes: [provider_distribution_rule]
`,
    },
  ]);

  assert.deepEqual(releases, [
    {
      plugin_dir: 'runtime-extensions/@taichuy/session-retry-distribution',
      provider_code: 'session-retry-distribution',
      release_tag: 'session-retry-distribution-v1.0.0',
      version: '1.0.0',
    },
  ]);
});

test('detectVersionReleases requires a version bump for provider distribution implementation changes', () => {
  assert.throws(
    () => detectVersionReleases([
      {
        path: 'runtime-extensions/@taichuy/session-retry-distribution/src/main.rs',
      },
      {
        path: 'runtime-extensions/@taichuy/session-retry-distribution/manifest.yaml',
        beforeContent: `version: 1.0.0\nslot_codes: [provider_distribution_rule]\n`,
        afterContent: `version: 1.0.0\nslot_codes: [provider_distribution_rule]\n`,
      },
    ]),
    /provider_version_bump_required: session-retry-distribution changed without updating manifest version 1\.0\.0/
  );
});

test('detectVersionReleases ignores provider distribution non-package inputs', () => {
  const releases = detectVersionReleases([
    { path: 'runtime-extensions/@taichuy/session-retry-distribution/README.md' },
    { path: 'runtime-extensions/@taichuy/session-retry-distribution/tests/stdio.rs' },
    { path: 'runtime-extensions/@taichuy/session-retry-distribution/target/release/binary' },
  ]);
  assert.deepEqual(releases, []);
});

test('detectVersionReleases treats a newly added provider manifest as releasable', () => {
  const releases = detectVersionReleases([
    {
      path: 'runtime-extensions/@taichuy/new_provider/manifest.yaml',
      beforeContent: '',
      afterContent: `plugin_code: new_provider
display_name: New Provider
version: 1.0.0
`,
    },
  ]);

  assert.deepEqual(releases, [
    {
      plugin_dir: 'runtime-extensions/@taichuy/new_provider',
      provider_code: 'new_provider',
      release_tag: 'new_provider-v1.0.0',
      version: '1.0.0',
    },
  ]);
});

test('shared observation SDK requires a manifest bump for every consumer', () => {
  assert.throws(() => detectVersionReleases([{ path: 'sdk/provider-observation/src/lib.rs' }]), /provider_version_bump_required/);
  const consumers = ['openai', 'openai_compatible', 'anthropic', 'gemini', 'deepseek', 'aliyun_bailian', 'chatgpt-codex'];
  const changes = [{ path: 'sdk/provider-observation/src/lib.rs' }, ...consumers.map((provider) => ({
    path: `runtime-extensions/@taichuy/${provider}/manifest.yaml`, beforeContent: 'version: 1.0.0', afterContent: 'version: 1.0.1',
  }))];
  assert.equal(detectVersionReleases(changes).length, 7);
  changes[1].afterContent = changes[1].beforeContent;
  assert.throws(() => detectVersionReleases(changes), /provider_version_bump_required/);
});

const managedServiceManifest = version => `manifest_version: 2
version: ${version}
consumption_kind: runtime_extension
slot_codes: []
managed:
  module:
    contributions:
      - contribution_id: browser-obscura.browsers.list
        point_id: 1flowbase.managed-service.operation
  execution_bindings:
    - contribution_id: browser-obscura.browsers.list
      handler: browser-obscura.browsers.list
`;

test('managed-only service releases without provider slots or notification types', () => {
  const path = 'runtime-extensions/@taichuy/browser-obscura/manifest.yaml';
  const afterContent = managedServiceManifest('0.1.0');
  assert.deepEqual(detectVersionReleases([{ path, afterContent }]), [{
    plugin_dir: 'runtime-extensions/@taichuy/browser-obscura', provider_code: 'browser-obscura',
    release_tag: 'browser-obscura-v0.1.0', version: '0.1.0',
  }]);
  assert.throws(() => detectVersionReleases([{ path, beforeContent: afterContent, afterContent }]), /provider_version_bump_required/);
});
test('managed-only discovery rejects empty, unbound, unrelated, and non-runtime declarations', () => {
  const path = 'runtime-extensions/@taichuy/browser-obscura/manifest.yaml';
  const valid = managedServiceManifest('0.1.0');
  const invalid = [
    valid.replace(/managed:[\s\S]+/, 'managed:\n  module: {}\n'),
    valid.replace(/  execution_bindings:[\s\S]+/, '  execution_bindings: []\n'),
    valid.replace('point_id: 1flowbase.managed-service.operation', 'point_id: 1flowbase.plugin-data.owned-collection'),
    valid.replace('consumption_kind: runtime_extension', 'consumption_kind: template'),
    valid.replace('  execution_bindings:\n    - contribution_id: browser-obscura.browsers.list', '  execution_bindings:\n    - contribution_id: unrelated.operation'),
    valid.replace('managed:', 'other:'),
  ];
  for (const afterContent of invalid) assert.deepEqual(detectVersionReleases([{ path, afterContent }]), []);
});
test('JSON managed-only services require a matching operation execution binding', () => {
  const manifest = {
    manifest_version: 2, version: '0.1.0', consumption_kind: 'runtime_extension', slot_codes: [],
    managed: { module: { contributions: [{ contribution_id: 'browsers.list', point_id: '1flowbase.managed-service.operation' }] }, execution_bindings: [{ contribution_id: 'browsers.list' }] },
  };
  const path = 'runtime-extensions/@taichuy/browser-obscura/manifest.yaml';
  assert.equal(detectVersionReleases([{ path, afterContent: JSON.stringify(manifest) }]).length, 1);
  manifest.managed.execution_bindings[0].contribution_id = 'other';
  assert.deepEqual(detectVersionReleases([{ path, afterContent: JSON.stringify(manifest) }]), []);
});
