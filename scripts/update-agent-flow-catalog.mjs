import {
  createHash,
  createPrivateKey,
  createPublicKey,
  sign,
  verify,
} from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const AGENT_FLOW_CATALOG_SCHEMA = '1flowbase.agent-flow-catalog/v1';
export const AGENT_FLOW_TEMPLATE_SCHEMAS = new Set([
  '1flowbase.application-archive/v1',
  '1flowbase.application-template/v1',
]);
export const RELEASE_REPOSITORY = 'taichuy/1flowbase-official-plugins';

function json(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function posix(value) {
  return value.split(path.sep).join('/');
}

function sha256(bytes) {
  return `sha256:${createHash('sha256').update(bytes).digest('hex')}`;
}

function assertNonemptyString(value, field) {
  if (typeof value !== 'string' || value.length === 0) {
    throw new Error(`${field} must be a non-empty string`);
  }
}

function assertRfc3339(value, field) {
  assertNonemptyString(value, field);
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/.test(value) || Number.isNaN(Date.parse(value))) {
    throw new Error(`${field} must be an RFC3339 timestamp`);
  }
}

function assertUuid(value, field) {
  assertNonemptyString(value, field);
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value)) {
    throw new Error(`${field} must be a UUID string`);
  }
}

function readCatalog(catalogPath) {
  if (!fs.existsSync(catalogPath)) {
    return { schema_version: AGENT_FLOW_CATALOG_SCHEMA, generated_at: null, templates: [] };
  }
  const catalog = JSON.parse(fs.readFileSync(catalogPath, 'utf8'));
  if (catalog.schema_version !== AGENT_FLOW_CATALOG_SCHEMA || !Array.isArray(catalog.templates)) {
    throw new Error(`unsupported Agent Flow catalog schema in ${catalogPath}`);
  }
  const templateIds = new Set();
  const sourcePaths = new Set();
  for (const template of catalog.templates) {
    assertUuid(template.template_id, 'catalog.templates[].template_id');
    assertNonemptyString(template.source_path, 'catalog.templates[].source_path');
    if (templateIds.has(template.template_id) || sourcePaths.has(template.source_path)) {
      throw new Error(`duplicate Agent Flow catalog identity for ${template.template_id}`);
    }
    templateIds.add(template.template_id);
    sourcePaths.add(template.source_path);
    let previousVersion = 0;
    for (const version of template.versions ?? []) {
      validateReleaseRecord(version);
      if (version.template_id !== template.template_id || version.release_version <= previousVersion) {
        throw new Error(`invalid Agent Flow version history for ${template.template_id}`);
      }
      previousVersion = version.release_version;
    }
  }
  return catalog;
}

export function validateAgentFlowTemplate(document, sourcePath = 'template.json') {
  if (!document || typeof document !== 'object' || Array.isArray(document)) {
    throw new Error(`${sourcePath} must contain one template entry`);
  }
  if (!AGENT_FLOW_TEMPLATE_SCHEMAS.has(document.schema_version)) {
    throw new Error(`${sourcePath} has an unsupported template schema`);
  }
  const entry = document.schema_version === '1flowbase.application-archive/v1'
    ? (() => {
        if (!Array.isArray(document.applications) || document.applications.length !== 1) {
          throw new Error(`${sourcePath} must contain exactly one exported application`);
        }
        return document.applications[0];
      })()
    : document;
  assertUuid(entry.template_id, `${sourcePath}.template_id`);
  if (!Number.isSafeInteger(entry.release_version) || entry.release_version < 1) {
    throw new Error(`${sourcePath}.release_version must be an unsigned integer >= 1`);
  }
  assertNonemptyString(entry.exported_from_system_version, `${sourcePath}.exported_from_system_version`);
  assertRfc3339(entry.exported_at, `${sourcePath}.exported_at`);
  for (const field of ['application', 'flow_document']) {
    if (!entry[field] || typeof entry[field] !== 'object' || Array.isArray(entry[field])) {
      throw new Error(`${sourcePath}.${field} must be an object`);
    }
  }
  if (!Array.isArray(entry.dependencies)) {
    throw new Error(`${sourcePath}.dependencies must be an array`);
  }
  return entry;
}

export function discoverAgentFlowTemplates(repoRoot) {
  const root = path.join(repoRoot, 'agent-flow');
  if (!fs.existsSync(root)) return [];
  const templates = [];
  for (const organizationEntry of fs.readdirSync(root, { withFileTypes: true })) {
    if (!organizationEntry.isDirectory() || !organizationEntry.name.startsWith('@')) continue;
    const organization = organizationEntry.name.slice(1);
    const organizationRoot = path.join(root, organizationEntry.name);
    for (const artifactEntry of fs.readdirSync(organizationRoot, { withFileTypes: true })) {
      if (!artifactEntry.isDirectory()) continue;
      const templatePath = path.join(organizationRoot, artifactEntry.name, 'template.json');
      if (!fs.existsSync(templatePath)) continue;
      const sourcePath = posix(path.relative(repoRoot, templatePath));
      const bytes = fs.readFileSync(templatePath);
      const entry = validateAgentFlowTemplate(JSON.parse(bytes), sourcePath);
      templates.push({
        organization,
        artifact: artifactEntry.name,
        source_path: sourcePath,
        template_path: templatePath,
        entry,
        checksum: sha256(bytes),
      });
    }
  }
  return templates.sort((left, right) => compareText(left.source_path, right.source_path));
}

