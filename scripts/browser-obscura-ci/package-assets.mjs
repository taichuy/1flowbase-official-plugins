import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const pluginRoot = path.resolve(import.meta.dirname, '../../runtime-extensions/@taichuy/browser-obscura');
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
function files(directory, prefix = '') {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const relative = path.posix.join(prefix, entry.name);
    return entry.isDirectory() ? files(path.join(directory, entry.name), relative) : [relative];
  });
}

// Called only for our freshly built archive, before publication. Inspect actual
// archive bytes so source files alone cannot satisfy the distribution contract.
export function verifyPackageAssets(archive) {
  const extracted = fs.mkdtempSync(path.join(os.tmpdir(), 'browser-obscura-package-proof-'));
  try {
    execFileSync('tar', ['-xzf', path.resolve(archive), '-C', extracted]);
    const required = ['LICENSE', 'README.md', 'README.en.md',
      ...files(path.join(pluginRoot, '_assets'), '_assets')];
    for (const relative of required) {
      assert.ok(fs.existsSync(path.join(extracted, relative)), `Missing archive asset: ${relative}`);
    }
    for (const relative of required) {
      const packaged = path.join(extracted, relative);
      assert.equal(digest(fs.readFileSync(packaged)), digest(fs.readFileSync(path.join(pluginRoot, relative))),
        `Archive asset differs from source: ${relative}`);
    }
    const bundleRoot = path.join(extracted, '_assets/mcp');
    const manifest = JSON.parse(fs.readFileSync(path.join(bundleRoot, 'manifest.json'), 'utf8'));
    for (const entry of manifest.files) {
      assert.equal(`sha256:${digest(fs.readFileSync(path.join(bundleRoot, entry.path)))}`, entry.sha256,
        `MCP example checksum mismatch: ${entry.path}`);
    }
    return { verified_files: required.length, mcp_tools: manifest.files.filter(file => file.kind === 'tool').length };
  } finally {
    fs.rmSync(extracted, { recursive: true, force: true });
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  assert.ok(process.argv[2], 'Pass the freshly built .1flowbasepkg archive');
  process.stdout.write(`${JSON.stringify(verifyPackageAssets(process.argv[2]))}\n`);
}
