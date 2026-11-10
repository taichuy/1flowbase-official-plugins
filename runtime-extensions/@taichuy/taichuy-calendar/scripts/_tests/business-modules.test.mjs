// Central QA only: run after the root build. This fixture never invokes a build or installs tools.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { Script } from 'node:vm';
import test from 'node:test';
import { root, sourceNodeModules, loadHostTool } from '../toolchain.mjs';

const expectedVersions = {
  'lunar-typescript': '1.8.6', tyme4ts: '1.3.9', iztro: '2.5.3',
  'astronomy-engine': '2.1.19', 'china-division': '2.7.0',
};
const sourceRequire = createRequire(resolve(sourceNodeModules, '../package.json'));
let ts;
try { ts = loadHostTool('typescript'); } catch { ts = sourceRequire('typescript'); }
const contract = JSON.parse(await readFile(resolve(root, 'migration/bundle-contract.json'), 'utf8'));
assert.ok(Array.isArray(contract.businessModules), 'Root build must write businessModules into bundle-contract.json first');
const modules = contract.businessModules;
const namespaces = new Map();
const texts = new Map();
const moduleDeclarations = new Map();
for (const descriptor of modules) {
  const asset = descriptor.assets?.[0];
  assert.ok(asset?.path.startsWith('_assets/ui/business/'), 'Only business assets belong in this fixture');
  const path = resolve(root, asset.path);
  texts.set(descriptor.source, await readFile(path, 'utf8'));
  moduleDeclarations.set(descriptor.source, await readFile(resolve(root, descriptor.type_declarations_file), 'utf8'));
  const { createFrontendModule } = await import(pathToFileURL(path).href);
  assert.equal(typeof createFrontendModule, 'function');
  // A self-contained factory may not even read a host/fetched namespace.
  namespaces.set(descriptor.source, createFrontendModule(new Proxy(Object.create(null), {
    get() { throw new Error('Business factory attempted to consume host/fetched modules'); },
    has() { throw new Error('Business factory attempted to inspect host/fetched modules'); },
    ownKeys() { throw new Error('Business factory attempted to enumerate host/fetched modules'); },
  })));
}

