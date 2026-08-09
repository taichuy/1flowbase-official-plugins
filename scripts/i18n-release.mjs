import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const I18N_RELEASE_SCHEMA_VERSION = '1flowbase.i18n-release-catalog/v1';

function json(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function sha256(bytes) {
  return `sha256:${crypto.createHash('sha256').update(bytes).digest('hex')}`;
}

function paths(repoRoot) {
  return {
    source: path.join(repoRoot, 'i18n', 'catalog.json'),
    seed: path.join(repoRoot, 'i18n', 'dist', 'catalog-seed.json'),
    releases: path.join(repoRoot, 'i18n', 'releases', 'v1', 'catalog.json'),
  };
}

function validateRecord(record) {
  const fields = ['version', 'release_tag', 'download_url', 'checksum', 'algorithm', 'key_id', 'signature'];
  if (!record || typeof record !== 'object' || fields.some((field) => typeof record[field] !== 'string' || !record[field])) {
    throw new Error(`signed i18n release fields must be exactly: ${fields.join(', ')}`);
  }
  if (Object.keys(record).sort().join(',') !== fields.sort().join(',')) {
    throw new Error(`signed i18n release fields must be exactly: ${fields.join(', ')}`);
  }
  if (!/^\d+\.\d+\.\d+$/.test(record.version)) throw new Error('release version must be semantic version x.y.z');
  if (record.release_tag !== `i18n-catalog-v${record.version}`) throw new Error('release tag must match version');
  if (!record.download_url.endsWith(`/${record.release_tag}/i18n-catalog-seed-v${record.version}.json`)) {
    throw new Error('release download URL must match version and tag');
  }
  if (!/^sha256:[a-f0-9]{64}$/.test(record.checksum)) throw new Error('release checksum must be SHA-256');
  if (record.algorithm !== 'ed25519') throw new Error('release signature algorithm must be ed25519');
  if (Buffer.from(record.signature, 'base64').toString('base64') !== record.signature) {
    throw new Error('release signature must be canonical base64');
  }
}

export function buildSignedI18nRelease({ repoRoot, privateKey, keyId }) {
  if (!repoRoot || !privateKey || !keyId) throw new Error('repoRoot, privateKey, and keyId are required');
  if (privateKey.asymmetricKeyType !== 'ed25519') throw new Error('i18n signing key must be Ed25519');
  const locations = paths(repoRoot);
  const version = JSON.parse(fs.readFileSync(locations.source, 'utf8')).catalog_version;
  const bytes = fs.readFileSync(locations.seed);
  const releaseTag = `i18n-catalog-v${version}`;
  const record = {
    version,
    release_tag: releaseTag,
    download_url: `https://github.com/taichuy/1flowbase-official-plugins/releases/download/${releaseTag}/i18n-catalog-seed-v${version}.json`,
    checksum: sha256(bytes),
    algorithm: 'ed25519',
    key_id: keyId,
    signature: crypto.sign(null, bytes, privateKey).toString('base64'),
  };
  validateRecord(record);
  return record;
}

export function verifySignedI18nRelease({ repoRoot, record, publicKey }) {
  validateRecord(record);
  if (!publicKey || publicKey.asymmetricKeyType !== 'ed25519') throw new Error('i18n public key must be Ed25519');
  const bytes = fs.readFileSync(paths(repoRoot).seed);
  if (sha256(bytes) !== record.checksum) throw new Error('i18n release checksum mismatch');
  if (!crypto.verify(null, bytes, publicKey, Buffer.from(record.signature, 'base64'))) {
    throw new Error('i18n release signature is invalid');
  }
  return true;
}

export function registerSignedI18nRelease({ repoRoot, record, publicKey }) {
  verifySignedI18nRelease({ repoRoot, record, publicKey });
  const locations = paths(repoRoot);
  const sourceVersion = JSON.parse(fs.readFileSync(locations.source, 'utf8')).catalog_version;
  if (sourceVersion !== record.version) throw new Error('release version does not match the canonical i18n source');
  const catalog = fs.existsSync(locations.releases)
    ? JSON.parse(fs.readFileSync(locations.releases, 'utf8'))
    : { schema_version: I18N_RELEASE_SCHEMA_VERSION, releases: [] };
  if (catalog.schema_version !== I18N_RELEASE_SCHEMA_VERSION || !Array.isArray(catalog.releases)) {
    throw new Error('invalid signed i18n release catalog');
  }
  const existing = catalog.releases.find((release) => release.version === record.version);
  if (existing && json(existing) !== json(record)) throw new Error(`immutable i18n release conflict for ${record.version}`);
  if (!existing) catalog.releases.push(record);
  catalog.releases.sort((left, right) => left.version.localeCompare(right.version, undefined, { numeric: true }));
  fs.mkdirSync(path.dirname(locations.releases), { recursive: true });
  fs.writeFileSync(locations.releases, json(catalog));
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
    else throw new Error('usage: node scripts/i18n-release.mjs build|register --private-key-pem-file PATH --key-id ID --record PATH');
  }
  return options;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const options = parseCli(process.argv.slice(2));
  const repoRoot = path.resolve(import.meta.dirname, '..');
  const privateKey = crypto.createPrivateKey(fs.readFileSync(options.privateKeyPath, 'utf8'));
  const publicKey = crypto.createPublicKey(privateKey);
  if (options.command === 'build') {
    const record = buildSignedI18nRelease({ repoRoot, privateKey, keyId: options.keyId });
    fs.writeFileSync(options.recordPath, json(record));
  } else if (options.command === 'register') {
    const record = JSON.parse(fs.readFileSync(options.recordPath, 'utf8'));
    registerSignedI18nRelease({ repoRoot, record, publicKey });
  } else {
    throw new Error('command must be build or register');
  }
}
