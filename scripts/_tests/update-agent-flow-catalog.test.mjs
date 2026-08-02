import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  AGENT_FLOW_CATALOG_SCHEMA,
  buildAgentFlowReleasePlan,
  signAgentFlowArtifact,
  updateAgentFlowCatalog,
  validateAgentFlowTemplate,
  verifyAgentFlowArtifact,
} from '../update-agent-flow-catalog.mjs';

// Deterministic fixture key for tests only. Production keys come from GitHub Actions secrets.
const TEST_PRIVATE_KEY = `-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEIJc4ZPHgt74X/YRVKoTqHBTYTVWQs2he1XCxFc0SiD3I
-----END PRIVATE KEY-----
`;
const TEST_PUBLIC_KEY = `-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEA6s4oAVb83/zXHAYtpr0Sj53dmBwiLCFjrJ2Yb3VLzr4=
-----END PUBLIC KEY-----
`;
const TEMPLATE_ID = '76dfdbb6-cbc5-4bd7-bdc9-cc7c2b720f70';

function fixtureEntry(releaseVersion = 1) {
  return {
    schema_version: '1flowbase.application-archive/v1',
    applications: [{
      template_id: TEMPLATE_ID,
      release_version: releaseVersion,
      exported_from_system_version: '0.3.0',
      exported_at: '2026-08-01T00:00:00Z',
      application: { application_type: 'agent_flow', name: 'Example', description: '' },
      flow_document: { meta: { name: 'Example' }, graph: { nodes: [], edges: [] } },
      dependencies: [],
    }],
  };
}

function fixtureRepository(releaseVersion = 1) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'agent-flow-release-'));
  const templateRoot = path.join(root, 'agent-flow', '@taichuy', 'example');
  fs.mkdirSync(templateRoot, { recursive: true });
  const templatePath = path.join(templateRoot, 'template.json');
  fs.writeFileSync(templatePath, `${JSON.stringify(fixtureEntry(releaseVersion), null, 2)}\n`);
  fs.mkdirSync(path.join(root, 'agent-flow', 'releases', 'v1'), { recursive: true });
  fs.writeFileSync(path.join(root, 'agent-flow', 'releases', 'v1', 'catalog.json'), `${JSON.stringify({
    schema_version: AGENT_FLOW_CATALOG_SCHEMA,
    generated_at: null,
    templates: [],
  }, null, 2)}\n`);
  return { root, templatePath };
}

function signedRecord(templatePath, releaseVersion = 1) {
  return signAgentFlowArtifact({
    templatePath,
    privateKeyPem: TEST_PRIVATE_KEY,
    keyId: 'test-agent-flow-2026',
    downloadUrl: `https://github.com/taichuy/1flowbase-official-plugins/releases/download/agent-flow-${TEMPLATE_ID}-v${releaseVersion}/${TEMPLATE_ID}-v${releaseVersion}.json`,
  });
}

test('AC-002 validates the frozen single-template export contract', () => {
  assert.equal(validateAgentFlowTemplate(fixtureEntry()).template_id, TEMPLATE_ID);
  const invalidVersion = fixtureEntry();
  invalidVersion.applications[0].release_version = 0;
  assert.throws(
    () => validateAgentFlowTemplate(invalidVersion),
    /unsigned integer >= 1/,
  );
  const multipleApplications = fixtureEntry();
  multipleApplications.applications.push(multipleApplications.applications[0]);
  assert.throws(
    () => validateAgentFlowTemplate(multipleApplications),
    /exactly one exported application/,
  );
});

test('AC-007 signs and checksums the exact artifact bytes with Ed25519', () => {
  const { templatePath } = fixtureRepository();
  const bytes = fs.readFileSync(templatePath);
  const record = signedRecord(templatePath);

  assert.equal(record.algorithm, 'ed25519');
  assert.equal(record.checksum, `sha256:${createHash('sha256').update(bytes).digest('hex')}`);
  assert.equal(record.key_id, 'test-agent-flow-2026');
  assert.match(record.signature, /^[A-Za-z0-9+/]+={0,2}$/);
  assert.equal(verifyAgentFlowArtifact({ artifactBytes: bytes, record, publicKeyPem: TEST_PUBLIC_KEY }), true);
  assert.equal(verifyAgentFlowArtifact({ artifactBytes: Buffer.concat([bytes, Buffer.from(' ')]), record, publicKeyPem: TEST_PUBLIC_KEY }), false);
});

test('AC-007 rejects the same template ID and version when artifact bytes change', () => {
  const { root, templatePath } = fixtureRepository();
  const record = signedRecord(templatePath);
  updateAgentFlowCatalog({ repoRoot: root, records: [record], generatedAt: '2026-08-01T01:00:00Z' });
  const changed = fixtureEntry();
  changed.applications[0].application.name = 'Changed without a version bump';
  fs.writeFileSync(templatePath, `${JSON.stringify(changed, null, 2)}\n`);

  assert.throws(
    () => buildAgentFlowReleasePlan({
      repoRoot: root,
      catalogPath: path.join(root, 'agent-flow/releases/v1/catalog.json'),
    }),
    /immutable release conflict.*checksum changed/,
  );
});

test('AC-007 catalog uses immutable Release URLs and enumerates version history', () => {
  const { root, templatePath } = fixtureRepository();
  const versionOne = signedRecord(templatePath, 1);
  updateAgentFlowCatalog({ repoRoot: root, records: [versionOne], generatedAt: '2026-08-01T01:00:00Z' });

  fs.writeFileSync(templatePath, `${JSON.stringify(fixtureEntry(2), null, 2)}\n`);
  const versionTwo = signedRecord(templatePath, 2);
  const catalog = updateAgentFlowCatalog({
    repoRoot: root,
    records: [versionTwo],
    generatedAt: '2026-08-01T02:00:00Z',
  });

  assert.deepEqual(catalog.templates[0].versions.map((entry) => entry.release_version), [1, 2]);
  assert.ok(catalog.templates[0].versions.every((entry) => entry.download_url.includes('/releases/download/')));
  assert.ok(catalog.templates[0].versions.every((entry) => !entry.download_url.includes('/raw/')));
  assert.equal(catalog.templates[0].versions[1].algorithm, 'ed25519');
  assert.equal(catalog.templates[0].versions[1].key_id, 'test-agent-flow-2026');
});

test('AC-007 rejects non-Release artifact URLs before catalog generation', () => {
  const { root, templatePath } = fixtureRepository();
  const record = {
    ...signedRecord(templatePath),
    download_url: 'https://raw.githubusercontent.com/taichuy/1flowbase-official-plugins/main/template.json',
  };
  assert.throws(
    () => updateAgentFlowCatalog({ repoRoot: root, records: [record] }),
    /immutable GitHub Release asset URL/,
  );
});
