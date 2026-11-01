import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { root, hostRoot, sourceNodeModules, loadEsbuild, verifyBusinessVersions } from './toolchain.mjs';
import { sourceOracles } from './source-oracles.mjs';

// Root's centralized QA owns execution and this evidence directory. No service is started.
const evidenceRoot = process.env.TAICHUY_EVIDENCE_ROOT ?? resolve(hostRoot, 'tmp/test-governance/taichuy-calendar');
const outdir = resolve(evidenceRoot, 'compiled');
await mkdir(outdir, { recursive: true });
await verifyBusinessVersions();
const esbuild = await loadEsbuild();
const oracles = await sourceOracles();
await esbuild.build({
  absWorkingDir: root, entryPoints: ['src/_tests/algorithms.fixture.ts'],
  outfile: resolve(outdir, 'algorithms.test.cjs'), format: 'cjs', platform: 'node',
  target: 'node22', bundle: true, nodePaths: [sourceNodeModules],
  define: { 'process.env.NODE_ENV': '"test"' },
  plugins: [{ name: 'approved-source-oracles', setup(build) {
    build.onResolve({ filter: /^approved-source-oracles$/ }, () => ({ path: 'oracles', namespace: 'approved' }));
    build.onLoad({ filter: /.*/, namespace: 'approved' }, () => ({ contents: oracles, loader: 'ts', resolveDir: root }));
  } }],
});
// QA-only harness deliberately bundles host dependencies once. It is NEVER a release asset.
// All host packages resolve from the same host installation (including transitive React).
await esbuild.build({
  absWorkingDir: root, entryPoints: ['src/_tests/factory-browser.fixture.tsx'],
  outfile: resolve(outdir, 'factory-browser.js'), format: 'iife', platform: 'browser',
  target: 'es2022', bundle: true, jsx: 'transform', legalComments: 'inline',
  nodePaths: [resolve(hostRoot, 'web/app/node_modules'), resolve(hostRoot, 'web/node_modules')],
  define: { 'process.env.NODE_ENV': '"production"' },
  plugins: [{ name: 'test-host-root', setup(build) {
    build.onResolve({ filter: /^@taichuy\/taichuy-calendar$/ }, () => ({ path: resolve(root, 'src/_tests/public-components.fixture.tsx') }));
    // The harness lives outside the host; stop Node resolution falling back to source-site React.
    build.onResolve({ filter: /^(react(?:-dom)?(?:\/.*)?|antd(?:\/.*)?|antd-style|dayjs(?:\/.*)?|clsx|@ant-design\/icons)$/ }, async (args) => {
      if (args.pluginData?.hostResolved) return undefined;
      const result = await build.resolve(args.path, { kind: args.kind,
        resolveDir: resolve(hostRoot, 'web/app'), pluginData: { hostResolved: true } });
      return { ...result, pluginData: { hostResolved: true } };
    });
  } }],
});
await writeFile(resolve(evidenceRoot, 'fixture-plan.json'), JSON.stringify({
  status: 'prepared_not_executed', timezone: 'Asia/Shanghai',
  algorithmCommand: `TZ=Asia/Shanghai node --test ${resolve(outdir, 'algorithms.test.cjs')}`,
  browserCommand: `node ${resolve(root, 'scripts/run-component-fixtures.mjs')}`,
  source: 'Approved source hashes are checked before deriving independent reference calculations.',
  factory: 'Real generated browser.js, frozen ESM-like host namespaces, actual host React/Antd/dayjs.',
}, null, 2) + '\n');
console.log(JSON.stringify({ evidenceRoot, status: 'prepared_not_executed' }));
