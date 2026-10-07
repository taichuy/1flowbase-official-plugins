import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { PLATFORM_TARGETS, archiveName } from './catalog.mjs';

const json = value => `${JSON.stringify(value, null, 2)}\n`;
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
export const distributionName = collector => `${collector.collector_code}-${collector.version}-distribution.tar.gz`;

function regularBytes(file) {
  const stat = fs.lstatSync(file);
  if (!stat.isFile() || stat.nlink !== 1) throw new Error(`Distribution asset must be a regular unlinked file: ${path.basename(file)}`);
  return fs.readFileSync(file);
}

// Sign the exact outer artifact bytes using the existing generic catalog descriptor:
// checksum sha256:<hex>, signature {algorithm, key_id, signature (base64)}.
export function buildDistribution({ repoRoot, collector, nativeDirectory, outputDirectory, sourceSha, privateKey, keyId }) {
  if (!sourceSha || !keyId || privateKey?.asymmetricKeyType !== 'ed25519') throw new Error('Source SHA and official Ed25519 signing configuration are required');
  const stage = path.join(outputDirectory, 'members');
  fs.mkdirSync(outputDirectory, { recursive: true });
  fs.rmSync(stage, { recursive: true, force: true });
  fs.mkdirSync(stage);
  const write = (name, bytes) => fs.writeFileSync(path.join(stage, name), bytes);
  const artifacts = PLATFORM_TARGETS.map(({ os, arch, rust_target }) => {
    const name = archiveName(collector, { os, arch });
    const bytes = regularBytes(path.join(nativeDirectory, name)); // Never unpack native archives.
    write(name, bytes);
    return { os, arch, rust_target, name, sha256: digest(bytes), size: bytes.length };
  });
  for (const name of ['install.sh', 'install.ps1']) write(name, regularBytes(path.join(repoRoot, 'installers/client-collectors', name)));
  for (const name of ['README.md', 'README.en.md']) write(name, regularBytes(path.join(repoRoot, collector.plugin_dir, name)));
  const checksums = Buffer.from(artifacts.map(item => `${item.sha256}  ${item.name}`).join('\n') + '\n');
  write('checksums.txt', checksums);
  write('checksums.txt.sig', crypto.sign(null, checksums, privateKey));
  write('signing-public-key.pem', crypto.createPublicKey(privateKey).export({ type: 'spki', format: 'pem' }));
  const { plugin_dir: _pluginDir, release_tag: _releaseTag, assets: _assets, ...manifest } = collector;
  write('release.json', json({ schema_version: '1flowbase.client-collector-release/v1', collector_code: collector.collector_code,
    version: collector.version, source_sha: sourceSha, signature_algorithm: 'ed25519', signing_key_id: keyId, artifacts }));
  const names = fs.readdirSync(stage).sort();
  manifest.assets = names.map(name => {
    const bytes = regularBytes(path.join(stage, name));
    return { name, sha256: digest(bytes), size: bytes.length };
  });
  write('collector-manifest.json', json(manifest));
  const archive = path.join(outputDirectory, distributionName(collector));
  // Explicit flat members avoid directory entries, links, and unexpected payloads.
  execFileSync('tar', ['--format=ustar', '--mtime=@0', '--owner=0', '--group=0', '--numeric-owner',
    '--mode=0644', '-czf', archive, '-C', stage, 'collector-manifest.json', ...names]);
  const bytes = fs.readFileSync(archive);
  const signature = { algorithm: 'ed25519', key_id: keyId, signature: crypto.sign(null, bytes, privateKey).toString('base64') };
  const entry = {
    name: collector.display_name, version: collector.version, description: collector.description.en_US,
    host_version_requirement: `>=${collector.minimum_host_version}`,
    source: { kind: 'runtime_extension_manifest', locator: `${collector.plugin_dir}/collector-manifest.json`,
      distribution_kind: 'client_collector', client_collector: {
        collector_code: collector.collector_code, source_client: collector.source_client, display_name: collector.display_name,
        execution_target: 'client', protocol_version: collector.protocol_version,
      } },
    checksum: `sha256:${digest(bytes)}`, signature,
    download_locator: { kind: 'release_asset', locator: null }, slot_codes: [], keywords: ['client', 'collector', collector.source_client],
  };
  return { archive, entry, manifest };
}

export function verifyDistributionSignature(bytes, entry, publicKey) {
  if (entry.checksum !== `sha256:${digest(bytes)}` || entry.signature?.algorithm !== 'ed25519' ||
      !entry.signature.key_id || !crypto.verify(null, bytes, publicKey, Buffer.from(entry.signature.signature, 'base64'))) {
    throw new Error('Distribution checksum or signature mismatch');
  }
  return true;
}
