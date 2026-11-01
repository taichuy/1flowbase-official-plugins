import assert from 'node:assert/strict';
import { readFile, access } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { root } from './toolchain.mjs';
import { manifestText } from './manifest.mjs';
import { componentExports, hostModules, businessDependencies, bundledSupportDependencies, isBundledHostMetadata } from './module-contract.mjs';

// Release readiness only: this does NOT claim host installation or UI acceptance.
const browser = await readFile(resolve(root, '_assets/ui/browser.js'), 'utf8');
const digest = createHash('sha256').update(browser).digest('hex');
assert.equal(await readFile(resolve(root, 'manifest.yaml'), 'utf8'), await manifestText(digest),
  'Manifest must be rebuilt from the current asset/host-module contract');
const contract = JSON.parse(await readFile(resolve(root, 'migration/bundle-contract.json'), 'utf8'));
assert.equal(contract.asset.sha256, digest);
assert.equal(contract.asset.bytes, Buffer.byteLength(browser));
assert.deepEqual(contract.exports, componentExports);
assert.deepEqual(contract.hostModules, hostModules);
assert.deepEqual(contract.externalSources, hostModules.map((module) => module.source).sort());
for (const name of Object.keys({ ...businessDependencies, ...bundledSupportDependencies })) assert.ok(contract.bundledPackages.includes(name), `${name} not bundled`);
for (const name of ['react', 'react-dom', 'antd-style', 'clsx', '@ant-design/icons']) {
  assert.ok(!contract.bundledPackages.includes(name), `Host dependency ${name} must not be bundled`);
}
assert.ok(contract.bundledHostMetadataInputs?.length, 'Expected bundled Chinese locale metadata');
for (const input of contract.bundledHostMetadataInputs) {
  assert.ok(isBundledHostMetadata(input), `Non-locale host implementation bundled: ${input}`);
}
for (const name of ['antd', 'dayjs']) {
  assert.ok(contract.bundledHostMetadataInputs.some((input) => input.includes(`/node_modules/${name}/`)
    || input.startsWith(`node_modules/${name}/`)), `${name} locale metadata missing`);
}
const licenses = await readFile(resolve(root, '_assets/ui/THIRD_PARTY_LICENSES.txt'), 'utf8');
for (const [name, version] of Object.entries({ ...businessDependencies, ...bundledSupportDependencies })) assert.ok(licenses.includes(`${name}@${version}`));
// This import evaluates only the ESM wrapper, not the CJS body or any React component.
const namespace = await import(pathToFileURL(resolve(root, '_assets/ui/browser.js')).href);
assert.equal(typeof namespace.createFrontendModule, 'function');
assert.deepEqual(Object.keys(namespace), ['createFrontendModule']);
await access(resolve(root, `bin/taichuy-calendar${process.platform === 'win32' ? '.exe' : ''}`));
console.log(JSON.stringify({ status: 'release_files_ready', sha256: digest, exports: componentExports,
  note: 'Host schema/loader installation and component behavior still require centralized QA.' }));
