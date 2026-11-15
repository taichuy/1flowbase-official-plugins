#!/usr/bin/env node
/** Real, destructive acceptance only on a Root-owned loopback fixture host.
 * Root provisions its isolated database/process. This runner owns no SQL or server lifecycle.
 * Passwords, API keys, cookies, request bodies and raw responses never enter reports/stdout.
 */
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const CONNECTIONS = '/api/console/managed-services/umami/hosts';
const MCP = '/api/console/mcp';
const EXTENSIONS = '/api/console/settings/extension-center';
const UUID = /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i;
const MAX_RESPONSE_BYTES = 16 * 1024 * 1024;

class FixtureError extends Error {
  constructor(code) { super(code); this.code = code; }
}
function check(condition, code) { if (!condition) throw new FixtureError(code); }
function object(value) { return value !== null && typeof value === 'object' && !Array.isArray(value); }
function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (!object(value)) return value;
  return Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])]));
}
function digest(value) { return crypto.createHash('sha256').update(value).digest('hex'); }
function snapshot(value) { return digest(JSON.stringify(canonical(value))); }
function same(left, right, code) { check(snapshot(left) === snapshot(right), code); }
const sleep = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));

function options(argv) {
  const result = { keepConnection: false };
  const fields = {
    '--host-root': 'hostRoot', '--api-base-url': 'apiBaseUrl', '--package': 'packageFile',
    '--upgrade-package': 'upgradePackage', '--umami-base-url': 'umamiBaseUrl',
    '--umami-username': 'umamiUsername', '--umami-password-file': 'umamiPasswordFile',
    '--root-account': 'rootAccount', '--root-password-file': 'rootPasswordFile', '--report': 'reportFile',
  };
  for (let index = 0; index < argv.length; index++) {
    const key = argv[index];
    if (key === '--help') return { help: true };
    if (key === '--keep-connection') { result.keepConnection = true; continue; }
    check(fields[key] && argv[index + 1] && !argv[index + 1].startsWith('--'), 'invalid_cli_arguments');
    check(result[fields[key]] === undefined, 'duplicate_cli_argument');
    result[fields[key]] = argv[++index];
  }
  for (const key of ['hostRoot', 'apiBaseUrl', 'packageFile', 'umamiBaseUrl', 'umamiUsername', 'umamiPasswordFile']) {
    check(typeof result[key] === 'string' && result[key].length > 0, `missing_${key}`);
  }
  for (const key of ['hostRoot', 'packageFile', 'upgradePackage', 'umamiPasswordFile', 'rootPasswordFile', 'reportFile']) {
    if (result[key]) result[key] = path.resolve(result[key]);
  }
  let api; let umami;
  try { api = new URL(result.apiBaseUrl); umami = new URL(result.umamiBaseUrl); }
  catch { throw new FixtureError('invalid_base_url'); }
  for (const url of [api, umami]) {
    check(['http:', 'https:'].includes(url.protocol) && !url.username && !url.password && !url.search && !url.hash, 'invalid_base_url');
  }
  check(['127.0.0.1', 'localhost', '[::1]'].includes(api.hostname) && api.pathname === '/', 'fixture_host_must_be_loopback_origin');
  result.apiBaseUrl = api.origin;
  result.umamiBaseUrl = umami.href.replace(/\/+$/, '');
  return result;
}
function secretFile(filename) {
  const stat = fs.statSync(filename);
  check(stat.isFile() && stat.size > 0 && stat.size <= 64 * 1024, 'invalid_secret_file');
  const value = fs.readFileSync(filename, 'utf8').replace(/\r?\n$/, '');
  check(value.length > 0 && !/[\r\n\0]/.test(value), 'secret_file_must_contain_one_password_line');
  return value;
}
function packageMetadata(filename) {
  check(fs.statSync(filename).isFile(), 'package_file_missing');
  const tar = args => {
    const result = spawnSync('tar', args, { encoding: 'utf8', maxBuffer: MAX_RESPONSE_BYTES });
    check(!result.error && result.status === 0, 'package_archive_read_failed');
    return result.stdout;
  };
  const entries = tar(['-tzf', filename]).split('\n').filter(Boolean);
  const read = target => {
    const matches = entries.filter(entry => {
      const normalized = entry.replace(/^\.\//, '');
      return normalized === target || normalized.endsWith(`/${target}`);
    });
    check(matches.length === 1, 'package_entry_missing_or_ambiguous');
    return tar(['-xOzf', filename, '--', matches[0]]);
  };
  const manifest = read('manifest.yaml');
  check(/^plugin_id:\s*umami\s*$/m.test(manifest), 'package_must_be_umami');
  const version = manifest.match(/^version:\s*([0-9]+\.[0-9]+\.[0-9]+)\s*$/m)?.[1];
  check(version, 'package_version_missing');
  const operations = JSON.parse(read('_assets/api/operations.json'));
  const instance = JSON.parse(read('_assets/mcp/instances/umami.json'));
  const bundle = JSON.parse(read('_assets/mcp/manifest.json'));
  const getDefinition = bundle.files.find(file => file.kind === 'tool' && file.path.endsWith('/get_website.json'));
  check(getDefinition, 'package_get_website_tool_missing');
  const tool = JSON.parse(read(`_assets/mcp/${getDefinition.path}`));
  const featureLine = manifest.match(/^  feature:\s*(\{[^\n]+\})\s*$/m)?.[1];
  check(featureLine, 'package_managed_feature_missing');
  const feature = JSON.parse(featureLine);
  check(Array.isArray(operations) && operations.length === 202, 'package_upstream_inventory_must_have_202_operations');
  check(new Set(operations.map(operation => operation.operation_id)).size === 202, 'package_duplicate_operation_id');
  check(instance.instance_id === 'umami' && instance.bindings.length === 208, 'package_must_have_208_umami_bindings');
  check(new Set(instance.bindings.map(binding => binding.tool_id)).size === 208, 'package_duplicate_tool_binding');
  check(tool.tool_id === 'plugin.umami.get_website' && bundle.bundle_version === version, 'package_template_identity_mismatch');
  return { filename, version, sha256: digest(fs.readFileSync(filename)), operations, instance, tool, feature };
}

class Client {
  constructor(baseUrl, owner, report) { this.baseUrl = baseUrl; this.owner = owner; this.report = report; this.rpcId = 0; this.apiKey = null; }
  async raw(route, { method = 'GET', body, form, publicToken, anonymous = false } = {}) {
    check(route.startsWith('/api/'), 'invalid_fixture_route');
    const headers = {};
    if (!anonymous && publicToken === undefined) {
      headers.cookie = this.owner.cookie;
      headers['x-csrf-token'] = this.owner.csrfToken;
    }
    if (publicToken !== undefined) {
      if (publicToken) headers.authorization = `Bearer ${publicToken}`;
      headers['mcp-protocol-version'] = '2025-11-25';
    }
    if (body !== undefined) headers['content-type'] = 'application/json';
    let response;
    try {
      response = await fetch(`${this.baseUrl}${route}`, {
        method, headers, body: form ?? (body === undefined ? undefined : JSON.stringify(body)),
        redirect: 'error', signal: AbortSignal.timeout(240_000),
      });
    } catch { throw new FixtureError('fixture_http_transport_failed'); }
    this.report.http_requests++;
    const chunks = []; let length = 0;
    const reader = response.body?.getReader();
    if (reader) {
      try {
        for (;;) {
          const part = await reader.read();
          if (part.done) break;
          length += part.value.byteLength;
          if (length > MAX_RESPONSE_BYTES) { await reader.cancel(); throw new FixtureError('fixture_response_too_large'); }
          chunks.push(Buffer.from(part.value));
        }
      } catch (error) { if (error instanceof FixtureError) throw error; throw new FixtureError('fixture_response_read_failed'); }
    }
    const text = Buffer.concat(chunks).toString('utf8');
    let json = null;
    if (text) { try { json = JSON.parse(text); } catch { throw new FixtureError('fixture_response_not_json'); } }
    if (response.status >= 400) {
      const code = typeof json?.code === 'string' && /^[a-z][a-z0-9_]{0,95}$/.test(json.code) ? json.code : null;
      this.report.http_errors ??= [];
      this.report.http_errors.push({ method, route: route.split('?', 1)[0], status: response.status, code });
    }
    return { status: response.status, json };
  }
  async api(route, request = {}, statuses = [200]) {
    const result = await this.raw(route, request);
    check(statuses.includes(result.status), `console_http_${result.status}`);
    if (result.status === 204) return null;
    check(object(result.json) && Object.hasOwn(result.json, 'data'), 'console_data_envelope_missing');
    return result.json.data;
  }
  async rpc(method, params, token = this.apiKey) {
    const result = await this.raw('/api/mcp/umami', {
      method: 'POST', publicToken: token,
      body: { jsonrpc: '2.0', id: ++this.rpcId, method, params },
    });
    check(result.status === 200, `mcp_http_${result.status}`);
    check(result.json?.jsonrpc === '2.0' && !result.json.error && object(result.json.result), 'mcp_protocol_error');
    return result.json.result;
  }
  async meta(name, args) {
    const result = await this.rpc('tools/call', { name, arguments: args });
    check(result.isError === false && object(result.structuredContent), 'mcp_tool_error');
    return result.structuredContent;
  }
  async detail(toolId) {
    const detail = await this.meta('mcp_get', { tool_id: toolId });
    check(detail.tool_id === toolId && object(detail.input_schema), 'mcp_get_tool_contract_missing');
    return detail;
  }
  async call(toolId, args) {
    const detail = await this.detail(toolId);
    const controls = { tool_id: toolId, arguments: args };
    if (detail.des_id_required) { check(typeof detail.des_id === 'string', 'mcp_description_id_missing'); controls.des_id = detail.des_id; }
    if (detail.call_parameters?.includes('max_inline_chars')) controls.max_inline_chars = 64 * 1024;
    const result = await this.meta('mcp_call', controls);
    check(!result.detail?.result_ref, 'fixture_result_requires_continuation');
    return result;
  }
}

function assertNoSecrets(value, secrets) {
  const visit = item => {
    if (Array.isArray(item)) { for (const child of item) visit(child); return; }
    if (!object(item)) return;
    for (const [key, child] of Object.entries(item)) {
      check(!['password', 'token', 'partialToken', 'credential_id', 'authorization', 'cookie', 'csrf_token', 'api_key'].includes(key), 'public_dto_contains_secret_field');
      visit(child);
    }
  };
  visit(value);
  const text = JSON.stringify(value);
  for (const secret of secrets.filter(secret => typeof secret === 'string' && secret.length > 0)) {
    check(!text.includes(secret), 'public_dto_contains_secret_value');
  }
}
function instanceBody(instance) {
  return Object.fromEntries(['instance_id', 'name', 'description_short', 'status', 'default_entry_path', 'webmcp_exposure'].map(field => [field, instance[field]]));
}
function toolBody(tool) {
  return Object.fromEntries(['name', 'des_id', 'short_description', 'full_description', 'execution_target', 'parameter_schema', 'result_schema', 'input_mapping', 'output_mapping', 'max_inline_chars', 'response_fields', 'permission_code', 'risk_level', 'status'].map(field => [field, tool[field]]));
}

async function awaitTask(client, initial) {
  check(object(initial) && typeof initial.id === 'string', 'extension_task_missing');
  let task = initial;
  for (let attempt = 0; attempt < 90; attempt++) {
    if (task.status === 'succeeded') return;
    check(!['failed', 'cancelled'].includes(task.status), 'extension_activation_task_failed');
    await sleep(500);
    task = await client.api(`/api/console/plugins/tasks/${encodeURIComponent(initial.id)}`);
  }
  throw new FixtureError('extension_activation_task_timeout');
}
async function activate(client, installationId, action = 'enable') {
  const task = await client.api(`${EXTENSIONS}/installed/${installationId}/${action}`, { method: 'POST' }, [200, 201, 202]);
  await awaitTask(client, task);
}
async function upload(client, metadata, report) {
  const bytes = fs.readFileSync(metadata.filename);
  const form = risk => {
    const value = new FormData();
    value.append('file', new Blob([bytes], { type: 'application/gzip' }), path.basename(metadata.filename));
    if (risk) value.append('risk_override', JSON.stringify(risk));
    return value;
  };
  let result = await client.raw(`${EXTENSIONS}/install-upload`, { method: 'POST', form: form() });
  if (result.status === 409 && result.json?.code === 'extension_risk_confirmation_required') {
    const challenge = result.json.risk_challenge;
    check(object(challenge) && !challenge.compatibility && Array.isArray(challenge.warnings), 'package_requires_unsupported_compatibility_override');
    const allowed = new Set(['checksum_missing', 'signature_missing', 'signing_key_unknown']);
    check(challenge.warnings.every(warning => warning.overridable && allowed.has(warning.code)), 'package_requires_unexpected_risk_override');
    const codes = challenge.warnings.map(warning => warning.code).sort();
    report.package_warning_codes = [...new Set([...(report.package_warning_codes ?? []), ...codes])];
    // The explicit --package artifact is the authorized fixture candidate. Acknowledge
    // only checksum/signature warnings, tied to its exact SHA; never bypass compatibility.
    result = await client.raw(`${EXTENSIONS}/install-upload`, { method: 'POST', form: form({ reason: `Explicit Umami acceptance candidate sha256:${metadata.sha256}`, acknowledged_warnings: codes }) });
  }
  check([200, 201].includes(result.status), `extension_upload_http_${result.status}`);
  const installed = result.json?.data;
  check(installed?.installation?.artifact_id === 'umami' && installed.installation.version === metadata.version, 'extension_installation_identity_mismatch');
  const id = installed.node_plugin_installation_id ?? installed.installation.id;
  check(typeof id === 'string' && UUID.test(id), 'extension_installation_id_missing');
  return id;
}

async function catalogEvidence(client, metadata) {
  const interfaces = await client.api(`${MCP}/interface-capabilities`);
  check(Array.isArray(interfaces), 'interface_catalog_not_array');
  const upstream = interfaces.filter(item => item.interface_id.startsWith('umami.api.'));
  const hosts = interfaces.filter(item => item.interface_id.startsWith('umami.hosts.'));
  same(upstream.map(item => item.interface_id).sort(), metadata.operations.map(operation => `umami.api.${operation.operation_id}`).sort(), 'upstream_interface_inventory_mismatch');
  check(hosts.length === 6, 'host_configuration_interface_count_mismatch');
  for (const operation of metadata.operations) {
    check(upstream.find(item => item.interface_id === `umami.api.${operation.operation_id}`)?.method === operation.method, 'upstream_interface_method_mismatch');
  }
  const catalog = await client.api(`${MCP}/catalog`);
  check(Array.isArray(catalog.instances) && Array.isArray(catalog.tools) && Array.isArray(catalog.bindings), 'mcp_catalog_contract_missing');
  const instance = catalog.instances.find(item => item.instance_id === 'umami');
  check(instance?.status === 'enabled', 'umami_mcp_instance_not_enabled');
  const bindings = catalog.bindings.filter(binding => binding.instance_record_id === instance.id);
  same(bindings.map(binding => binding.tool_id).sort(), metadata.instance.bindings.map(binding => binding.tool_id).sort(), 'umami_mcp_binding_inventory_mismatch');
  check(bindings.every(binding => binding.visible), 'umami_mcp_binding_hidden');
  const bound = new Set(bindings.map(binding => binding.tool_id));
  const tools = catalog.tools.filter(tool => bound.has(tool.tool_id));
  check(tools.length === 208 && tools.every(tool => tool.status === 'enabled' && tool.availability_status === 'available'), 'umami_mcp_tools_unavailable');
  return { catalog, instance, counts: { upstream_interfaces: upstream.length, configuration_interfaces: hosts.length, umami_tools: tools.length, umami_bindings: bindings.length } };
}

export async function run(optionsValue) {
  const args = optionsValue;
  const runId = crypto.randomBytes(8).toString('hex');
  const reportFile = args.reportFile ?? path.join(args.hostRoot, 'tmp/test-governance/umami', `integration-${runId}.json`);
  const report = {
    schema_version: 1, run_id: runId, started_at: new Date().toISOString(), status: 'running',
    api_base_url: args.apiBaseUrl, http_requests: 0, assertions: [], cleanup: [],
    upgrade: { status: args.upgradePackage ? 'pending' : 'unverified', reason: args.upgradePackage ? null : 'no_upgrade_package_provided' },
  };
  const state = { owner: null, client: null, hostId: null, hostCreateStarted: false, websiteId: null, websiteCreateStarted: false, websiteDeleted: false, keyId: null, unrelatedId: null, dirtyDefaults: null, failure: null };
  let password; let rootPassword; let wrongPassword; let apiKeySecret; let hostSnapshot; let currentPackage;
  const temporaryWebsite = { name: `1flowbase Umami QA ${runId}`, domain: `umami-qa-${runId}.example.invalid` };
  const stage = async (name, action) => {
    const started = Date.now();
    try {
      const evidence = await action();
      report.assertions.push({ name, status: 'passed', duration_ms: Date.now() - started, ...(evidence === undefined ? {} : { evidence }) });
    } catch (error) {
      report.assertions.push({ name, status: 'failed', duration_ms: Date.now() - started, code: error instanceof FixtureError ? error.code : 'fixture_unexpected_error' });
      throw error;
    }
  };
  const cleanup = async (name, action) => {
    try { await action(); report.cleanup.push({ name, status: 'passed' }); }
    catch (error) { report.cleanup.push({ name, status: 'failed', code: error instanceof FixtureError ? error.code : 'fixture_cleanup_failed' }); }
  };
  try {
    const metadata = packageMetadata(args.packageFile);
    const upgrade = args.upgradePackage ? packageMetadata(args.upgradePackage) : null;
    if (upgrade) check(upgrade.version !== metadata.version, 'upgrade_package_must_use_a_different_version');
    currentPackage = metadata;
    report.package = { version: metadata.version, sha256: metadata.sha256 };
    if (upgrade) report.upgrade.package = { version: upgrade.version, sha256: upgrade.sha256 };
    password = secretFile(args.umamiPasswordFile);
    wrongPassword = `deliberately-wrong-${crypto.randomUUID()}`;
    const require = createRequire(path.join(args.hostRoot, 'package.json'));
    const { loadRootCredentials, openTemporaryOwnerSession } = require(path.join(args.hostRoot, 'scripts/node/page-debug/auth.js'));
    let credentials;
    try {
      credentials = loadRootCredentials({ repoRoot: args.hostRoot, accountOverride: args.rootAccount, passwordOverride: args.rootPasswordFile ? secretFile(args.rootPasswordFile) : undefined });
      rootPassword = credentials.password;
      state.owner = await openTemporaryOwnerSession({ apiBaseUrl: args.apiBaseUrl, account: credentials.account, password: credentials.password, fetchImpl: (url, init) => fetch(url, { ...init, redirect: 'error', signal: AbortSignal.timeout(30_000) }) });
    } catch { throw new FixtureError('temporary_owner_session_failed'); }
    const client = new Client(args.apiBaseUrl, state.owner, report);
    state.client = client;
    const secrets = () => [password, wrongPassword, rootPassword, client.apiKey, state.owner.cookie, state.owner.csrfToken];
    const plugin = async (route, request) => {
      const data = await client.api(route, request);
      assertNoSecrets(data, secrets());
      return data;
    };
    let installationId;
    await stage('extension_center_upload_and_activate', async () => {
      installationId = await upload(client, metadata, report);
      await activate(client, installationId);
      report.installation_id = installationId;
      return { version: metadata.version, installation_id: installationId };
    });
    await stage('settings_feature_and_complete_catalog', async () => {
      const navigation = await client.api('/api/console/navigation');
      check(navigation.route_definitions?.some(route => route.route_id === metadata.feature.route_id && route.path === metadata.feature.path), 'umami_settings_route_missing');
      const page = await client.api(`/api/console/settings/ui-management/plugin-settings-page?route_id=${encodeURIComponent(metadata.feature.route_id)}`);
      check(page.feature_id === metadata.feature.feature_id && page.provider_code === 'umami' && page.source?.length > 0 && page.applied_plugin_version === metadata.version, 'umami_settings_feature_mismatch');
      const evidence = await catalogEvidence(client, metadata);
      report.operation_counts = evidence.counts;
      return evidence.counts;
    });
    const draft = { name: `Umami acceptance ${runId}`, base_url: args.umamiBaseUrl, username: args.umamiUsername, enabled: true };
    await stage('draft_authentication_wrong_password_and_success', async () => {
      const rejected = await plugin(`${CONNECTIONS}/test`, { method: 'POST', body: { ...draft, password: wrongPassword } });
      check(rejected.success === false && rejected.error?.code === 'umami_authentication_failed', 'wrong_password_not_rejected');
      const accepted = await plugin(`${CONNECTIONS}/test`, { method: 'POST', body: { ...draft, password } });
      check(accepted.success === true && accepted.authenticated === true && object(accepted.user), 'correct_password_not_authenticated');
    });
    await stage('saved_connection_crud_and_secret_isolation', async () => {
      state.hostCreateStarted = true;
      const saved = await plugin(CONNECTIONS, { method: 'POST', body: { ...draft, password } });
      check(saved.success === true && typeof saved.host?.host_id === 'string', 'saved_connection_missing');
      state.hostId = saved.host.host_id;
      report.connection = { host_id: state.hostId, retained_by_request: args.keepConnection };
      check(saved.host.has_password === true, 'saved_password_not_present');
      const updated = await plugin(`${CONNECTIONS}/${encodeURIComponent(state.hostId)}`, { method: 'PUT', body: { ...draft, name: `${draft.name} updated` } });
      check(updated.success === true && updated.host.has_password === true, 'metadata_update_lost_saved_password');
      hostSnapshot = updated.host;
      const tested = await plugin(`${CONNECTIONS}/${encodeURIComponent(state.hostId)}/test`, { method: 'POST' });
      check(tested.success === true && tested.authenticated === true, 'saved_connection_test_failed');
      const listed = await plugin(`${CONNECTIONS}?page=1&page_size=100&search=${encodeURIComponent(runId)}`);
      check(listed.success === true && listed.items?.some(host => host.host_id === state.hostId && host.has_password), 'saved_connection_list_missing');
      return { has_password: true, list_count: listed.items.length };
    });
    const upstreamRoot = () => `${CONNECTIONS}/${encodeURIComponent(state.hostId)}`;
    await stage('normal_http_list_websites', async () => {
      const websites = await plugin(`${upstreamRoot()}/api/websites?page=1&pageSize=10`);
      check(websites.success === true && websites.status === 200 && Array.isArray(websites.data?.data), 'http_list_websites_failed');
      return { status: websites.status, returned_items: websites.data.data.length };
    });
    await stage('public_mcp_uses_temporary_user_api_key', async () => {
      const anonymous = await client.raw('/api/mcp/umami', { method: 'POST', publicToken: '', body: { jsonrpc: '2.0', id: 1, method: 'tools/list', params: {} } });
      check(anonymous.status === 401, 'public_mcp_did_not_require_authentication');
      const created = await client.api('/api/console/user-api-keys', { method: 'POST', body: { name: `Umami acceptance ${runId}`, expiration_policy: '30d' } }, [201]);
      check(typeof created.id === 'string' && typeof created.token === 'string' && created.token.length > 0, 'temporary_user_api_key_missing');
      state.keyId = created.id; client.apiKey = created.token; apiKeySecret = created.token;
      await client.rpc('initialize', { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: '1flowbase-umami-acceptance', version: '0.1.0' } });
      const tools = await client.rpc('tools/list', {});
      check(['mcp_list', 'mcp_get', 'mcp_call'].every(name => tools.tools?.some(tool => tool.name === name)), 'public_mcp_meta_tools_missing');
      return { authentication: 'server_issued_temporary_user_api_key', anonymous_status: anonymous.status };
    });
    await stage('public_mcp_discovery_get_and_original_field_names', async () => {
      const directory = await client.meta('mcp_list', { path: '/websites', depth: 1, limit: 100 });
      check(Array.isArray(directory.result) && directory.result.length > 0, 'public_mcp_directory_empty');
      const detail = await client.detail('plugin.umami.get_website');
      check(detail.input_schema.properties?.websiteId && detail.input_schema.properties?.host_id && !detail.input_schema.properties?.website_id, 'public_mcp_original_field_contract_changed');
      return { directory_items: directory.result.length, tool_id: detail.tool_id };
    });
    await stage('public_mcp_temporary_website_create_read_update_delete', async () => {
      state.websiteCreateStarted = true;
      const created = await client.call('plugin.umami.create_website', { host_id: state.hostId, ...temporaryWebsite });
      check(created.success === true && UUID.test(created.data?.id ?? ''), 'mcp_create_website_failed');
      state.websiteId = created.data.id;
      check(created.data.domain === temporaryWebsite.domain && created.data.name === temporaryWebsite.name, 'mcp_created_website_identity_mismatch');
      const read = await client.call('plugin.umami.get_website', { host_id: state.hostId, websiteId: state.websiteId });
      check(read.success === true && read.data?.id === state.websiteId && read.data.name === temporaryWebsite.name, 'mcp_read_website_failed');
      const name = `${temporaryWebsite.name} updated`;
      const updated = await client.call('plugin.umami.update_website', { host_id: state.hostId, websiteId: state.websiteId, name });
      check(updated.success === true && updated.data?.name === name, 'mcp_update_website_failed');
      const verified = await client.call('plugin.umami.get_website', { host_id: state.hostId, websiteId: state.websiteId });
      check(verified.success === true && verified.data?.name === name && verified.data.domain === temporaryWebsite.domain, 'mcp_update_not_persisted');
      const deleted = await client.call('plugin.umami.delete_website', { host_id: state.hostId, websiteId: state.websiteId });
      check(deleted.success === true && deleted.status === 200, 'mcp_delete_website_failed');
      state.websiteDeleted = true;
      const listed = await plugin(`${upstreamRoot()}/api/websites?search=${encodeURIComponent(temporaryWebsite.domain)}&pageSize=100`);
      check(listed.success === true && !listed.data?.data?.some(website => website.id === state.websiteId), 'mcp_deleted_website_still_listed');
      return { website_id: state.websiteId, temporary_domain: temporaryWebsite.domain, deleted: true };
    });
    let unrelatedSnapshot;
    await stage('unrelated_instance_owned_fixture', async () => {
      state.unrelatedId = `umami_acceptance_${runId}`;
      unrelatedSnapshot = await client.api(`${MCP}/instances`, { method: 'POST', body: { instance_id: state.unrelatedId, name: `Unrelated acceptance ${runId}`, description_short: 'Must survive Umami-only template reset', status: 'enabled', default_entry_path: '/', webmcp_exposure: 'disabled' } }, [201]);
      check(unrelatedSnapshot.instance_id === state.unrelatedId, 'unrelated_instance_creation_failed');
      return { instance_id: state.unrelatedId, baseline_sha256: snapshot(unrelatedSnapshot) };
    });
    const reset = async (metadataValue, resetAction) => {
      const before = await catalogEvidence(client, currentPackage);
      const tool = before.catalog.tools.find(tool => tool.tool_id === 'plugin.umami.get_website');
      check(tool, 'mutable_template_tool_missing');
      state.dirtyDefaults = { instance: before.instance, tool };
      await client.api(`${MCP}/instances/umami`, { method: 'PUT', body: { ...instanceBody(before.instance), description_short: `Edited acceptance ${runId}` } });
      await client.api(`${MCP}/tools/${encodeURIComponent(tool.tool_id)}`, { method: 'PUT', body: { ...toolBody(tool), short_description: `Edited acceptance ${runId}`, full_description: `Temporary edited description ${runId}` } });
      const changed = await client.api(`${MCP}/catalog`);
      check(changed.instances.find(instance => instance.instance_id === 'umami')?.description_short === `Edited acceptance ${runId}` && changed.tools.find(candidate => candidate.tool_id === tool.tool_id)?.short_description === `Edited acceptance ${runId}`, 'template_mutation_not_persisted');
      await resetAction();
      const after = await catalogEvidence(client, metadataValue);
      const resetTool = after.catalog.tools.find(candidate => candidate.tool_id === tool.tool_id);
      check(after.instance.description_short === metadataValue.instance.description_short && resetTool?.short_description === metadataValue.tool.short_description && resetTool.full_description === metadataValue.tool.full_description, 'activation_did_not_restore_package_template_defaults');
      same(after.catalog.instances.find(instance => instance.instance_id === state.unrelatedId), unrelatedSnapshot, 'activation_modified_unrelated_instance');
      const listed = await plugin(`${CONNECTIONS}?page=1&page_size=100&search=${encodeURIComponent(runId)}`);
      same(listed.items?.find(host => host.host_id === state.hostId), hostSnapshot, 'activation_changed_saved_connection');
      const tested = await plugin(`${CONNECTIONS}/${encodeURIComponent(state.hostId)}/test`, { method: 'POST' });
      check(tested.success === true && tested.authenticated === true, 'activation_lost_saved_password');
      const mcp = await client.call('plugin.umami.list_websites', { host_id: state.hostId, pageSize: 1 });
      check(mcp.success === true && mcp.status === 200 && Array.isArray(mcp.data?.data), 'activation_broke_public_mcp_connection');
      state.dirtyDefaults = null; currentPackage = metadataValue;
      return { ...after.counts, version: metadataValue.version, host_id_preserved: true, has_password_preserved: true, unrelated_instance_sha256: snapshot(unrelatedSnapshot), package_defaults_restored: true };
    };
    await stage('reactivation_forces_owned_template_reset', () => reset(metadata, async () => {
      await activate(client, installationId, 'disable');
      await activate(client, installationId, 'enable');
    }));
    if (upgrade) {
      await stage('actual_version_upgrade_forces_owned_template_reset', async () => {
        const evidence = await reset(upgrade, async () => {
          installationId = await upload(client, upgrade, report);
          await activate(client, installationId);
        });
        const page = await client.api(`/api/console/settings/ui-management/plugin-settings-page?route_id=${encodeURIComponent(upgrade.feature.route_id)}`);
        check(page.applied_plugin_version === upgrade.version, 'upgrade_version_not_applied');
        report.upgrade.status = 'passed'; report.active_version = upgrade.version;
        return evidence;
      });
    } else report.active_version = metadata.version;
    report.status = 'passed';
  } catch (error) {
    report.status = 'failed';
    state.failure = error instanceof FixtureError ? error.code : 'fixture_unexpected_error';
    report.failure_code = state.failure;
  } finally {
    const client = state.client;
    if (client) {
      if (state.websiteCreateStarted && !state.websiteDeleted && state.hostId) await cleanup('temporary_website', async () => {
        let ids = state.websiteId ? [state.websiteId] : [];
        if (!ids.length) {
          // A lost create response never permits replaying create. Discover only our
          // exact unique domain/name, then delete that fixture through the formal HTTP API.
          const listed = await client.api(`${CONNECTIONS}/${encodeURIComponent(state.hostId)}/api/websites?search=${encodeURIComponent(temporaryWebsite.domain)}&pageSize=100`);
          check(listed.success === true && Array.isArray(listed.data?.data), 'website_cleanup_discovery_failed');
          ids = listed.data.data.filter(website => website.domain === temporaryWebsite.domain && [temporaryWebsite.name, `${temporaryWebsite.name} updated`].includes(website.name)).map(website => website.id);
        }
        for (const id of ids) {
          check(UUID.test(id), 'website_cleanup_id_invalid');
          const deleted = await client.api(`${CONNECTIONS}/${encodeURIComponent(state.hostId)}/api/websites/${encodeURIComponent(id)}`, { method: 'DELETE' });
          check(deleted.success === true, 'website_cleanup_delete_failed');
        }
      });
      if (state.dirtyDefaults) await cleanup('restore_edited_umami_template_after_failure', async () => {
        await client.api(`${MCP}/instances/umami`, { method: 'PUT', body: instanceBody(state.dirtyDefaults.instance) });
        await client.api(`${MCP}/tools/${encodeURIComponent(state.dirtyDefaults.tool.tool_id)}`, { method: 'PUT', body: toolBody(state.dirtyDefaults.tool) });
      });
      if (state.unrelatedId) await cleanup('unrelated_owned_instance', () => client.api(`${MCP}/instances/${encodeURIComponent(state.unrelatedId)}`, { method: 'DELETE' }, [204]));
      if (state.hostCreateStarted && !state.hostId && !args.keepConnection) await cleanup('discover_saved_connection_after_lost_create_response', async () => {
        const listed = await client.api(`${CONNECTIONS}?page=1&page_size=100&search=${encodeURIComponent(runId)}`);
        check(listed.success === true && Array.isArray(listed.items), 'connection_cleanup_discovery_failed');
        const owned = listed.items.filter(host => [`Umami acceptance ${runId}`, `Umami acceptance ${runId} updated`].includes(host.name)
          && host.username === args.umamiUsername && host.base_url === args.umamiBaseUrl);
        check(owned.length <= 1, 'connection_cleanup_identity_ambiguous');
        state.hostId = owned[0]?.host_id ?? null;
      });
      if (state.hostId && !args.keepConnection) await cleanup('saved_connection', async () => {
        const deleted = await client.api(`${CONNECTIONS}/${encodeURIComponent(state.hostId)}`, { method: 'DELETE' });
        check(deleted.success === true, 'connection_cleanup_delete_failed');
        const listed = await client.api(`${CONNECTIONS}?page=1&page_size=100&search=${encodeURIComponent(runId)}`);
        check(listed.success === true && !listed.items?.some(host => host.host_id === state.hostId), 'connection_cleanup_record_remains');
      });
      if (state.hostId && args.keepConnection) report.cleanup.push({ name: 'saved_connection', status: 'retained_by_request', host_id: state.hostId });
      if (state.keyId) await cleanup('temporary_user_api_key', () => client.api(`/api/console/user-api-keys/${encodeURIComponent(state.keyId)}/revoke`, { method: 'POST' }));
      client.apiKey = null;
    }
    if (state.owner) await cleanup('temporary_owner_session', () => state.owner.dispose());
    if (report.cleanup.some(item => item.status === 'failed')) report.status = 'failed';
    if (report.upgrade.status === 'pending') report.upgrade = { ...report.upgrade, status: 'not_run', reason: 'earlier_stage_failed' };
    report.finished_at = new Date().toISOString();
    // Report fields are deliberately constructed from assertion labels/counts/fixture IDs.
    // A final value check also prevents accidental future introduction of known secrets.
    const serialized = JSON.stringify(report, null, 2);
    const known = [password, wrongPassword, rootPassword, apiKeySecret, state.owner?.cookie, state.owner?.csrfToken].filter(value => typeof value === 'string' && value.length > 0);
    check(known.every(secret => !serialized.includes(secret)), 'report_secret_redaction_failed');
    fs.mkdirSync(path.dirname(reportFile), { recursive: true });
    fs.writeFileSync(reportFile, `${serialized}\n`, { mode: 0o600 });
  }
  return { passed: report.status === 'passed', reportFile, passed_assertions: report.assertions.filter(item => item.status === 'passed').length, failed_assertions: report.assertions.filter(item => item.status === 'failed').length, cleanup_failures: report.cleanup.filter(item => item.status === 'failed').length };
}

