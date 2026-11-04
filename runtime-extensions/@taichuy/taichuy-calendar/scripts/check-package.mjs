import assert from 'node:assert/strict';
import { readFile, access } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { basename, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { root } from './toolchain.mjs';
import { manifestText } from './manifest.mjs';
import { publicExports, hostModules, businessDependencies, bundledSupportDependencies, isBundledHostMetadata } from './module-contract.mjs';

// Release readiness only: this does NOT claim host installation or UI acceptance.
// Contracts, metadata and manifest generation always come from this trusted source
// root: published archives intentionally do not contain migration/, scripts/ or types/.
// null is used only by the pre-cargo asset check; the default still requires bin/.
export async function checkPackage(assetRoot = root,
  runtimeBinary = resolve(assetRoot, `bin/taichuy-calendar${process.platform === 'win32' ? '.exe' : ''}`)) {
  assetRoot = resolve(assetRoot);
  const browser = await readFile(resolve(assetRoot, '_assets/ui/browser.js'));
  const digest = createHash('sha256').update(browser).digest('hex');
  const contract = JSON.parse(await readFile(resolve(root, 'migration/bundle-contract.json'), 'utf8'));
  const generatedManifest = await manifestText(digest, contract.businessModules);
  assert.equal(await readFile(resolve(root, 'manifest.yaml'), 'utf8'), generatedManifest,
    'Manifest must be rebuilt from the current asset/host-module contract');
  // The package CLI changes runtime.entry to .exe on Windows, and nothing else.
  const expectedManifest = assetRoot !== root && basename(runtimeBinary ?? '') === 'taichuy-calendar.exe'
    ? generatedManifest.replace(/^(\s*entry: bin\/taichuy-calendar)$/gm, '$1.exe') : generatedManifest;
  const manifest = await readFile(resolve(assetRoot, 'manifest.yaml'), 'utf8');
  assert.equal(manifest, expectedManifest, 'Package manifest must match the source contract (including target runtime.entry)');
  assert.equal(manifest.match(/ui_registration: component/g)?.length, 6, 'The six reusable UI components stay in component management');
  assert.equal(manifest.match(/ui_registration: code_template/g)?.length, 1, 'Register exactly one complete editable composition template');
  assert.doesNotMatch(manifest, /^  feature:|^settings_pages:|\/settings\/taichuy-calendar/m,
    'Calendar exposes backend calculations and components without an admin settings page');

  const interfaceMetadata = [...manifest.matchAll(/^      (?:summary|description): (.+)$/gm)];
  assert.equal(interfaceMetadata.length, 16, 'All eight canonical operations must declare English summary and description');
  for (const match of interfaceMetadata) {
    assert.match(match[1], /^[\x20-\x7e]+$/, 'Canonical operation metadata must be static English ASCII');
  }

  assert.equal(contract.asset.sha256, digest);
  assert.equal(contract.asset.bytes, browser.byteLength);
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
  const licenses = await readFile(resolve(assetRoot, '_assets/ui/THIRD_PARTY_LICENSES.txt'), 'utf8');
  for (const [name, version] of Object.entries({ ...businessDependencies, ...bundledSupportDependencies })) assert.ok(licenses.includes(`${name}@${version}`));
  // This import evaluates only the ESM wrapper, not the CJS body or any React component.
  const namespace = await import(pathToFileURL(resolve(assetRoot, '_assets/ui/browser.js')).href);
  assert.equal(typeof namespace.createFrontendModule, 'function');
  assert.deepEqual(Object.keys(namespace), ['createFrontendModule']);
  if (runtimeBinary !== null) await access(resolve(runtimeBinary));

  assert.equal(contract.businessModules.length, 5);
  for (const module of contract.businessModules) {
    assert.equal(module.version, businessDependencies[module.source]);
    assert.ok(module.type_declarations.trim());
    for (const asset of module.assets) {
      const bytes = await readFile(resolve(assetRoot, asset.path));
      assert.equal(createHash('sha256').update(bytes).digest('hex'), asset.sha256);
    }
  }


  // Release backend identity is compiled into the executable as the same descriptor.
  // Runtime behavior and registration still require real-binary/Host acceptance.
  const descriptor = JSON.parse(await readFile(resolve(assetRoot, '_assets/runtime/descriptor.json'), 'utf8'));
  const backend = await readFile(resolve(assetRoot, descriptor.path));
  const metadata = JSON.parse(await readFile(resolve(root, 'package.json'), 'utf8'));
  const backendContract = JSON.parse(await readFile(resolve(root, 'migration/backend-contract.json'), 'utf8'));
  assert.equal(descriptor.version, metadata.version);
  assert.equal(descriptor.path, '_assets/runtime/backend.js');
  assert.equal(descriptor.bytes, backend.byteLength, 'Runtime backend byte length differs from descriptor');
  assert.equal(descriptor.sha256, createHash('sha256').update(backend).digest('hex'), 'Runtime backend SHA-256 differs from descriptor');
  for (const key of ['version', 'path', 'bytes', 'sha256']) assert.equal(backendContract[key], descriptor[key]);
  for (const [name, version] of Object.entries(businessDependencies)) assert.equal(backendContract.packages[name], version);
  assert.ok(backendContract.inputs.length > 0);
  assert.ok(!backendContract.inputs.some(input => input.startsWith('/home/')), 'No build-machine absolute source paths in release metadata');
  assert.match(manifest, /minimum_host_version: 0\.6\.2(?:\n|$)/);
  return {
    frontend: { status: 'release_files_ready', sha256: digest, exports: publicExports,
      note: 'Host schema/loader installation and component behavior still require centralized QA.' },
    backend: { status: 'backend_release_files_ready', ...descriptor,
      note: 'Not a substitute for installed API/authorization/runtime acceptance.' },
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  let assetRoot = root;
  let runtimeBinary;
  const seen = new Set();
  for (let index = 2; index < process.argv.length; index += 2) {
    const flag = process.argv[index];
    const value = process.argv[index + 1];
    assert.ok(['--asset-root', '--runtime-binary'].includes(flag) && !seen.has(flag)
      && value && !value.startsWith('--'), 'Expected optional --asset-root PATH / --runtime-binary PATH');
    seen.add(flag);
    if (flag === '--asset-root') assetRoot = resolve(value);
    else runtimeBinary = resolve(value);
  }
  const result = await checkPackage(assetRoot, runtimeBinary);
  console.log(JSON.stringify(result.frontend));
  console.log(JSON.stringify(result.backend));
}
