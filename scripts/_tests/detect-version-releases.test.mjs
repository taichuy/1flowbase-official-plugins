import test from 'node:test';
import assert from 'node:assert/strict';

import { detectVersionReleases } from '../detect-version-releases.mjs';

test('detectVersionReleases returns release metadata when a provider version changes', () => {
  const releases = detectVersionReleases([
    {
      path: 'runtime-extensions/model-providers/openai_compatible/manifest.yaml',
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
      plugin_dir: 'runtime-extensions/model-providers/openai_compatible',
      provider_code: 'openai_compatible',
      release_tag: 'openai_compatible-v0.2.0',
      version: '0.2.0',
    },
  ]);
});

test('detectVersionReleases ignores manifest changes when version is unchanged', () => {
  const releases = detectVersionReleases([
    {
      path: 'runtime-extensions/model-providers/openai_compatible/manifest.yaml',
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
      path: 'runtime-extensions/model-providers/new_provider/manifest.yaml',
      beforeContent: '',
      afterContent: `plugin_code: new_provider
display_name: New Provider
version: 1.0.0
`,
    },
  ]);

  assert.deepEqual(releases, [
    {
      plugin_dir: 'runtime-extensions/model-providers/new_provider',
      provider_code: 'new_provider',
      release_tag: 'new_provider-v1.0.0',
      version: '1.0.0',
    },
  ]);
});
