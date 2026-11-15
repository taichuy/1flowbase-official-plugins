import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import schemaCodec from './schema.cjs';

export const pluginRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const version = '0.1.0';
const object = (properties = {}, required = [], extra = {}) => ({ type: 'object', properties, required, additionalProperties: false, ...extra });
const text = description => ({ type: 'string', minLength: 1, ...(description ? { description } : {}) });
const clone = value => structuredClone(value);
const secret = { type: 'string', writeOnly: true, format: 'password', description: 'Write-only credential. Omit when editing to retain the saved password.' };
const hostProperties = { host_id: text('Saved Umami connection ID. Obtain it from hosts_list.'), name: text(), base_url: { type: 'string', format: 'uri' }, username: text(), enabled: { type: 'boolean' }, has_password: { type: 'boolean' } };
const hostSummary = object(hostProperties, Object.keys(hostProperties));
const hostInput = { name: text(), base_url: text('Umami origin, optionally including its deployment base path.'), username: text(), enabled: { type: 'boolean' }, password: secret };
const failure = object({ code: text() }, ['code']);
const envelope = (properties, required = []) => object({ success: { type: 'boolean' }, ...properties, error: failure }, ['success', ...required]);
const input = (paths = {}, query = object(), body = object()) => object({ path: object(paths, Object.keys(paths)), query, body }, ['path', 'query', 'body']);

const resolveRefs = schemaCodec.resolveSchemaReferences;

function requestLocations(contract) {
  const operation = contract.operation;
  const locations = { path: object(), query: object() };
  for (const location of ['path', 'query']) if (operation.requestParams?.[location]) locations[location] = resolveRefs(operation.requestParams[location]);
  for (const parameter of operation.parameters ?? []) {
    if (!['path', 'query'].includes(parameter.in)) continue;
    const location = locations[parameter.in];
    location.properties ??= {};
    location.required ??= [];
    location.properties[parameter.name] = { ...resolveRefs(parameter.schema ?? {}), ...(parameter.description ? { description: parameter.description } : {}) };
    if (parameter.required || parameter.in === 'path') location.required.push(parameter.name);
  }
  for (const [, name] of contract.path.matchAll(/\{([^}]+)\}/g)) {
    locations.path.properties ??= {};
    locations.path.properties[name] ??= text();
    locations.path.required = [...new Set([...(locations.path.required ?? []), name])];
  }
  locations.path.properties.host_id = hostProperties.host_id;
  locations.path.required = [...new Set([...(locations.path.required ?? []), 'host_id'])];
  const content = operation.requestBody?.content;
  const media = content?.['application/json'] ?? (content ? Object.values(content)[0] : undefined);
  locations.body = media?.schema ? resolveRefs(media.schema) : object();
  // Authentication belongs to the saved connection. Callers never submit or receive its token.
  if (contract.path === '/api/auth/login') locations.body = object();
  if (/^\/api\/(?:auth|2fa|users)(?:\/|$)|^\/api\/me\/(?:password|api-keys)/.test(contract.path)) {
    markSecrets(locations.body, ['password', 'currentPassword', 'newPassword', 'token', 'partialToken', 'backupCode', 'code']);
  }
  return locations;
}

function markSecrets(schema, names) {
  if (!schema || typeof schema !== 'object') return;
  for (const [name, child] of Object.entries(schema.properties ?? {})) {
    if (names.includes(name)) Object.assign(child, { writeOnly: true, secret: true });
  }
  for (const keyword of ['allOf', 'anyOf', 'oneOf']) for (const branch of schema[keyword] ?? []) markSecrets(branch, names);
}