export function buildAgentFlowReleasePlan({ repoRoot, catalogPath }) {
  const catalog = readCatalog(catalogPath);
  const templates = discoverAgentFlowTemplates(repoRoot);
  const currentIds = new Set();
  const currentPaths = new Set();
  const catalogById = new Map(catalog.templates.map((entry) => [entry.template_id, entry]));
  const catalogByPath = new Map(catalog.templates.map((entry) => [entry.source_path, entry]));
  const pending = [];

  for (const source of templates) {
    const { template_id: templateId, release_version: releaseVersion } = source.entry;
    if (currentIds.has(templateId)) throw new Error(`duplicate template_id ${templateId}`);
    if (currentPaths.has(source.source_path)) throw new Error(`duplicate source path ${source.source_path}`);
    currentIds.add(templateId);
    currentPaths.add(source.source_path);

    const pathOwner = catalogByPath.get(source.source_path);
    if (pathOwner && pathOwner.template_id !== templateId) {
      throw new Error(`${source.source_path} cannot change template_id from ${pathOwner.template_id} to ${templateId}`);
    }
    const history = catalogById.get(templateId);
    if (history && history.source_path !== source.source_path) {
      throw new Error(`template_id ${templateId} is already owned by ${history.source_path}`);
    }
    const versions = history?.versions ?? [];
    const existing = versions.find((version) => version.release_version === releaseVersion);
    if (existing) {
      if (existing.checksum !== source.checksum) {
        throw new Error(`immutable release conflict for ${templateId} v${releaseVersion}: checksum changed`);
      }
      continue;
    }
    const latestVersion = versions.reduce((maximum, version) => Math.max(maximum, version.release_version), 0);
    if (releaseVersion <= latestVersion) {
      throw new Error(`release_version for ${templateId} must be greater than ${latestVersion}`);
    }
    const releaseTag = `agent-flow-${templateId}-v${releaseVersion}`;
    const assetName = `${templateId}-v${releaseVersion}.json`;
    pending.push({
      template_id: templateId,
      release_version: releaseVersion,
      source_path: source.source_path,
      template_path: source.template_path,
      checksum: source.checksum,
      release_tag: releaseTag,
      asset_name: assetName,
      download_url: `https://github.com/${RELEASE_REPOSITORY}/releases/download/${releaseTag}/${assetName}`,
    });
  }
  return pending;
}

export function signAgentFlowArtifact({ templatePath, privateKeyPem, keyId, downloadUrl }) {
  assertNonemptyString(keyId, 'key_id');
  assertNonemptyString(downloadUrl, 'download_url');
  const bytes = fs.readFileSync(templatePath);
  const sourcePath = posix(templatePath);
  const entry = validateAgentFlowTemplate(JSON.parse(bytes), sourcePath);
  const privateKey = createPrivateKey(privateKeyPem);
  if (privateKey.asymmetricKeyType !== 'ed25519') {
    throw new Error('Agent Flow signing key must be Ed25519');
  }
  const signature = sign(null, bytes, privateKey).toString('base64');
  const publicKey = createPublicKey(privateKey);
  if (!verify(null, bytes, publicKey, Buffer.from(signature, 'base64'))) {
    throw new Error('generated Agent Flow signature failed verification');
  }
  return {
    template_id: entry.template_id,
    release_version: entry.release_version,
    exported_from_system_version: entry.exported_from_system_version,
    exported_at: entry.exported_at,
    application: {
      name: entry.application.name ?? '',
      description: entry.application.description ?? '',
    },
    download_url: downloadUrl,
    checksum: sha256(bytes),
    algorithm: 'ed25519',
    key_id: keyId,
    signature,
  };
}

export function verifyAgentFlowArtifact({ artifactBytes, record, publicKeyPem }) {
  if (record.algorithm !== 'ed25519') return false;
  if (sha256(artifactBytes) !== record.checksum) return false;
  return verify(
    null,
    artifactBytes,
    createPublicKey(publicKeyPem),
    Buffer.from(record.signature, 'base64'),
  );
}

