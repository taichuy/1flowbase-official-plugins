import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export function readSdkPin(content, requested = '') {
  const match = content.match(/^runtime-extension-sdk\s*=\s*\{[^\n}]*git\s*=\s*"https:\/\/github\.com\/taichuy\/1flowbase\.git"[^\n}]*rev\s*=\s*"([a-f0-9]{40})"[^\n}]*\}/m);
  assert.ok(match, 'SDK dependency must pin the exact official host git revision');
  if (requested) assert.equal(match[1], requested, 'Requested host SHA must equal the plugin SDK pin');
  return match[1];
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const root = path.resolve(import.meta.dirname, '../..');
  const manifest = fs.readFileSync(path.join(root, 'runtime-extensions/@taichuy/browser-obscura/Cargo.toml'), 'utf8');
  process.stdout.write(`${readSdkPin(manifest, process.env.REQUESTED_HOST_SHA)}\n`);
}