function mcpArguments(locations, method) {
  const fields = {}, required = [], mappings = [], descriptors = [];
  function add(name, schema, location, isRequired) {
    if (fields[name]) throw new Error(`Ambiguous parameter ${name}`);
    fields[name] = clone(schema);
    if (isRequired) required.push(name);
    mappings.push({ interface_param: name, mcp_param: name, source: { kind: 'mcp_argument', path: name }, required: isRequired, description: schema.description ?? `Original Umami ${location} parameter ${name}.` });
    descriptors.push({ name, field_type: schema.type ?? 'object', parameter_type: location === 'body' ? 'json_body' : 'url', description: schema.description ?? '', required: isRequired, schema: clone(schema) });
  }
  for (const location of ['path', 'query']) for (const [name, schema] of Object.entries(locations[location].properties ?? {})) add(name, schema, location, location === 'path' || (locations[location].required ?? []).includes(name));
  if (!['GET', 'DELETE', 'HEAD'].includes(method)) {
    const body = locations.body;
    if (body.properties && Object.keys(body.properties).length) {
      for (const [name, schema] of Object.entries(body.properties)) add(name, schema, 'body', (body.required ?? []).includes(name));
    } else if (!body.properties || Object.keys(body).some(key => ['oneOf', 'anyOf', 'allOf', 'items'].includes(key))) {
      add('body', body, 'body', true);
    }
  }
  return { parameter_schema_snapshot: object(fields, required), input_mapping: { interface_parameters: descriptors, mappings } };
}

function upstreamResult(contract) {
  const response = Object.entries(contract.operation.responses ?? {}).find(([status]) => /^2\d\d$/.test(status))?.[1];
  const schema = response?.content?.['application/json']?.schema;
  // Responses are passed through in full. The snapshot describes the success
  // payload; upstream failures and text endpoints have their own transport shape.
  let data = schema ? resolveRefs(schema) : {};
  if (contract.path === '/api/auth/login') data = object({ authenticated: { const: true }, user: { type: 'object' } }, ['authenticated', 'user']);
  if (['/api/auth/sso', '/api/2fa/verify'].includes(contract.path)) data = { type: 'object' };
  if (/\/me\/api-keys$/.test(contract.path) && contract.method === 'post') markSecrets(data, ['key', 'apiKey', 'token']);
  const sensitiveResult = /^\/api\/me\/api-keys/.test(contract.path) || /^\/api\/2fa\//.test(contract.path);
  return { ...envelope({ status: { type: 'integer', minimum: 100, maximum: 599 }, data: sensitiveResult ? { secret: true } : {}, content_type: { type: 'string' }, encoding: { const: 'base64' } }, []), 'x-umami-success-schema': data };
}

function configurationOperations() {
  const authResult = envelope({ authenticated: { const: true }, user: { type: 'object' } });
  return [
    { id: 'hosts.list', method: 'GET', path: '/hosts', summary: 'List Umami connections', description: 'List shared saved Umami connections. Passwords and session tokens are never returned.', input_schema: input({}, object({ page: { type: 'integer', minimum: 1 }, page_size: { type: 'integer', minimum: 1, maximum: 100 }, search: { type: 'string' } })), output_schema: envelope({ items: { type: 'array', items: hostSummary }, total: { type: 'integer' }, page: { type: 'integer' }, page_size: { type: 'integer' } }) },
    { id: 'hosts.create', method: 'POST', path: '/hosts', summary: 'Create Umami connection', description: 'Authenticate and save a shared Umami connection. The password is stored through the encrypted Host credential service.', input_schema: input({}, object(), object(hostInput, ['name', 'base_url', 'username', 'enabled', 'password'])), output_schema: envelope({ host: hostSummary }) },
    { id: 'hosts.update', method: 'PUT', path: '/hosts/{host_id}', summary: 'Update Umami connection', description: 'Update a shared connection. Omit password to retain it. Changed connection details are authenticated before saving.', input_schema: input({ host_id: hostProperties.host_id }, object(), object(hostInput, ['name', 'base_url', 'username', 'enabled'])), output_schema: envelope({ host: hostSummary }) },
    { id: 'hosts.delete', method: 'DELETE', path: '/hosts/{host_id}', summary: 'Delete Umami connection', description: 'Disable and remove a shared connection, its encrypted credential and ephemeral session.', input_schema: input({ host_id: hostProperties.host_id }), output_schema: envelope({}) },
    { id: 'hosts.test_draft', method: 'POST', path: '/hosts/test', summary: 'Test Umami connection draft', description: 'Authenticate a draft without saving the connection or password. An existing host_id may supply the saved password.', input_schema: input({}, object(), object({ ...hostInput, host_id: hostProperties.host_id }, ['name', 'base_url', 'username', 'enabled'])), output_schema: authResult },
    { id: 'hosts.test', method: 'POST', path: '/hosts/{host_id}/test', summary: 'Test saved Umami connection', description: 'Authenticate the saved enabled connection and refresh its ephemeral session.', input_schema: input({ host_id: hostProperties.host_id }), output_schema: authResult },
  ].map(({ id, ...op }) => ({ ...op, operation_id: id, group: 'connections', handler: id, interface_id: `umami.${id}`, contribution_id: `umami.${id}`, path: `/api/console/managed-services/umami${op.path}` }));
}

