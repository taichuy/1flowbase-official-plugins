import { mkdir, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { root, sourceNodeModules, loadEsbuild, verifyBusinessVersions } from './toolchain.mjs';
import { publicExports, hostModules, assertBundlingBoundary, isBundledHostMetadata } from './module-contract.mjs';
import { buildBusinessModules } from './business-modules.mjs';
import { writeManifest } from './manifest.mjs';
import { buildBackend } from './build-backend.mjs';
import { writeBundledLicenses } from './licenses.mjs';

await verifyBusinessVersions();
const esbuild = await loadEsbuild();
const bySource = new Map(hostModules.map((module) => [module.source, module]));
const result = await esbuild.build({
  absWorkingDir: root, entryPoints: ['src/index.tsx'], bundle: true, write: false,
  format: 'cjs', platform: 'browser', target: 'es2022', jsx: 'transform',
  minify: true, legalComments: 'inline', metafile: true,
  nodePaths: [sourceNodeModules], define: { 'process.env.NODE_ENV': '"production"' },
  plugins: [{ name: 'host-namespace-externals', setup(build) {
    build.onResolve({ filter: /.*/ }, (args) => {
      if (bySource.has(args.path)) {
        // Shim namespace.default explicitly. ESM namespaces do not promise a CJS __esModule flag.
        if (args.namespace === 'host-namespace') return { path: args.path, external: true };
        return { path: args.path, namespace: 'host-namespace' };
      }
      if (args.path === 'react-dom' || args.path.startsWith('react/') || args.path.startsWith('react-dom/')) {
        throw new Error(`Undeclared React import ${args.path}; React must never be privately bundled`);
      }
      return undefined;
    });
    build.onLoad({ filter: /.*/, namespace: 'host-namespace' }, ({ path }) => ({
      loader: 'js',
      contents: `const host = require(${JSON.stringify(path)});\n` + bySource.get(path).exports.map((name) =>
        name === 'default' ? 'export default host.default;' : `export const ${name} = host.${name};`).join('\n'),
    }));
  } }],
});
const imported = new Set(Object.values(result.metafile.outputs).flatMap((output) => output.imports.map((item) => item.path)));
for (const name of imported) if (!bySource.has(name)) throw new Error(`Undeclared external ${name}`);
assertBundlingBoundary(Object.keys(result.metafile.inputs));
// Do not evaluate a CJS bundle in a blob with bare imports. The host resolves manifest bindings first.
const text = `// Generated from approved source. Rebuild with scripts/build.mjs; do not edit.\n` +
  `export function createFrontendModule(hostModules) {\n` +
  `  const module = { exports: {} };\n  const exports = module.exports;\n` +
  `  const require = (source) => {\n` +
  `    if (!Object.hasOwn(hostModules, source)) throw new Error('Missing frontend host module: ' + source);\n` +
  `    return { ...hostModules[source], __esModule: true };\n  };\n` + result.outputFiles[0].text + '\n  return module.exports;\n}\n';
await mkdir(resolve(root, '_assets/ui'), { recursive: true });
await writeFile(resolve(root, '_assets/ui/browser.js'), text);
const sha256 = createHash('sha256').update(text).digest('hex');
const business = await buildBusinessModules(esbuild);
const backend = await buildBackend(esbuild);
await writeManifest(sha256, business.modules);
await writeBundledLicenses({ ...result.metafile.inputs, ...business.inputs, ...backend.inputs });
await mkdir(resolve(root, 'migration'), { recursive: true });
await writeFile(resolve(root, 'migration/bundle-contract.json'), JSON.stringify({
  source: '@taichuy/taichuy-calendar', version: '0.1.20', factory: 'createFrontendModule',
  exports: publicExports, hostModules, externalSources: [...imported].sort(),
  businessModules: business.modules,
  asset: { path: '_assets/ui/browser.js', sha256, bytes: Buffer.byteLength(text) },
  esbuildVersion: esbuild.version,
  bundledHostMetadataInputs: Object.keys(result.metafile.inputs).filter(isBundledHostMetadata).sort(),
  // Relative paths only: reproducible across source worktrees.
  bundledPackages: [...new Set(Object.keys(result.metafile.inputs).flatMap((input) => {
    const match = [...input.matchAll(/node_modules\/((?:@[^/]+\/)?[^/]+)/g)].at(-1); return match ? [match[1]] : [];
  }))].sort(),
}, null, 2) + '\n');
console.log(JSON.stringify({ assetBytes: Buffer.byteLength(text), sha256, exports: publicExports, externalSources: [...imported].sort() }));
