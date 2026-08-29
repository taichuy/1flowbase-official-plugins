import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

import { detectVersionReleases } from '../detect-version-releases.mjs';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');

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