const help = `Usage: node integration.mjs --host-root HOST_REPO --api-base-url http://127.0.0.1:7802
  --package CANDIDATE.1flowbasepkg --umami-base-url https://umami.example
  --umami-username USER --umami-password-file PRIVATE_PASSWORD_FILE
  [--upgrade-package NEXT_VERSION.1flowbasepkg] [--report REDACTED_REPORT.json]
  [--root-account ACCOUNT] [--root-password-file PRIVATE_ROOT_PASSWORD_FILE] [--keep-connection]

Requires a Root-owned isolated loopback API/database and an account allowed to create temporary Umami websites.
Uses page-debug/auth.js temporary sessions; creates/revokes a dedicated user API key.
Only unique fixture websites/connections/instances are deleted. The installed Umami package stays active.
Without --upgrade-package, reactivation is verified and version upgrade is explicitly unverified.
Passwords must be one plain text line. Credentials, raw bodies and authentication headers are never printed.
`;

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = options(process.argv.slice(2));
    if (args.help) process.stdout.write(help);
    else {
      const result = await run(args);
      process.stdout.write(`${JSON.stringify(result)}\n`);
      if (!result.passed) process.exitCode = 1;
    }
  } catch (error) {
    process.stderr.write(`${JSON.stringify({ passed: false, code: error instanceof FixtureError ? error.code : 'fixture_setup_failed' })}\n`);
    process.exitCode = 1;
  }
}
