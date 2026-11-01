import { operations, object, operationId, route, outputSchema } from './runtime-contract.mjs';
import { dataSchemas } from './runtime-schemas.mjs';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { root } from './toolchain.mjs';
import { moduleSource, componentExports, publicExports, hostModules } from './module-contract.mjs';

const titles = {
  Calendar: '太初y 月历 / Lunar Calendar', Almanac: '太初y 黄历 / Almanac',
  BirthInfo: '太初y 出生信息 / Birth Information', ZiWei: '太初y 紫微斗数 / ZiWei',
  Bazi: '太初y 八字排盘 / Bazi', CalendarPage: '太初y 旧版整页（兼容） / Legacy Calendar Page',
};

// Canonical interface metadata is static English ASCII, independent of UI labels.
const interfaceSummaries = {
  'calendar.month': 'Month calendar', 'calendar.convert': 'Calendar conversion',
  'almanac.day': 'Daily almanac', 'birth.normalize': 'Normalize birth information',
  'time.true-solar': 'True solar time', 'bazi.calculate': 'Bazi chart',
  'ziwei.calculate': 'Ziwei chart', 'regions.children': 'Region children',
};

export async function manifestText(sha256, businessModules) {
  if (typeof sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(sha256)) {
    throw new Error('A real browser.js SHA-256 is required; never publish a placeholder manifest');
  }
  if (!Array.isArray(businessModules) || businessModules.length !== 5) throw new Error('Five built business module contracts are required');
  const typeDeclarations = await readFile(resolve(root, 'types/index.d.ts'), 'utf8');
  const codeModules = [
    ...hostModules.map((module) => ({ ...module, binding: 'host',
      type_declarations: module.exports.map((name) => name === 'default'
        ? 'declare const defaultExport: any; export default defaultExport;'
        : `export declare const ${name}: any;`).join('\n') })),
    { source: moduleSource, version: '0.1.13', exports: publicExports, binding: 'fetched',
      assets: [{ path: '_assets/ui/browser.js', role: 'browser_module', media_type: 'text/javascript; charset=utf-8', sha256 }],
      type_declarations: typeDeclarations },
    ...businessModules,
  ];
  const manifest = {
    manifest_version: 2, schema_version: '1flowbase.plugin.manifest/v2',
    plugin_id: 'taichuy-calendar', version: '0.1.13', publisher_namespace: '1flowbase', vendor: 'Taichuy',
    display_name: '太初y 日历排盘 / Taichuy Calendar',
    description: 'Independent lunar calendar, almanac, birth information, ZiWei and Bazi with local astronomical calculations',
    source_kind: 'official_registry', trust_level: 'verified_official', consumption_kind: 'runtime_extension',
    execution_mode: 'process_per_call', slot_codes: [], binding_targets: ['system'],
    selection_mode: 'assignment_then_select', minimum_host_version: '0.6.2',
    contract_version: '1flowbase.extension-bus/v1',
    permissions: { network: 'none', secrets: 'none', storage: 'none', mcp: 'none', subprocess: 'deny' },
    runtime: { protocol: 'stdio_json', entry: 'bin/taichuy-calendar' },
    managed: {
      module: { bus_version: 'v1', module_id: 'taichuy-calendar', module_version: '0.1.13', module_kind: 'runtime',
        contributions: [...operations.map(op => ({ contribution_id: operationId(op.name), contributor_module_id: 'taichuy-calendar', point_id: '1flowbase.managed-service.operation', contract_version: '1', required_permissions: ['service.execute'], mode: 'append' })), ...componentExports.map((name) => ({
          contribution_id: `taichuy-calendar.${name}`, contributor_module_id: 'taichuy-calendar',
          point_id: '1flowbase.workspace.frontend-block-contribution', contract_version: '1',
          required_permissions: ['frontend-block.ui-mount.trusted-host'], mode: 'append',
        }))],
      },
      // Frontend payloads stay declarative; backend bindings are explicitly separate.
      execution_bindings: [...operations.map(op => ({ contribution_id: operationId(op.name), execution_mode: 'process_per_call', runtime: { protocol: 'stdio_json', entry: 'bin/taichuy-calendar' }, handler: op.name })), ...componentExports.map((name) => ({
        contribution_id: `taichuy-calendar.${name}`, execution_mode: 'declarative_only',
        runtime: { protocol: 'stdio_json', entry: 'bin/taichuy-calendar' },
        handler: `taichuy-calendar.render.${name}`,
        payload: { kind: 'frontend_block', contribution_code: name },
      }))],
    },
    managed_service: {
      scope: 'system',
      feature: { feature_id: 'taichuy-calendar.settings', label: '太初y 日历计算 / Calendar calculations', description: 'Version-pinned synchronous calendar calculations', route_id: 'taichuy-calendar', path: '/settings/taichuy-calendar', icon: 'CalendarOutlined' },
      operations: operations.map(op => ({ interface_id: operationId(op.name), contribution_id: operationId(op.name), method: 'POST', path: route(op.name), summary: interfaceSummaries[op.name], description: interfaceSummaries[op.name] + '; deterministic bundled npm computation; no runtime Node dependency', execution_concurrency: 'parallel', input_schema: object({ path: object({}), query: object({}), body: op.body }), output_schema: outputSchema(dataSchemas[op.name]) })),
    },
    block_contributions: componentExports.map((name) => ({
      contribution_code: name, title: titles[name], runtime: 'native_react', entry: 'default', ui_registration: 'component',
      code_template: `import { ${name} } from '${moduleSource}';\nexport default function Block() { return <${name} />; }\n`,
      code_template_version: '0.1.13', code_template_language: 'tsx', code_modules: codeModules,
      context_contract: { primitives: [], input_schema: { type: 'object', additionalProperties: false, properties: {} } },
      permissions: { network: 'none', storage: 'none', secrets: 'none' },
      ui_capabilities: ['responsive', 'theming'],
    })),
    keywords: ['calendar', 'almanac', 'lunar', 'ziwei', 'bazi', '日历', '黄历', '紫微', '八字'],
  };
  return emitYaml(manifest) + '\n';
}

export async function writeManifest(sha256, businessModules) {
  await writeFile(resolve(root, 'manifest.yaml'), await manifestText(sha256, businessModules));
}

// Finite manifest writer. Plain safe paths preserve the existing packager's line-based runtime.entry reader.
function scalar(value) {
  if (typeof value !== 'string') return JSON.stringify(value);
  if (/^[a-zA-Z0-9@][a-zA-Z0-9_./@-]*$/.test(value) && !/^\d+$/.test(value)
    && !['true', 'false', 'null'].includes(value)) return value;
  return JSON.stringify(value);
}
function emitYaml(value, indent = 0) {
  const pad = ' '.repeat(indent);
  return Object.entries(value).map(([key, item]) => {
    if (Array.isArray(item)) {
      // Module declarations are repeated verbatim; the host deduplicates source/version identities.
      if (key === 'code_modules' || !item.length || item.every((entry) => typeof entry !== 'object')) {
        return `${pad}${key}: ${JSON.stringify(item)}`;
      }
      return `${pad}${key}:\n` + item.map((entry) => {
        const lines = emitYaml(entry, indent + 4).split('\n');
        lines[0] = `${pad}  - ` + lines[0].slice(indent + 4);
        return lines.join('\n');
      }).join('\n');
    }
    if (item && typeof item === 'object') {
      if (!Object.keys(item).length) return `${pad}${key}: {}`;
      return `${pad}${key}:\n${emitYaml(item, indent + 2)}`;
    }
    return `${pad}${key}: ${scalar(item)}`;
  }).join('\n');
}
