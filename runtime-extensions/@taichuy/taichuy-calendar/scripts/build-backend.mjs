import { mkdir, writeFile, readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { root, sourceNodeModules, loadEsbuild, verifyBusinessVersions } from './toolchain.mjs';
import { businessDependencies } from './module-contract.mjs';
export async function buildBackend(esbuild) {
  const { version } = JSON.parse(await readFile(resolve(root, 'package.json'), 'utf8'));
  const dayjsVersion = JSON.parse(await readFile(resolve(sourceNodeModules, 'dayjs/package.json'), 'utf8')).version;
  if (dayjsVersion !== '1.11.19') throw new Error(`Backend clock adapter expected existing dayjs 1.11.19, got ${dayjsVersion}`);
  const result = await esbuild.build({ absWorkingDir: root, entryPoints: ['src/runtime/backend/main.ts'], bundle: true, write: false, format: 'iife', globalName: '__calendar_backend', platform: 'neutral', mainFields: ['module', 'main'], target: 'es2022', minify: true, legalComments: 'inline', metafile: true, nodePaths: [sourceNodeModules] });
  const imports = Object.values(result.metafile.outputs).flatMap(v => v.imports);
  if (imports.length) throw new Error(`Backend must be closed, not external imports: ${JSON.stringify(imports)}`);
  const source = result.outputFiles[0].text + '\nfunction main(input) { return __calendar_backend.main(input); }\n';
  if (/\brequire\s*\(|\bimport\s*(?:\(|["'])|\bprocess\s*\./.test(source)) throw new Error('Backend contains a runtime Node/module dependency');
  const descriptor = { version, path: '_assets/runtime/backend.js', bytes: Buffer.byteLength(source), sha256: createHash('sha256').update(source).digest('hex') };
  await mkdir(resolve(root, '_assets/runtime'), { recursive: true });
  await writeFile(resolve(root, descriptor.path), source);
  await writeFile(resolve(root, '_assets/runtime/descriptor.json'), JSON.stringify(descriptor, null, 2) + '\n');
  await writeFile(resolve(root, 'migration/backend-contract.json'), JSON.stringify({ ...descriptor, bytes: Buffer.byteLength(source), esbuild_version: esbuild.version, packages: { ...businessDependencies, dayjs: dayjsVersion }, inputs: Object.keys(result.metafile.inputs).map(p => p.replace(sourceNodeModules, '<source-node_modules>')).sort() }, null, 2) + '\n');
  return { descriptor, inputs: result.metafile.inputs };
}
if (process.argv[1] === new URL(import.meta.url).pathname) { await verifyBusinessVersions(); console.log(JSON.stringify((await buildBackend(await loadEsbuild())).descriptor)); }
