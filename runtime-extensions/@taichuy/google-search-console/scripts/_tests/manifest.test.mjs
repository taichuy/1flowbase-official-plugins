import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { generate, pluginRoot } from '../generate.mjs';

const sha256 = (filePath) =>
  `sha256:${createHash('sha256').update(fs.readFileSync(filePath)).digest('hex')}`;

function hasRef(node) {
  if (!node || typeof node !== 'object') return false;
  if (Array.isArray(node)) return node.some(hasRef);
  if ('$ref' in node || '$dynamicRef' in node) return true;
  return Object.values(node).some(hasRef);
}

test('generated manifest, MCP bundle, and ui/settings.tsx are deterministic and valid', () => {
  const summary = generate(pluginRoot, { check: true });
  assert.equal(summary.operations, 22);
  assert.equal(summary.mcp_tools, 15);

  const manifestText = fs.readFileSync(path.join(pluginRoot, 'manifest.yaml'), 'utf8');
  assert.match(manifestText, /^plugin_id:\s*google-search-console$/m);
  assert.match(manifestText, /^minimum_host_version:\s*0\.6\.6$/m);

  const bundlePath = path.join(pluginRoot, '_assets/mcp/manifest.json');
  const bundle = JSON.parse(fs.readFileSync(bundlePath, 'utf8'));
  assert.equal(bundle.bundle_id, 'google-search-console');
  assert.equal(bundle.organization, 'taichuy');

  const toolIds = new Set();
  let instanceCount = 0;
  for (const entry of bundle.files) {
    const abs = path.join(pluginRoot, '_assets/mcp', entry.path);
    assert.equal(sha256(abs), entry.sha256, `digest mismatch for ${entry.path}`);
    const parsed = JSON.parse(fs.readFileSync(abs, 'utf8'));
    if (entry.kind === 'instance') {
      instanceCount += 1;
      assert.equal(parsed.instance_id, 'google_search_console');
      assert.equal(parsed.default_entry_path, '/gsc');
      assert.equal(parsed.bindings.length, 15);
      assert.equal(typeof parsed.discovery_policy?.list_default_limit, 'number');
    } else if (entry.kind === 'tool') {
      assert.ok(parsed.tool_id.startsWith('plugin.google-search-console.'));
      assert.ok(!hasRef(parsed.parameter_schema_snapshot));
      assert.ok(!hasRef(parsed.result_schema_snapshot));
      toolIds.add(parsed.tool_id);
    }
  }
  assert.equal(instanceCount, 1);
  assert.equal(toolIds.size, 15);

  const settingsTsx = fs.readFileSync(path.join(pluginRoot, 'ui/settings.tsx'), 'utf8');
  assert.ok(!settingsTsx.includes("from './api'"));
  assert.ok(!settingsTsx.includes("from './copy'"));
  assert.ok(!settingsTsx.includes('Modal.confirm'));
  assert.ok(!settingsTsx.includes('notification.'));
  assert.ok(settingsTsx.includes('export default function GoogleSearchConsoleSettingsPage'));
});

