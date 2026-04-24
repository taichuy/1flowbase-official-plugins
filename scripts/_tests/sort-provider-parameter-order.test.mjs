import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import {
  sortProviderParameterOrderContent,
  sortProviderParameterOrderForPlugin,
} from '../sort-provider-parameter-order.mjs';

function makeProviderRoot() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'provider-parameter-order-'));
}

function writeProviderYaml(rootDir, providerCode, content) {
  const providerDir = path.join(
    rootDir,
    'runtime-extensions',
    'model-providers',
    providerCode,
    'provider'
  );
  fs.mkdirSync(providerDir, { recursive: true });
  const providerPath = path.join(providerDir, `${providerCode}.yaml`);
  fs.writeFileSync(providerPath, content, 'utf8');
  return providerPath;
}

test('sortProviderParameterOrderContent rewrites order by current field sequence only', () => {
  const content = [
    'provider_code: openai_compatible',
    'parameter_form:',
    '  fields:',
    '  - key: beta',
    '    label: Beta',
    '    type: string',
    '    order: 90',
    '  - key: alpha',
    '    label: Alpha',
    '    type: string',
    '    order: 10',
    'config_schema:',
    '- key: base_url',
    '  type: string',
    '',
  ].join('\n');

  const result = sortProviderParameterOrderContent(content);

  assert.equal(result.changed, true);
  assert.equal(result.fieldCount, 2);
  assert.ok(result.content.includes(['  - key: beta', '    label: Beta', '    type: string', '    order: 10'].join('\n')));
  assert.ok(result.content.includes(['  - key: alpha', '    label: Alpha', '    type: string', '    order: 20'].join('\n')));
  assert.ok(result.content.includes(['config_schema:', '- key: base_url', '  type: string'].join('\n')));
});

test('sortProviderParameterOrderContent inserts missing order in sequence', () => {
  const content = [
    'parameter_form:',
    '  fields:',
    '  - key: first',
    '    label: First',
    '    type: string',
    '  - key: second',
    '    label: Second',
    '    type: string',
    '    order: 80',
    '',
  ].join('\n');

  const result = sortProviderParameterOrderContent(content);

  assert.equal(result.changed, true);
  assert.ok(result.content.includes(['  - key: first', '    order: 10', '    label: First'].join('\n')));
  assert.ok(result.content.includes(['  - key: second', '    label: Second', '    type: string', '    order: 20'].join('\n')));
});

test('sortProviderParameterOrderForPlugin resolves provider yaml from plugin_id', () => {
  const rootDir = makeProviderRoot();
  const providerPath = writeProviderYaml(
    rootDir,
    'openai_compatible',
    [
      'provider_code: openai_compatible',
      'parameter_form:',
      '  fields:',
      '  - key: temperature',
      '    type: number',
      '    order: 50',
      '  - key: top_p',
      '    type: number',
      '    order: 10',
      '',
    ].join('\n')
  );

  const result = sortProviderParameterOrderForPlugin('openai_compatible@0.3.16', { rootDir });

  assert.equal(result.providerCode, 'openai_compatible');
  assert.equal(result.providerPath, providerPath);
  assert.equal(result.changed, true);
  assert.ok(fs.readFileSync(providerPath, 'utf8').includes(['  - key: temperature', '    type: number', '    order: 10'].join('\n')));
  assert.ok(fs.readFileSync(providerPath, 'utf8').includes(['  - key: top_p', '    type: number', '    order: 20'].join('\n')));
});
