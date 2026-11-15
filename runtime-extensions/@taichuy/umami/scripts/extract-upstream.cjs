// Extract the actual colocated Umami contracts without executing route handlers.
// Development-only tools reuse TypeScript and Zod already installed in the host.
const fs = require('node:fs');
const path = require('node:path');
const { createRequire } = require('node:module');
const { execFileSync } = require('node:child_process');

function dependency(name, moduleRoot) {
  const requireFrom = createRequire(path.join(moduleRoot, 'package.json'));
  try { return requireFrom(name); } catch (error) {
    if (error.code !== 'MODULE_NOT_FOUND') throw error;
    const store = path.join(moduleRoot, 'node_modules/.pnpm');
    const candidates = fs.readdirSync(store).filter(entry => entry.startsWith(`${name}@`));
    if (candidates.length !== 1) throw new Error(`Specify an installed module root with one ${name} version`);
    return requireFrom(path.join(store, candidates[0], 'node_modules', name));
  }
}

function extract(upstreamRoot, moduleRoot) {
  const ts = dependency('typescript', moduleRoot);
  const { z } = dependency('zod', moduleRoot);
  const sourceRoot = path.join(upstreamRoot, 'src');
  const cache = new Map();
  const walk = directory => fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const file = path.join(directory, entry.name);
    return entry.isDirectory() ? walk(file) : [file];
  }).sort();
  const convert = value => {
    if (value?._zod) return clean(z.toJSONSchema(value, { io: 'input', unrepresentable: 'any' }));
    if (Array.isArray(value)) return value.map(convert);
    if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, convert(child)]));
    return value;
  };
  function clean(value) {
    if (Array.isArray(value)) return value.map(clean);
    if (!value || typeof value !== 'object') return value;
    return Object.fromEntries(Object.entries(value).filter(([key]) => !['$schema', 'id'].includes(key)).map(([key, child]) => [key, clean(child)]));
  }
  // Date validation callbacks are not executed by schema generation. Extract the
  // real enum from source, and fail if extraction accidentally calls a callback.
  const dates = ts.createSourceFile('date.ts', fs.readFileSync(path.join(sourceRoot, 'lib/date.ts'), 'utf8'), ts.ScriptTarget.Latest, true);
  const periodDeclaration = dates.statements.find(statement => ts.isVariableStatement(statement) && statement.declarationList.declarations.some(d => d.name.getText(dates) === 'DATE_PERIODS'));
  if (!periodDeclaration) throw new Error('Umami DATE_PERIODS declaration missing');
  const periodModule = { exports: {} };
  new Function('exports', ts.transpileModule(periodDeclaration.getText(dates), { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText)(periodModule.exports);
  const dateSchemaImports = { ...periodModule.exports };
  for (const name of ['isValidTimezone', 'normalizeTimezone']) dateSchemaImports[name] = () => { throw new Error(`Schema extraction must not execute ${name}`); };

  function resolve(source, parent) {
    const stem = source.startsWith('@/') ? path.join(sourceRoot, source.slice(2)) : path.resolve(path.dirname(parent), source);
    for (const candidate of [stem, `${stem}.ts`, `${stem}.tsx`, path.join(stem, 'index.ts')]) if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) return candidate;
    throw new Error(`Cannot resolve ${source} from ${parent}`);
  }
  function load(file) {
    if (cache.has(file)) return cache.get(file).exports;
    const module = { exports: {} }; cache.set(file, module);
    const source = fs.readFileSync(file, 'utf8');
    const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
    const requireSchema = name => {
      if (name === 'zod') return { z, ...z };
      if (name === '@/lib/date') return dateSchemaImports;
      if (name.startsWith('.') || name.startsWith('@/')) return load(resolve(name, file));
      throw new Error(`Contract extraction cannot execute runtime dependency ${name} (${file})`);
    };
    new Function('require', 'module', 'exports', compiled)(requireSchema, module, module.exports);
    return module.exports;
  }
  const operationKey = operation => `${operation.method.toUpperCase()} ${operation.path}`;
  const canonical = [];
  for (const file of walk(path.join(sourceRoot, 'app/api')).filter(file => /\/contract(?:\.generated)?\.ts$/.test(file))) {
    for (const contract of load(file).operations) {
      if (contract.method === 'options') continue;
      canonical.push({ ...convert(contract), source: path.relative(upstreamRoot, file).replaceAll(path.sep, '/'), compatibility: false });
    }
  }
  const expected = [];
  function routeInventory(directory, prefix) {
    for (const file of walk(directory).filter(file => file.endsWith('/route.ts'))) {
      const routePath = (prefix + path.relative(directory, path.dirname(file)).replaceAll(path.sep, '/')).replace(/\[([^\]]+)\]/g, '{$1}').replace(/\/$/, '');
      for (const match of fs.readFileSync(file, 'utf8').matchAll(/export\s+(?:async\s+)?function\s+(GET|POST|PUT|PATCH|DELETE|HEAD)\b/g)) expected.push({ method: match[1], path: routePath, source: path.relative(upstreamRoot, file).replaceAll(path.sep, '/') });
    }
  }
  routeInventory(path.join(sourceRoot, 'app/api'), '/api/');
  const compatDirectory = path.join(sourceRoot, 'app/(compat)/compat/api');
  routeInventory(compatDirectory, '/api/');
  const shared = load(path.join(sourceRoot, 'lib/schema.ts'));
  const compatibility = expected.filter(item => item.source.includes('(compat)')).map(item => {
    const pathFields = Object.fromEntries([...item.path.matchAll(/\{([^}]+)\}/g)].map(([, name]) => [name, { type: 'string', minLength: 1 }]));
    const query = item.method === 'GET' && !item.path.endsWith('{reportId}') ? convert(z.object({ ...(item.path === '/api/reports' ? { websiteId: z.uuid() } : {}), type: shared.reportTypeParam.optional(), ...shared.pagingParams })) : { type: 'object', properties: {} };
    const body = item.method === 'POST' ? convert(item.path === '/api/reports' || item.path.endsWith('{reportId}') ? shared.reportSchema : shared.reportResultSchema) : { type: 'object', properties: {} };
    return {
      method: item.method.toLowerCase(), path: item.path, auth: 'bearer', audience: 'internal', source: item.source, compatibility: true,
      operation: {
        operationId: `legacy_${item.method.toLowerCase()}_${item.path.replace(/^\/api\//, '').replace(/[{}]/g, '').replaceAll('/', '_')}`,
        summary: `Legacy ${item.method} ${item.path}`,
        description: 'Historical Umami report API retained by the upstream rewrite. Prefer the corresponding current website operation for new workflows.',
        requestParams: { path: { type: 'object', properties: pathFields, required: Object.keys(pathFields) }, query },
        ...(item.method === 'POST' ? { requestBody: { required: true, content: { 'application/json': { schema: body } } } } : {}),
        responses: { '200': { description: 'Original report response.', content: { 'application/json': { schema: {} } } } },
      },
    };
  });
  const contracts = [...canonical, ...compatibility].sort((a, b) => operationKey(a).localeCompare(operationKey(b)));
  const actualKeys = contracts.map(operationKey).sort();
  const expectedKeys = expected.map(operationKey).sort();
  if (JSON.stringify(actualKeys) !== JSON.stringify(expectedKeys) || new Set(actualKeys).size !== actualKeys.length) throw new Error('Route/contract operation mismatch');
  return {
    provenance: { repository: 'https://github.com/umami-software/umami', version: JSON.parse(fs.readFileSync(path.join(upstreamRoot, 'package.json'))).version, commit: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: upstreamRoot, encoding: 'utf8' }).trim(), license: 'MIT', canonical_operations: canonical.length, compatibility_operations: compatibility.length },
    routes: expected, contracts,
  };
}

module.exports = { extract };
if (require.main === module) {
  const [upstreamRoot, moduleRoot, output] = process.argv.slice(2);
  if (!upstreamRoot || !moduleRoot || !output) throw new Error('Usage: node extract-upstream.cjs <umami-root> <host-web-root> <output-json>');
  const result = extract(path.resolve(upstreamRoot), path.resolve(moduleRoot));
  fs.mkdirSync(path.dirname(output), { recursive: true });
  fs.writeFileSync(output, `${JSON.stringify(result)}\n`);
  console.log(JSON.stringify({ ...result.provenance, total_operations: result.contracts.length }));
}
