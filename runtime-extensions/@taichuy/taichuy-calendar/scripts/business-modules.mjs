import { createHash } from 'node:crypto';
import { createRequire, builtinModules } from 'node:module';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, relative, resolve, sep } from 'node:path';
import { Script } from 'node:vm';
import { root, sourceNodeModules, loadHostTool } from './toolchain.mjs';

// This packet owns the approved exact versions, independently of host UI bindings.
const versions = Object.freeze({
  'lunar-typescript': '1.8.6', tyme4ts: '1.3.9', iztro: '2.5.3',
  'astronomy-engine': '2.1.19', 'china-division': '2.7.0',
});
const builtins = new Set(builtinModules.map((name) => name.replace(/^node:/, '')));
const preservedTypeSources = new Set(['lunar-typescript', 'dayjs']);
const declarationPackages = new Set(['iztro', 'lunar-lite', 'i18next']);
const forbiddenHost = /^(?:react(?:-dom)?|antd|antd-style|@ant-design\/icons|clsx)(?:\/|$)/;

function loadTypeScript() {
  try { return loadHostTool('typescript'); }
  catch {
    // Resolve only the already-installed source toolchain, never install anything.
    return createRequire(resolve(sourceNodeModules, '../package.json'))('typescript');
  }
}

async function approvedMetadata() {
  const metadata = new Map();
  for (const [source, version] of Object.entries(versions)) {
    const dir = resolve(sourceNodeModules, source);
    const value = JSON.parse(await readFile(resolve(dir, 'package.json'), 'utf8'));
    if (value.version !== version) throw new Error(`${source}: expected ${version}, found ${value.version}`);
    metadata.set(source, { ...value, dir });
  }
  return metadata;
}

/** Preserve upstream declarations, rewriting only module references and ambient declare modifiers. */
async function iztroDeclarations(ts, packageRoot) {
  const files = new Map();
  const pending = [];
  const ownerCache = new Map();
  const options = { moduleResolution: ts.ModuleResolutionKind.Node10, target: ts.ScriptTarget.ES2022 };
  const resolutionCache = ts.createModuleResolutionCache(sourceNodeModules, (value) => value, options);

  async function owner(file) {
    let dir = dirname(file);
    while (dir !== dirname(dir)) {
      if (ownerCache.has(dir)) return ownerCache.get(dir);
      let text;
      try { text = await readFile(resolve(dir, 'package.json'), 'utf8'); }
      catch (error) { if (error.code !== 'ENOENT') throw error; }
      if (text) {
        const metadata = JSON.parse(text);
        const result = { dir, ...metadata };
        ownerCache.set(dir, result);
        return result;
      }
      dir = dirname(dir);
    }
    throw new Error(`No declaration package owner: ${file}`);
  }

  async function enqueue(file) {
    file = resolve(file);
    if (files.has(file)) return files.get(file).name;
    const pkg = await owner(file);
    if (!declarationPackages.has(pkg.name)) throw new Error(`Unsupported iztro type dependency: ${pkg.name} (${file})`);
    if (!/\.d\.[cm]?ts$/.test(file)) throw new Error(`Expected a declaration file: ${file}`);
    const entry = resolve(pkg.dir, pkg.types ?? pkg.typings ?? 'index.d.ts');
    const suffix = relative(pkg.dir, file).split(sep).join('/').replace(/\.d\.[cm]?ts$/, '');
    const name = file === entry ? pkg.name : `${pkg.name}/${suffix}`;
    const record = { name, file };
    files.set(file, record);
    pending.push(record);
    return name;
  }

  // Only publish declarations reachable from the public type entry. Scanning all
  // of lib promotes upstream's stale, unexported files into ambient roots.
  const metadata = JSON.parse(await readFile(resolve(packageRoot, 'package.json'), 'utf8'));
  await enqueue(resolve(packageRoot, metadata.types ?? metadata.typings ?? 'index.d.ts'));
  for (let index = 0; index < pending.length; index++) {
    const record = pending[index];
    const source = ts.createSourceFile(record.file, await readFile(record.file, 'utf8'), ts.ScriptTarget.Latest, true);
    if (source.parseDiagnostics.length) throw new Error(`Cannot parse declaration: ${record.file}`);
    if (source.referencedFiles.length || source.typeReferenceDirectives.length || source.libReferenceDirectives.length) {
      throw new Error(`Unsupported reference directive in ${record.file}; do not discard type dependencies`);
    }
    const references = [];
    function visit(node) {
      if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier) {
        references.push(node.moduleSpecifier);
      } else if (ts.isImportTypeNode(node)) {
        if (!ts.isLiteralTypeNode(node.argument) || !ts.isStringLiteral(node.argument.literal)) {
          throw new Error(`Unsupported import type in ${record.file}`);
        }
        references.push(node.argument.literal);
      } else if (ts.isExternalModuleReference(node)) {
        references.push(node.expression);
      } else if (ts.isModuleDeclaration(node) && (ts.isStringLiteral(node.name) || node.flags & ts.NodeFlags.GlobalAugmentation)) {
        throw new Error(`Unsupported declaration augmentation in ${record.file}`);
      }
      ts.forEachChild(node, visit);
    }
    visit(source);
    const replacements = new Map();
    for (const node of references) {
      if (!node || !ts.isStringLiteral(node)) throw new Error(`Unsupported module specifier in ${record.file}`);
      if (preservedTypeSources.has(node.text)) continue;
      const resolved = ts.resolveModuleName(node.text, record.file, options, ts.sys, resolutionCache).resolvedModule;
      if (!resolved) throw new Error(`Cannot resolve ${node.text} from ${record.file}`);
      replacements.set(node, await enqueue(resolved.resolvedFileName));
    }
    const transformed = ts.transform(source, [(context) => {
      function rewrite(node) {
        if (replacements.has(node)) return ts.factory.createStringLiteral(replacements.get(node));
        // Inside declare module, declarations are already ambient (TS1038 otherwise).
        if (node.kind === ts.SyntaxKind.DeclareKeyword) return undefined;
        return ts.visitEachChild(node, rewrite, context);
      }
      return (node) => ts.visitNode(node, rewrite);
    }]);
    try {
      const ambient = ts.factory.createModuleDeclaration(
        [ts.factory.createModifier(ts.SyntaxKind.DeclareKeyword)], ts.factory.createStringLiteral(record.name),
        ts.factory.createModuleBlock(transformed.transformed[0].statements), ts.NodeFlags.None,
      );
      record.text = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed }).printFile(
        ts.factory.updateSourceFile(source, [ambient]),
      );
    } finally { transformed.dispose(); }
  }
  return [...files.values()].sort((a, b) => a.name.localeCompare(b.name)).map((record) => record.text).join('\n');
}