test('five exact sources have closed factory assets, real hashes, and complete actual exports', () => {
  assert.deepEqual(modules.map((item) => item.source).sort(), Object.keys(expectedVersions).sort());
  for (const descriptor of modules) {
    const { source, version, exports: names, binding, assets } = descriptor;
    const declarations = moduleDeclarations.get(source);
    assert.equal(version, expectedVersions[source]);
    assert.equal(binding, 'fetched');
    assert.equal(assets.length, 1);
    assert.equal(assets[0].role, 'browser_module');
    assert.equal(assets[0].media_type, 'text/javascript; charset=utf-8');
    const text = texts.get(source);
    assert.equal(assets[0].sha256, createHash('sha256').update(text).digest('hex'));
    assert.deepEqual(names, Object.keys(namespaces.get(source)).sort());
    assert.ok(declarations.length > 100, `${source}: missing complete declarations`);
    const parsed = ts.createSourceFile(`${source}.js`, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
    assert.equal(parsed.parseDiagnostics.length, 0);
    function visit(node) {
      assert.ok(!ts.isImportDeclaration(node), `${source}: bare import in factory`);
      assert.ok(!(ts.isExportDeclaration(node) && node.moduleSpecifier), `${source}: external re-export`);
      if (ts.isCallExpression(node)) {
        assert.notEqual(node.expression.kind, ts.SyntaxKind.ImportKeyword, `${source}: dynamic import`);
        assert.ok(!(ts.isIdentifier(node.expression) && ['require', '__require'].includes(node.expression.text)),
          `${source}: runtime Node require`);
      }
      ts.forEachChild(node, visit);
    }
    visit(parsed);
    // The generated ESM wrapper is the only syntax removed. No Node globals are provided.
    const isolated = new Script(text.replace('export function', 'function') + '\nObject.keys(createFrontendModule()).sort();')
      .runInNewContext(Object.create(null), { timeout: 5000 });
    assert.deepEqual(Array.from(isolated), names);
    if (source !== 'china-division') {
      assert.deepEqual(names, Object.keys(sourceRequire(source)).sort(), `${source}: namespace was truncated`);
    }
  }
});

test('lunar-typescript covers Spring Festival and the 2023 leap second month', () => {
  const { Solar } = namespaces.get('lunar-typescript');
  const festival = Solar.fromYmd(2024, 2, 10).getLunar();
  assert.deepEqual([festival.getYear(), festival.getMonth(), festival.getDay()], [2024, 1, 1]);
  const leap = Solar.fromYmd(2023, 3, 22).getLunar();
  assert.deepEqual([leap.getYear(), leap.getMonth(), leap.getDay()], [2023, -2, 1]);
});

test('tyme4ts performs a solar/lunar roundtrip without a fetched lunar factory', () => {
  const { SolarDay } = namespaces.get('tyme4ts');
  const lunar = SolarDay.fromYmd(2024, 2, 10).getLunarDay();
  assert.deepEqual([lunar.getYear(), lunar.getMonth(), lunar.getDay()], [2024, 1, 1]);
  const roundtrip = lunar.getSolarDay();
  assert.deepEqual([roundtrip.getYear(), roundtrip.getMonth(), roundtrip.getDay()], [2024, 2, 10]);
});

test('iztro generates twelve palaces with bundled transitive lunar/i18next/dayjs code', () => {
  const { astro } = namespaces.get('iztro');
  const chart = astro.bySolar('2024-2-10', 2, '男', true, 'zh-CN');
  assert.equal(chart.palaces.length, 12);
  assert.equal(new Set(chart.palaces.map((palace) => palace.name)).size, 12);
  const raw = chart.rawDates.lunarDate;
  assert.deepEqual([raw.lunarYear, raw.lunarMonth, raw.lunarDay, raw.isLeap], [2024, 1, 1, false]);
  const horoscope = chart.horoscope(new Date('2026-11-01T12:00:00+08:00'));
  assert.ok(horoscope.decadal);
  assert.ok(horoscope.yearly);
  assert.ok(horoscope.monthly);
});

test('astronomy-engine computes J2000 and the March equinox', () => {
  const { MakeTime, SunPosition } = namespaces.get('astronomy-engine');
  assert.equal(MakeTime(new Date('2000-01-01T12:00:00Z')).ut, 0);
  const { elon } = SunPosition(new Date('2024-03-20T03:06:00Z'));
  assert.ok(Number.isFinite(elon));
  assert.ok(Math.min(Math.abs(elon), Math.abs(360 - elon)) < 0.05);
});

test('china-division keeps upstream names and precise depth, with no village/street or Node entry', () => {
  const namespace = namespaces.get('china-division');
  const names = ['provinces', 'cities', 'areas', 'pc', 'pcC', 'pca', 'pcaC'];
  assert.deepEqual(Object.keys(namespace).sort(), [...names, 'default'].sort());
  assert.deepEqual(Object.keys(namespace.default).sort(), [...names].sort());
  for (const name of names) assert.strictEqual(namespace.default[name], namespace[name]);
  assert.equal(namespace.provinces.find((province) => province.code === '11').name, '北京市');
  assert.equal(namespace.cities.find((city) => city.code === '1101').provinceCode, '11');
  assert.equal(namespace.areas.find((area) => area.code === '110101').name, '东城区');
  assert.ok(namespace.pc['北京市'].includes('东城区'));
  assert.ok(namespace.pca['北京市']['市辖区'].includes('东城区'));
  function checkTree(nodes, remainingDepth) {
    for (const node of nodes) {
      assert.equal(typeof node.code, 'string');
      assert.equal(typeof node.name, 'string');
      if (remainingDepth) {
        assert.ok(Array.isArray(node.children));
        checkTree(node.children, remainingDepth - 1);
      } else assert.ok(!Object.hasOwn(node, 'children'));
    }
  }
  checkTree(namespace.pcC, 1);
  checkTree(namespace.pcaC, 2);
  assert.doesNotMatch(texts.get('china-division'), /(?:villages|streets|pcas)(?:-code)?\.json|china-division\/lib\/export\.js/);
});

test('published declarations compile real consumers and reject unavailable fields/subpaths', async () => {
  const virtualFiles = new Map();
  const roots = [];
  const externalEntries = new Map();
  for (const descriptor of modules) {
    const path = resolve(root, `types/business/.qa-virtual/${descriptor.source}.d.ts`);
    virtualFiles.set(path, moduleDeclarations.get(descriptor.source));
    roots.push(path);
    if (descriptor.source !== 'iztro') externalEntries.set(descriptor.source, path);
  }
  const consumer = resolve(root, 'types/business/_tests/consumer.ts');
  virtualFiles.set(consumer, await readFile(consumer, 'utf8'));
  roots.push(consumer);
  const options = {
    noEmit: true, strict: true, skipLibCheck: false, types: [],
    module: ts.ModuleKind.ESNext, moduleResolution: ts.ModuleResolutionKind.Node10,
    target: ts.ScriptTarget.ES2022, lib: ['lib.es2022.d.ts', 'lib.dom.d.ts'], esModuleInterop: true,
  };
  const host = ts.createCompilerHost(options);
  const baseRead = host.readFile.bind(host);
  host.readFile = (path) => virtualFiles.get(path) ?? baseRead(path);
  host.fileExists = (path) => virtualFiles.has(path) || ts.sys.fileExists(path);
  host.getSourceFile = (path, languageVersion) => {
    const text = host.readFile(path);
    return text === undefined ? undefined : ts.createSourceFile(path, text, languageVersion, true);
  };
  // No installed iztro/lunar-lite/i18next fallback: their ambient declarations must stand alone.
  host.resolveModuleNames = (names) => names.map((name) => {
    const path = externalEntries.get(name);
    if (path) return { resolvedFileName: path, extension: ts.Extension.Dts };
    if (name === 'dayjs') return { resolvedFileName: resolve(sourceNodeModules, 'dayjs/index.d.ts'), extension: ts.Extension.Dts };
    return undefined;
  });
  const program = ts.createProgram(roots, options, host);
  const diagnostics = ts.getPreEmitDiagnostics(program);
  assert.equal(diagnostics.length, 0, ts.formatDiagnostics(diagnostics.slice(0, 20), {
    getCanonicalFileName: (path) => path, getCurrentDirectory: () => root, getNewLine: () => '\n',
  }));
  const iztro = ts.createSourceFile('iztro.d.ts', moduleDeclarations.get('iztro'),
    ts.ScriptTarget.Latest, true);
  assert.ok(iztro.statements.every((node) => ts.isModuleDeclaration(node) && ts.isStringLiteral(node.name)),
    'The iztro declarations must be a script of absolute ambient modules, not an outer external module');
  const ambientNames = iztro.statements.map((node) => node.name.text);
  assert.equal(new Set(ambientNames).size, ambientNames.length, 'No ambiguous ambient source names');
  assert.ok(ambientNames.includes('iztro'));
  // Independently resolve the actual public entry with TypeScript, rather than
  // copying the generator's traversal or treating every lib file as public.
  const upstreamRoot = resolve(sourceNodeModules, 'iztro');
  const metadata = JSON.parse(await readFile(resolve(upstreamRoot, 'package.json'), 'utf8'));
  const entry = resolve(upstreamRoot, metadata.types ?? metadata.typings ?? 'index.d.ts');
  const upstream = ts.createProgram([entry], options);
  const expectedIztro = upstream.getSourceFiles()
    .map((file) => file.fileName).filter((file) => file.startsWith(upstreamRoot + '/'))
    .map((file) => file === entry ? 'iztro' : 'iztro/' + file.slice(upstreamRoot.length + 1).replace(/\.d\.ts$/, ''));
  assert.ok(expectedIztro.length > 0);
  assert.deepEqual(ambientNames.filter((name) => name === 'iztro' || name.startsWith('iztro/')).sort(),
    expectedIztro.sort(), 'Publish exactly the upstream public declaration graph, not unreachable files');
  assert.ok(ambientNames.includes('lunar-lite/lib/types'));
  assert.ok(ambientNames.includes('i18next'));
  function checkSpecifier(node) {
    if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier) {
      assert.ok(!node.moduleSpecifier.text.startsWith('.'), 'Relative declaration imports cannot escape their source file');
    }
    if (ts.isImportTypeNode(node)) assert.ok(!node.argument.literal.text.startsWith('.'));
    ts.forEachChild(node, checkSpecifier);
  }
  checkSpecifier(iztro);
});