export function generate(root = pluginRoot, { check = false } = {}) {
  const source = JSON.parse(fs.readFileSync(path.join(root, '_assets/api/upstream-contracts.json'), 'utf8'));
  const files = new Map();
  const write = (file, data, compact = false) => files.set(file, `${JSON.stringify(data, null, compact ? undefined : 2)}\n`);
  const pageSource = ['api.ts', 'copy.ts', 'page.tsx'].map(file => fs.readFileSync(path.join(root, 'ui', file), 'utf8').replace(/^import .* from '\.\/(?:api|copy)';\r?\n/gm, '').replace(/^export (?!default)/gm, '')).join('\n');
  files.set('ui/settings.tsx', `// Generated by scripts/generate.mjs from ui/api.ts, ui/copy.ts and ui/page.tsx.\n${pageSource.trimEnd()}\n`);
  const operations = source.contracts.map(contract => {
    const method = contract.method.toUpperCase();
    const locations = requestLocations(contract);
    return {
      operation_id: contract.operation.operationId, handler: `upstream.${contract.operation.operationId}`,
      interface_id: `umami.api.${contract.operation.operationId}`, contribution_id: `umami.api.${contract.operation.operationId}`,
      method, path: `/api/console/managed-services/umami/hosts/{host_id}${contract.path}`,
      summary: contract.operation.summary || `${method} ${contract.path}`,
      description: `${contract.operation.description || contract.operation.summary || `${method} ${contract.path}`} Select a saved Umami host_id. Original upstream: ${method} ${contract.path}.${contract.compatibility ? ' Historical compatibility API.' : ''}`,
      group: contract.compatibility ? 'legacy-reports' : contract.path.split('/')[2],
      input_schema: object(locations, ['path', 'query', 'body']), output_schema: upstreamResult(contract),
    };
  });
  const registrations = [...configurationOperations(), ...operations];
  write('_assets/api/operations.json', source.contracts.map(c => ({ operation_id: c.operation.operationId, method: c.method.toUpperCase(), path: c.path, auth: c.auth })));
  const groups = [...new Set(registrations.map(op => op.group))].sort();
  const bindings = [];
  registrations.forEach((op, index) => {
    const name = op.operation_id.replace(/([a-z0-9])([A-Z])/g, '$1_$2').replaceAll('.', '_').toLowerCase();
    const tool = {
      tool_id: `plugin.umami.${name}`, name, short_description: op.summary, full_description: op.description,
      execution_target: { kind: 'interface_wrapper', interface_id: op.interface_id },
      ...mcpArguments(op.input_schema.properties, op.method), result_schema_snapshot: op.output_schema,
      output_mapping: {}, max_inline_chars: null, response_fields: null, permission_code_snapshot: null,
      risk_level_snapshot: op.method === 'GET' ? 'low' : op.method === 'DELETE' || /reset|password|users|2fa/.test(op.path) ? 'high' : 'medium', status: 'enabled',
    };
    // Bucket by group and tens, keeping each generated directory below 15 files.
    const withinGroup = registrations.slice(0, index).filter(item => item.group === op.group).length;
    const file = `tools/${Math.floor(groups.indexOf(op.group) / 10)}/${op.group}/${Math.floor(withinGroup / 10)}/${name}.json`;
    write(`_assets/mcp/${file}`, tool);
    bindings.push({ group_path: `/${op.group}`, tool_id: tool.tool_id, display_alias: null, visible: true, sort_order: withinGroup });
  });
  write('_assets/mcp/instances/umami.json', {
    instance_id: 'umami', name: 'Umami', description_short: 'Complete self-hosted Umami API. Select a shared connection, then manage analytics and configuration.', status: 'enabled', default_entry_path: '/',
    groups: [{ path: '/', display_name: 'Umami', description_short: 'Connections and complete Umami API', enabled: true, sort_order: 0 }, ...groups.map((group, index) => ({ path: `/${group}`, display_name: group, description_short: group === 'connections' ? 'Configure shared Umami connections' : `Umami ${group} operations`, enabled: true, sort_order: index + 1 }))], bindings,
    discovery_policy: { list_default_limit: 20, list_max_depth: 3, list_regex_enabled: true, list_regex_max_length: 256, list_return_fields: ['id', 'name', 'short_description'] },
  });
  const hash = value => `sha256:${createHash('sha256').update(value).digest('hex')}`;
  write('_assets/mcp/manifest.json', { schema_version: '1flowbase.mcp.bundle/v2', organization: 'taichuy', bundle_id: 'umami', bundle_version: version, locale: 'zh_Hans', minimum_host_version: '0.6.3', exported_from_system_version: '0.6.3', exported_at: '2026-11-15T00:00:00Z', files: [...files].filter(([file]) => file.startsWith('_assets/mcp/')).map(([file, bytes]) => ({ path: file.slice('_assets/mcp/'.length), kind: file.includes('/tools/') ? 'tool' : 'instance', sha256: hash(bytes) })) });
  const permissions = ['service.execute', 'plugin_data.owned.write', 'credential.manage', 'ephemeral.manage', 'network.http.request'];
  const manifest = {
    manifest_version: 2, schema_version: '1flowbase.plugin.manifest/v2', plugin_id: 'umami', version, publisher_namespace: 'taichuy', vendor: 'Taichuy', display_name: 'Umami', description: 'Shared Umami connections, automatic authentication and the complete self-hosted API with MCP.', source_kind: 'official_registry', trust_level: 'verified_official', consumption_kind: 'runtime_extension', execution_mode: 'process_per_call', slot_codes: [], binding_targets: ['system'], selection_mode: 'assignment_then_select', minimum_host_version: '0.6.3', contract_version: '1flowbase.extension-bus/v1',
    permissions: { network: 'outbound_only', secrets: 'host_managed', storage: 'host_managed', mcp: 'none', subprocess: 'deny' },
    runtime: { protocol: 'stdio_json_multiplex_v1', entry: 'bin/umami', capabilities: ['runtime_host_call/v1'], limits: {} },
    managed: { module: { bus_version: 'v1', module_id: 'umami', module_version: version, module_kind: 'runtime', contributions: registrations.map(op => ({ contribution_id: op.contribution_id, contributor_module_id: 'umami', point_id: '1flowbase.managed-service.operation', contract_version: '1', required_permissions: permissions, mode: 'append' })) }, execution_bindings: registrations.map(op => ({ contribution_id: op.contribution_id, execution_mode: 'process_per_call', runtime: { protocol: 'stdio_json_multiplex_v1', entry: 'bin/umami', capabilities: ['runtime_host_call/v1'] }, handler: op.handler })) },
    data_models: [{ contribution_version: '1flowbase.plugin-data-model/v1', storage_binding: 'main', owned_collections: [{ collection_code: 'umami_hosts', fields: ['host_id', 'name', 'base_url', 'username', 'enabled', 'credential_id'].map(field => ({ field_code: field, field_type: field === 'enabled' ? 'boolean' : 'string', nullable: false })) }] }],
    managed_service: { scope: 'system', worker_lifetime: 'actor_scope', mcp_instance: { instance_id: 'umami', name: 'Umami', description: 'Complete Umami management and analytics through shared saved connections.', template: { path: '_assets/mcp/manifest.json', sha256: hash(files.get('_assets/mcp/manifest.json')) }, reset_on_activation: true }, feature: { feature_id: 'umami.management', label: 'Umami', description: 'Manage shared Umami connections', route_id: 'umami', path: '/settings/umami', icon: 'bar-chart' }, operations: registrations.map(({ operation_id, group, handler, ...op }) => ({ ...op, execution_concurrency: operation_id.startsWith('hosts.') ? 'serialized' : 'parallel' })) },
    settings_pages: [{ feature_id: 'umami.management', contribution_code: 'umami_settings', source_file: 'ui/settings.tsx', language: 'tsx' }], keywords: ['umami', 'analytics', 'websites', 'mcp'],
  };
  manifest.managed.module.contributions.push({ contribution_id: 'umami.data', contributor_module_id: 'umami', point_id: '1flowbase.plugin-data.owned-collection', contract_version: '1flowbase.plugin-data-model/v1', required_permissions: ['plugin_data.owned.write'], mode: 'append' });
  manifest.managed.execution_bindings.push({ contribution_id: 'umami.data', execution_mode: 'declarative_only', runtime: { protocol: 'stdio_json', entry: 'manifest.yaml' }, handler: 'data', payload: { kind: 'data_models' } });
  // Keep the runtime section in conventional YAML for the existing packager.
  // Managed bindings are a supported JSON-valued YAML field; operation records
  // remain one per line so generated manifests stay reviewable and bounded.
  const scalar = value => typeof value === 'string' && /^[A-Za-z0-9_./@-]+$/.test(value) ? value : JSON.stringify(value);
  const manifestLines = [];
  for (const [name, value] of Object.entries(manifest)) {
    if (name === 'runtime') {
      manifestLines.push('runtime:', ...Object.entries(value).map(([key, item]) => `  ${key}: ${scalar(item)}`));
    } else if (name === 'managed_service') {
      manifestLines.push('managed_service:');
      for (const [key, item] of Object.entries(value)) {
        if (key === 'operations') manifestLines.push('  operations:', ...item.map(op => `    - ${JSON.stringify(op)}`));
        else manifestLines.push(`  ${key}: ${scalar(item)}`);
      }
    } else manifestLines.push(`${name}: ${scalar(value)}`);
  }
  files.set('manifest.yaml', `${manifestLines.join('\n')}\n`);
  // Remove obsolete generated MCP files after a group or operation changes.
  if (!check) {
    const mcpRoot = path.join(root, '_assets/mcp');
    if (fs.existsSync(mcpRoot)) fs.rmSync(mcpRoot, { recursive: true });
  }
  for (const [file, bytes] of files) {
    const target = path.join(root, file);
    if (check) { if (!fs.existsSync(target) || fs.readFileSync(target, 'utf8') !== bytes) throw new Error(`Generated file differs: ${file}`); }
    else { fs.mkdirSync(path.dirname(target), { recursive: true }); fs.writeFileSync(target, bytes); }
  }
  return { upstream_operations: operations.length, registered_operations: registrations.length, mcp_tools: bindings.length, files: files.size };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) console.log(JSON.stringify(generate(pluginRoot, { check: process.argv.includes('--check') })));