function validateReleaseRecord(record) {
  assertUuid(record.template_id, 'record.template_id');
  if (!Number.isSafeInteger(record.release_version) || record.release_version < 1) {
    throw new Error('record.release_version must be an unsigned integer >= 1');
  }
  assertNonemptyString(record.exported_from_system_version, 'record.exported_from_system_version');
  assertRfc3339(record.exported_at, 'record.exported_at');
  assertNonemptyString(record.download_url, 'record.download_url');
  if (!/^https:\/\/github\.com\/[^/]+\/[^/]+\/releases\/download\/[^/]+\/[^/]+$/.test(record.download_url)) {
    throw new Error('record.download_url must be an immutable GitHub Release asset URL');
  }
  if (!/^sha256:[0-9a-f]{64}$/.test(record.checksum)) throw new Error('record.checksum must be sha256');
  if (record.algorithm !== 'ed25519') throw new Error("record.algorithm must be 'ed25519'");
  assertNonemptyString(record.key_id, 'record.key_id');
  assertNonemptyString(record.signature, 'record.signature');
  if (Buffer.from(record.signature, 'base64').toString('base64') !== record.signature) {
    throw new Error('record.signature must be canonical base64');
  }
}

export function updateAgentFlowCatalog({ repoRoot, records, generatedAt = new Date().toISOString() }) {
  assertRfc3339(generatedAt, 'generated_at');
  const catalogPath = path.join(repoRoot, 'agent-flow', 'releases', 'v1', 'catalog.json');
  const catalog = readCatalog(catalogPath);
  const sources = new Map(discoverAgentFlowTemplates(repoRoot).map((source) => [source.entry.template_id, source]));

  for (const record of records) {
    validateReleaseRecord(record);
    const source = sources.get(record.template_id);
    if (!source) throw new Error(`release record has no source template: ${record.template_id}`);
    if (source.entry.release_version !== record.release_version || source.checksum !== record.checksum) {
      throw new Error(`release record does not match source bytes: ${record.template_id} v${record.release_version}`);
    }
    let template = catalog.templates.find((entry) => entry.template_id === record.template_id);
    if (!template) {
      template = {
        template_id: record.template_id,
        organization: source.organization,
        artifact: source.artifact,
        source_path: source.source_path,
        versions: [],
      };
      catalog.templates.push(template);
    }
    const existing = template.versions.find((version) => version.release_version === record.release_version);
    if (existing) {
      if (existing.checksum !== record.checksum) {
        throw new Error(`immutable release conflict for ${record.template_id} v${record.release_version}: checksum changed`);
      }
      continue;
    }
    const latestVersion = template.versions.reduce((maximum, version) => Math.max(maximum, version.release_version), 0);
    if (record.release_version <= latestVersion) {
      throw new Error(`release_version for ${record.template_id} must be greater than ${latestVersion}`);
    }
    template.versions.push(record);
    template.versions.sort((left, right) => left.release_version - right.release_version);
  }

  catalog.schema_version = AGENT_FLOW_CATALOG_SCHEMA;
  catalog.generated_at = generatedAt;
  catalog.templates.sort((left, right) => compareText(left.template_id, right.template_id));
  fs.mkdirSync(path.dirname(catalogPath), { recursive: true });
  const temporaryPath = `${catalogPath}.tmp-${process.pid}`;
  fs.writeFileSync(temporaryPath, json(catalog));
  fs.renameSync(temporaryPath, catalogPath);
  return catalog;
}

function option(name) {
  const index = process.argv.indexOf(name);
  return index === -1 ? null : process.argv[index + 1];
}

function requiredOption(name) {
  const value = option(name);
  if (!value) throw new Error(`missing ${name}`);
  return value;
}

if (path.resolve(process.argv[1] || '') === fileURLToPath(import.meta.url)) {
  const command = process.argv[2];
  if (command === 'plan') {
    const repoRoot = path.resolve(requiredOption('--repo-root'));
    const catalogPath = path.resolve(option('--catalog') || path.join(repoRoot, 'agent-flow/releases/v1/catalog.json'));
    process.stdout.write(JSON.stringify(buildAgentFlowReleasePlan({ repoRoot, catalogPath })));
  } else if (command === 'sign') {
    const templatePath = path.resolve(requiredOption('--template'));
    const record = signAgentFlowArtifact({
      templatePath,
      privateKeyPem: fs.readFileSync(requiredOption('--private-key'), 'utf8'),
      keyId: requiredOption('--key-id'),
      downloadUrl: requiredOption('--download-url'),
    });
    fs.writeFileSync(requiredOption('--output'), json(record));
  } else if (command === 'update') {
    const repoRoot = path.resolve(requiredOption('--repo-root'));
    const recordsRoot = path.resolve(requiredOption('--records-dir'));
    const records = fs.readdirSync(recordsRoot)
      .filter((name) => name.endsWith('.json'))
      .sort(compareText)
      .map((name) => JSON.parse(fs.readFileSync(path.join(recordsRoot, name), 'utf8')));
    updateAgentFlowCatalog({ repoRoot, records, generatedAt: option('--generated-at') || new Date().toISOString() });
  } else {
    throw new Error('usage: update-agent-flow-catalog.mjs <plan|sign|update> [options]');
  }
}
