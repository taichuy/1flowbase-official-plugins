import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  stableJson,
  UI_COMPONENT_SEED_SCHEMA_VERSION,
  verifyUiComponentSeed,
} from './ui-component-catalog-publisher.mjs';

export const UI_COMPONENT_RELEASE_SCHEMA_VERSION = '1flowbase.ui-component-release-catalog/v1';

const RELEASE_FIELDS = Object.freeze([
  'version',
  'release_tag',
  'download_url',
  'checksum',
  'seed_schema_version',
  'total_components',
  'semantic_sha256',
  'algorithm',
  'key_id',
  'signature',
]);

function sha256(bytes) {
  return `sha256:${crypto.createHash('sha256').update(bytes).digest('hex')}`;
}

function locations(repoRoot) {
  return {
    source: path.join(repoRoot, 'ui_components', 'catalog-source.json'),
    seed: path.join(repoRoot, 'ui_components', 'dist', 'catalog-seed.json'),
    releases: path.join(repoRoot, 'ui_components', 'releases', 'v1', 'catalog.json'),
  };
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function releaseTag(version) {
  return `ui-component-catalog-v${version}`;
}

function releaseAssetName(version) {
  return `ui-component-catalog-v${version}.json`;
}

function validateReleaseRecord(record) {
  if (!record || typeof record !== 'object' || Array.isArray(record)) {
    throw new Error('signed UI component release must be an object');
  }
  const actualFields = Object.keys(record).sort();
  const expectedFields = [...RELEASE_FIELDS].sort();
  if (actualFields.length !== expectedFields.length ||
      actualFields.some((field, index) => field !== expectedFields[index])) {
    throw new Error(`signed UI component release fields must be exactly: ${expectedFields.join(', ')}`);
  }
  for (const field of ['version', 'release_tag', 'download_url', 'checksum', 'seed_schema_version', 'semantic_sha256', 'algorithm', 'key_id', 'signature']) {
    if (typeof record[field] !== 'string' || record[field].length === 0) {
      throw new Error(`signed UI component release ${field} must be a non-empty string`);
    }
  }
  if (!/^\d+\.\d+\.\d+$/.test(record.version)) throw new Error('release version must be semantic version x.y.z');
  if (record.release_tag !== releaseTag(record.version)) throw new Error('release tag must match version');
  if (!record.download_url.endsWith(`/${record.release_tag}/${releaseAssetName(record.version)}`)) {
    throw new Error('release download URL must match version and tag');
  }
  if (!/^sha256:[a-f0-9]{64}$/.test(record.checksum) ||
      !/^sha256:[a-f0-9]{64}$/.test(record.semantic_sha256)) {
    throw new Error('release checksums must be SHA-256');
  }
  if (record.seed_schema_version !== UI_COMPONENT_SEED_SCHEMA_VERSION) {
    throw new Error('release Seed schema version is invalid');
  }
  if (!Number.isInteger(record.total_components) || record.total_components < 0) {
    throw new Error('release total_components must be a non-negative integer');
  }
  if (record.algorithm !== 'ed25519') throw new Error('release signature algorithm must be ed25519');
  if (!/^[A-Za-z0-9+/]+={0,2}$/.test(record.signature) ||
      Buffer.from(record.signature, 'base64').toString('base64') !== record.signature) {
    throw new Error('release signature must be canonical base64');
  }
}

export function buildSignedUiComponentRelease({ repoRoot, privateKey, keyId }) {
  if (!repoRoot || !privateKey || !keyId) throw new Error('repoRoot, privateKey, and keyId are required');
  if (privateKey.asymmetricKeyType !== 'ed25519') {
    throw new Error('UI component signing key must be Ed25519');
  }
  const paths = locations(repoRoot);
  const metadata = readJson(paths.source);
  const seed = readJson(paths.seed);
  verifyUiComponentSeed(seed);
  if (metadata.catalog_version !== seed.manifest.catalog_version) {
    throw new Error('Seed version does not match canonical UI component source');
  }
  const bytes = fs.readFileSync(paths.seed);
  const tag = releaseTag(metadata.catalog_version);
  const record = {
    version: metadata.catalog_version,
    release_tag: tag,
    download_url: `https://github.com/taichuy/1flowbase-official-plugins/releases/download/${tag}/${releaseAssetName(metadata.catalog_version)}`,
    checksum: sha256(bytes),
    seed_schema_version: seed.manifest.schema_version,
    total_components: seed.manifest.total_components,
    semantic_sha256: seed.manifest.semantic_sha256,
    algorithm: 'ed25519',
    key_id: keyId,
    signature: crypto.sign(null, bytes, privateKey).toString('base64'),
  };
  validateReleaseRecord(record);
  return record;
}

export function verifySignedUiComponentRelease({ repoRoot, record, publicKey }) {
  validateReleaseRecord(record);
  if (!publicKey || publicKey.asymmetricKeyType !== 'ed25519') {
    throw new Error('UI component public key must be Ed25519');
  }
  const paths = locations(repoRoot);
  const seed = readJson(paths.seed);
  verifyUiComponentSeed(seed);
  if (record.version !== seed.manifest.catalog_version ||
      record.seed_schema_version !== seed.manifest.schema_version ||
      record.total_components !== seed.manifest.total_components ||
      record.semantic_sha256 !== seed.manifest.semantic_sha256) {
    throw new Error('UI component release metadata does not match the Seed');
  }
  const bytes = fs.readFileSync(paths.seed);
  if (sha256(bytes) !== record.checksum) throw new Error('UI component release checksum mismatch');
  if (!crypto.verify(null, bytes, publicKey, Buffer.from(record.signature, 'base64'))) {
    throw new Error('UI component release signature is invalid');
  }
  return true;
}

export function registerSignedUiComponentRelease({ repoRoot, record, publicKey }) {
  verifySignedUiComponentRelease({ repoRoot, record, publicKey });
  const paths = locations(repoRoot);
  const sourceVersion = readJson(paths.source).catalog_version;
  if (sourceVersion !== record.version) {
    throw new Error('release version does not match the canonical UI component source');
  }
  const catalog = fs.existsSync(paths.releases)
    ? readJson(paths.releases)
    : { schema_version: UI_COMPONENT_RELEASE_SCHEMA_VERSION, releases: [] };
  if (catalog.schema_version !== UI_COMPONENT_RELEASE_SCHEMA_VERSION || !Array.isArray(catalog.releases)) {
    throw new Error('invalid signed UI component release catalog');
  }
  const existing = catalog.releases.find((release) => release.version === record.version);
  if (existing && stableJson(existing) !== stableJson(record)) {
    throw new Error(`immutable release conflict for ${record.version}`);
  }
  if (!existing) catalog.releases.push(record);
  catalog.releases.sort((left, right) => left.version.localeCompare(
    right.version,
    undefined,
    { numeric: true },
  ));
  fs.mkdirSync(path.dirname(paths.releases), { recursive: true });
  fs.writeFileSync(paths.releases, stableJson(catalog));
  return catalog;
}

function parseCli(argv) {
  const [command, ...args] = argv;
  const options = { command };
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === '--private-key-pem-file') options.privateKeyPath = path.resolve(args[++index]);
    else if (argument === '--key-id') options.keyId = args[++index];
    else if (argument === '--record') options.recordPath = path.resolve(args[++index]);
    else throw new Error('usage: node scripts/ui-component-release.mjs build|register --private-key-pem-file PATH --key-id ID --record PATH');
  }
  return options;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const options = parseCli(process.argv.slice(2));
  if (!options.privateKeyPath || !options.recordPath) {
    throw new Error('private key and release record paths are required');
  }
  const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  const privateKey = crypto.createPrivateKey(fs.readFileSync(options.privateKeyPath, 'utf8'));
  const publicKey = crypto.createPublicKey(privateKey);
  if (options.command === 'build') {
    if (!options.keyId) throw new Error('key id is required to build a release record');
    const record = buildSignedUiComponentRelease({ repoRoot, privateKey, keyId: options.keyId });
    fs.writeFileSync(options.recordPath, stableJson(record));
  } else if (options.command === 'register') {
    registerSignedUiComponentRelease({
      repoRoot,
      record: readJson(options.recordPath),
      publicKey,
    });
  } else {
    throw new Error('command must be build or register');
  }
}
