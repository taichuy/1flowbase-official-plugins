import assert from 'node:assert/strict';
import { readFile, access } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { root } from './toolchain.mjs';
import { manifestText } from './manifest.mjs';
import { publicExports, hostModules, businessDependencies, bundledSupportDependencies, isBundledHostMetadata } from './module-contract.mjs';

// Release readiness only: this does NOT claim host installation or UI acceptance.
const browser = await readFile(resolve(root, '_assets/ui/browser.js'), 'utf8');
const digest = createHash('sha256').update(browser).digest('hex');
const contract = JSON.parse(await readFile(resolve(root, 'migration/bundle-contract.json'), 'utf8'));
assert.equal(await readFile(resolve(root, 'manifest.yaml'), 'utf8'), await manifestText(digest, contract.businessModules),
  'Manifest must be rebuilt from the current asset/host-module contract');
assert.equal((await readFile(resolve(root, 'manifest.yaml'), 'utf8')).match(/ui_registration: component/g)?.length, 6, 'Every UI contribution must register only as a component');

assert.equal(contract.asset.sha256, digest);
assert.equal(contract.asset.bytes, Buffer.byteLength(browser));
assert.deepEqual(contract.exports, publicExports);
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
console.log(JSON.stringify({ status: 'release_files_ready', sha256: digest, exports: publicExports,
  note: 'Host schema/loader installation and component behavior still require centralized QA.' }));

assert.equal(contract.businessModules.length, 5);
for (const module of contract.businessModules) {
  assert.equal(module.version, businessDependencies[module.source]);
  assert.ok(module.type_declarations.trim());
  for (const asset of module.assets) {
    const bytes = await readFile(resolve(root, asset.path));
    assert.equal(createHash('sha256').update(bytes).digest('hex'), asset.sha256);
  }
}


// Release backend identity is compiled into the executable as the same descriptor.
// Runtime behavior and registration still require real-binary/Host acceptance.
const descriptor = JSON.parse(await readFile(resolve(root, '_assets/runtime/descriptor.json'), 'utf8'));
const backend = await readFile(resolve(root, descriptor.path));
const metadata = JSON.parse(await readFile(resolve(root, 'package.json'), 'utf8'));
const backendContract = JSON.parse(await readFile(resolve(root, 'migration/backend-contract.json'), 'utf8'));
assert.equal(descriptor.version, metadata.version);
assert.equal(descriptor.path, '_assets/runtime/backend.js');
assert.equal(descriptor.bytes, backend.byteLength);
assert.equal(descriptor.sha256, createHash('sha256').update(backend).digest('hex'));
for (const key of ['version', 'path', 'bytes', 'sha256']) assert.equal(backendContract[key], descriptor[key]);
for (const [name, version] of Object.entries(businessDependencies)) assert.equal(backendContract.packages[name], version);
assert.ok(backendContract.inputs.length > 0);
assert.ok(!backendContract.inputs.some(input => input.startsWith('/home/')), 'No build-machine absolute source paths in release metadata');
assert.match(await readFile(resolve(root, 'manifest.yaml'), 'utf8'), /minimum_host_version: 0\.6\.2(?:\n|$)/);
console.log(JSON.stringify({ status: 'backend_release_files_ready', ...descriptor, note: 'Not a substitute for installed API/authorization/runtime acceptance.' }));
