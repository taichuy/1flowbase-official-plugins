import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import test from 'node:test';
import { generate, pluginRoot } from '../generate.mjs';

const json = file => JSON.parse(fs.readFileSync(path.join(pluginRoot, file), 'utf8'));
const source = json('_assets/api/upstream-contracts.json');
const lines = fs.readFileSync(path.join(pluginRoot, 'manifest.yaml'), 'utf8').split('\n');
const operations = lines.filter(line => line.startsWith('    - {')).map(line => JSON.parse(line.slice(6)));
const managed = JSON.parse(lines.find(line => line.startsWith('managed: ')).slice('managed: '.length));
const bundle = json('_assets/mcp/manifest.json');
const toolFiles = bundle.files.filter(file => file.kind === 'tool');
const tools = toolFiles.map(file => json(`_assets/mcp/${file.path}`));
const key = operation => `${operation.method.toUpperCase()} ${operation.path}`;

test('runtime MCP assets avoid source-only directories pruned by package installation', () => {
  for (const file of bundle.files) {
    const directories = file.path.split('/').slice(0, -1);
    assert.ok(!directories.some(name => name === 'scripts' || name === 'demo'), `Runtime asset would be omitted by the Host installer: ${file.path}`);
  }
});

test('all canonical and legacy routes have registered operations, execution bindings and MCP tools', () => {
  assert.equal(source.provenance.canonical_operations, 186);
  assert.equal(source.provenance.compatibility_operations, 16);
  const inventory = json('_assets/api/operations.json');
  const expected = source.routes.map(key).sort();
  assert.deepEqual(inventory.map(key).sort(), expected);
  assert.equal(new Set(expected).size, 202);
  const ids = new Set(operations.map(op => op.interface_id));
  assert.equal(operations.length, 208);
  assert.equal(ids.size, 208);
  const dataBindings = managed.execution_bindings.filter(binding => binding.payload?.kind === 'data_models');
  assert.equal(dataBindings.length, 1, 'Owned collections require one explicit declarative payload binding');
  assert.equal(dataBindings[0].execution_mode, 'declarative_only');
  assert.ok(managed.module.contributions.some(contribution => contribution.contribution_id === dataBindings[0].contribution_id && contribution.point_id === '1flowbase.plugin-data.owned-collection'));
  const canonicalFields = new Set(['interface_id', 'contribution_id', 'method', 'path', 'summary', 'description', 'input_schema', 'output_schema', 'execution_concurrency']);
  for (const operation of operations) for (const field of Object.keys(operation)) assert.ok(canonicalFields.has(field), `Unknown ManagedServiceOperation field: ${field}`);
  for (const upstream of inventory) {
    const id = `umami.api.${upstream.operation_id}`;
    const operation = operations.find(op => op.interface_id === id);
    assert.ok(operation, id);
    assert.equal(key(operation), `${upstream.method} /api/console/managed-services/umami/hosts/{host_id}${upstream.path}`);
    assert.ok(managed.execution_bindings.some(binding => binding.contribution_id === id && binding.handler === `upstream.${upstream.operation_id}`));
    assert.equal(tools.filter(tool => tool.execution_target.interface_id === id).length, 1, id);
  }
  assert.equal(tools.length, 208);
});

test('whole-body interfaces advertise their original arrays and discriminated objects', () => {
  for (const operationId of ['batch', 'send', 'record', 'legacy_post_reports_attribution']) {
    const tool = tools.find(item => item.execution_target.interface_id === `umami.api.${operationId}`);
    assert.ok(tool.parameter_schema_snapshot.properties.body, operationId);
    assert.ok(tool.input_mapping.mappings.some(mapping => mapping.interface_param === 'body' && mapping.source.path === 'body'));
  }
  const batch = tools.find(tool => tool.name === 'batch');
  assert.equal(batch.parameter_schema_snapshot.properties.body.type, 'array');
  const stats = tools.find(tool => tool.name === 'get_website_stats');
  for (const name of ['host_id', 'websiteId', 'startAt', 'endAt', 'period', 'timezone']) assert.ok(stats.parameter_schema_snapshot.properties[name], name);
  assert.ok(!stats.parameter_schema_snapshot.properties.website_id, 'preserve upstream DTO field names');
});

test('legitimate upstream id fields survive extraction, interface registration and MCP projection', () => {
  const upstream = source.contracts.find(contract => contract.operation.operationId === 'createWebsite');
  assert.ok(upstream.operation.requestBody.content['application/json'].schema.properties.id, 'Umami accepts a caller-provided website id');
  const operation = operations.find(item => item.interface_id === 'umami.api.createWebsite');
  assert.ok(operation.input_schema.properties.body.properties.id, 'HTTP contract must preserve id');
  const tool = tools.find(item => item.name === 'create_website');
  assert.ok(tool.parameter_schema_snapshot.properties.id, 'MCP must expose the same id parameter');
  function verify(value) {
    if (!value || typeof value !== 'object') return;
    if (value.properties && value.required?.includes('id')) assert.ok(Object.hasOwn(value.properties, 'id'), 'Required id has a declared property');
    for (const child of Object.values(value)) verify(child);
  }
  verify(source.contracts);
  verify(operations);
  verify(tools);
});

test('MCP native graph is owned, fully bound, hash verified and reset on activation', () => {
  const instanceFile = bundle.files.find(file => file.kind === 'instance');
  const instance = json(`_assets/mcp/${instanceFile.path}`);
  assert.equal(instance.instance_id, 'umami');
  assert.equal(bundle.organization, 'taichuy');
  assert.equal(bundle.bundle_id, 'umami');
  assert.deepEqual(new Set(instance.bindings.map(binding => binding.tool_id)), new Set(tools.map(tool => tool.tool_id)));
  assert.ok(tools.every(tool => tool.tool_id.startsWith('plugin.umami.')));
  assert.ok(lines.includes('  reset_on_activation: true') || lines.some(line => line.includes('"reset_on_activation":true')));
  for (const file of bundle.files) {
    const digest = createHash('sha256').update(fs.readFileSync(path.join(pluginRoot, '_assets/mcp', file.path))).digest('hex');
    assert.equal(file.sha256, `sha256:${digest}`, file.path);
  }
  const templateLine = lines.find(line => line.startsWith('  mcp_instance: '));
  const template = JSON.parse(templateLine.slice('  mcp_instance: '.length));
  assert.equal(template.reset_on_activation, true);
  assert.equal(template.template.sha256, `sha256:${createHash('sha256').update(fs.readFileSync(path.join(pluginRoot, '_assets/mcp/manifest.json'))).digest('hex')}`);
});

test('connection password is write only and saved login does not accept or expose session credentials', () => {
  for (const id of ['hosts.create', 'hosts.update', 'hosts.test_draft']) {
    const operation = operations.find(op => op.interface_id === `umami.${id}`);
    assert.equal(operation.input_schema.properties.body.properties.password.writeOnly, true);
  }
  const login = tools.find(tool => tool.name === 'login');
  assert.deepEqual(Object.keys(login.parameter_schema_snapshot.properties), ['host_id']);
  assert.equal(login.result_schema_snapshot['x-umami-success-schema'].properties.token, undefined);
  assert.equal(login.result_schema_snapshot['x-umami-success-schema'].properties.authenticated.const, true);
});

test('generated artifacts are deterministic and have no stale snapshots', () => {
  assert.equal(generate(pluginRoot, { check: true }).registered_operations, 208);
});