/** Raw single-file upstream declarations; iztro is one script containing absolute ambient modules. */
export async function readBusinessTypeDeclarations() {
  const metadata = await approvedMetadata();
  const result = {};
  for (const source of Object.keys(versions)) {
    const pkg = metadata.get(source);
    result[source] = source === 'iztro' ? await iztroDeclarations(loadTypeScript(), pkg.dir)
      : source === 'china-division' ? await readFile(resolve(root, 'types/business/china-division.d.ts'), 'utf8')
        : await readFile(resolve(pkg.dir, pkg.types ?? pkg.typings), 'utf8');
  }
  return result;
}

function assertClosedBundle(ts, text, metafile) {
  for (const input of Object.keys(metafile.inputs)) {
    if (/(?:^|\/)node_modules\/(?:react(?:-dom)?|antd|antd-style|@ant-design\/icons|clsx)(?:\/|$)/.test(input)) {
      throw new Error(`Host UI implementation bundled into business factory: ${input}`);
    }
  }
  for (const output of Object.values(metafile.outputs)) {
    if (output.imports.length) throw new Error(`Business factory contains unresolved imports: ${JSON.stringify(output.imports)}`);
  }
  const source = ts.createSourceFile('factory.cjs', text, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  function visit(node) {
    if (ts.isCallExpression(node) && (node.expression.kind === ts.SyntaxKind.ImportKeyword
      || ts.isIdentifier(node.expression) && ['require', '__require'].includes(node.expression.text))) {
      throw new Error('Business factory contains a runtime import/require; every business dependency must be bundled');
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
}

/** Called by the existing root build; no CLI side effects and no host/fetched prerequisites. */
export async function buildBusinessModules(esbuild) {
  const declarations = await readBusinessTypeDeclarations();
  const ts = loadTypeScript();
  const modules = [];
  const inputs = {};
  for (const [source, version] of Object.entries(versions)) {
    const result = await esbuild.build({
      absWorkingDir: root,
      ...(source === 'china-division' ? { entryPoints: ['src/business/china-division.ts'] }
        : { stdin: { contents: `module.exports = require(${JSON.stringify(source)});`,
          resolveDir: root, sourcefile: `business-entry:${source}.cjs`, loader: 'js' } }),
      bundle: true, write: false, format: 'cjs', platform: 'browser', target: 'es2022',
      minify: true, legalComments: 'inline', metafile: true, nodePaths: [sourceNodeModules],
      define: { 'process.env.NODE_ENV': '"production"' },
      logLevel: 'silent',
      plugins: [{ name: 'closed-business-module', setup(build) {
        build.onResolve({ filter: /.*/ }, (args) => {
          if (args.path.startsWith('node:') || builtins.has(args.path) || forbiddenHost.test(args.path)) {
            throw new Error(`Forbidden business dependency: ${args.path}`);
          }
          return undefined;
        });
      } }],
    });
    if (result.warnings.length) throw new Error(`Business bundle warnings (${source}): ${result.warnings.map((item) => item.text).join('; ')}`);
    if (result.outputFiles.length !== 1) throw new Error(`Expected one closed factory asset for ${source}`);
    const body = result.outputFiles[0].text;
    assertClosedBundle(ts, body, result.metafile);
    const text = '// Generated by scripts/business-modules.mjs; do not edit.\n'
      + 'export function createFrontendModule() {\n'
      + '  const module = { exports: {} };\n  const exports = module.exports;\n'
      + body + '\n  return module.exports;\n}\n';
    // Enumerate the actual bundled namespace, not a manually maintained export allowlist.
    const names = new Script(text.replace('export function', 'function')
      + '\nObject.keys(createFrontendModule()).sort();', { filename: `${source}-exports.js` })
      .runInNewContext(Object.create(null), { timeout: 5000 });
    if (!names.length) throw new Error(`Empty business namespace: ${source}`);
    const path = `_assets/ui/business/${source}.js`;
    await mkdir(dirname(resolve(root, path)), { recursive: true });
    await writeFile(resolve(root, path), text);
    const typeFile = `ui/types/business/${source}.d.ts`;
    await mkdir(dirname(resolve(root, typeFile)), { recursive: true });
    await writeFile(resolve(root, typeFile), declarations[source].replace(/[\t ]+$/gm, ''));
    modules.push({ source, version, exports: Array.from(names), binding: 'fetched',
      assets: [{ path, role: 'browser_module', media_type: 'text/javascript; charset=utf-8',
        sha256: createHash('sha256').update(text).digest('hex') }],
      type_declarations_file: typeFile });
    // Match esbuild's inputs shape for the existing license collector.
    for (const [path, input] of Object.entries(result.metafile.inputs)) {
      if (!path.startsWith('business-entry:')) inputs[path] = input;
    }
  }
  return { modules, inputs };
}
