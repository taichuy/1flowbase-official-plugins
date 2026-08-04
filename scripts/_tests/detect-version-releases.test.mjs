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

test('detectVersionReleases ignores manifest changes when version is unchanged', () => {
  const releases = detectVersionReleases([
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
