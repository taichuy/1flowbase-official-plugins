import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';

const descriptorPath = '_assets/runtime/descriptor.json';
const checker = async root => (await import(pathToFileURL(path.resolve(root, 'scripts/check-package.mjs')).href)).checkPackage;
const descriptor = root => fs.readFileSync(path.join(root, descriptorPath));
const sameDescriptor = (root, expected) => assert.ok(descriptor(root).equals(expected),
  'Runtime descriptor bytes differ from pre-cargo include_str! input; do not rebuild/re-pin after compilation');

// Save the exact include_str! input outside the plugin, never a parsed/rehashed copy.
export async function snapshotDescriptor(pluginRoot, snapshotFile) {
  const expected = descriptor(pluginRoot);
  await (await checker(pluginRoot))(pluginRoot, null);
  sameDescriptor(pluginRoot, expected);
  const parent = fs.realpathSync(path.dirname(path.resolve(snapshotFile)));
  const destination = path.join(parent, path.basename(snapshotFile));
  const relative = path.relative(fs.realpathSync(pluginRoot), destination);
  assert.ok(relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative),
    'The pre-cargo descriptor snapshot must be outside the plugin');
  fs.writeFileSync(destination, expected, { flag: 'wx', mode: 0o600 });
  return { status: 'pre_cargo_assets_verified', snapshot: destination };
}

// Only for our newly signed package, before upload. Source contracts stay at
// pluginRoot; the existing checker reads actual assets/manifest/binary from tar.
export async function verifyPackageAssets(archive, pluginRoot, snapshotFile) {
  const expected = fs.readFileSync(snapshotFile);
  sameDescriptor(pluginRoot, expected);
  const extracted = fs.mkdtempSync(path.join(process.env.RUNNER_TEMP || os.tmpdir(), 'calendar-runtime-package-'));
  try {
    const fd = fs.openSync(path.resolve(archive), 'r');
    try {
      // Streaming stdin + cwd avoids tar interpreting Windows drive letters or
      // Git Bash path syntax. Node alone resolves native filesystem paths.
      execFileSync('tar', ['-xzf', '-'], { cwd: extracted, stdio: [fd, 'ignore', 'pipe'] });
    } finally {
      fs.closeSync(fd);
    }
    sameDescriptor(extracted, expected);
    const name = fs.existsSync(path.join(extracted, 'bin/taichuy-calendar.exe'))
      ? 'taichuy-calendar.exe' : 'taichuy-calendar';
    const result = await (await checker(pluginRoot))(extracted, path.join(extracted, 'bin', name));
    sameDescriptor(pluginRoot, expected);
    return { status: 'package_assets_verified', ...result };
  } finally {
    fs.rmSync(extracted, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, pluginRoot, snapshotFile, archive, ...extra] = process.argv.slice(2);
  assert.ok(pluginRoot && snapshotFile && !extra.length
    && ((command === 'snapshot' && !archive) || (command === 'verify-package' && archive)),
  'Usage: assets.mjs snapshot PLUGIN_ROOT SNAPSHOT | verify-package PLUGIN_ROOT SNAPSHOT ARCHIVE');
  const result = command === 'snapshot' ? await snapshotDescriptor(pluginRoot, snapshotFile)
    : await verifyPackageAssets(archive, pluginRoot, snapshotFile);
  console.log(JSON.stringify(result));
}
