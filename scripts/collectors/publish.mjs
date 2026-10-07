import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { listCollectors, PLATFORM_TARGETS, archiveName } from './catalog.mjs';

const repoRoot = path.resolve(import.meta.dirname, '../..');
const repository = process.env.GITHUB_REPOSITORY || 'taichuy/1flowbase-official-plugins';
const sourceSha = process.env.GITHUB_SHA;
const signingPem = process.env.OFFICIAL_PLUGIN_SIGNING_KEY_PEM;
const signingKeyId = process.env.OFFICIAL_PLUGIN_SIGNING_KEY_ID;
if (!sourceSha || !signingPem || !signingKeyId) throw new Error('Release source SHA and official signing configuration are required');
const privateKey = crypto.createPrivateKey(signingPem);
if (privateKey.asymmetricKeyType !== 'ed25519') throw new Error('Official collector signing key must use Ed25519');

for (const collector of listCollectors(repoRoot)) {
  const releaseDir = path.join(repoRoot, 'dist', 'release', collector.collector_code);
  fs.mkdirSync(releaseDir, { recursive: true });
  const artifacts = PLATFORM_TARGETS.map(platform => {
    const name = archiveName(collector, platform);
    const source = path.join(repoRoot, 'dist', 'collectors', name);
    const bytes = fs.readFileSync(source); // All six builds must exist before publication.
    fs.copyFileSync(source, path.join(releaseDir, name));
    return { ...platform, name, sha256: crypto.createHash('sha256').update(bytes).digest('hex') };
  });
  for (const [source, destination] of [['install.sh', 'install.sh'], ['install.ps1', 'install.ps1']]) {
    fs.copyFileSync(path.join(repoRoot, 'installers', 'client-collectors', source), path.join(releaseDir, destination));
  }
  const checksums = artifacts.map(item => `${item.sha256}  ${item.name}`).join('\n') + '\n';
  fs.writeFileSync(path.join(releaseDir, 'checksums.txt'), checksums);
  fs.writeFileSync(path.join(releaseDir, 'checksums.txt.sig'), crypto.sign(null, Buffer.from(checksums), privateKey));
  fs.writeFileSync(path.join(releaseDir, 'signing-public-key.pem'), crypto.createPublicKey(privateKey).export({ type: 'spki', format: 'pem' }));
  fs.writeFileSync(path.join(releaseDir, 'release.json'), JSON.stringify({
    schema_version: '1flowbase.client-collector-release/v1', ...collector,
    source_sha: sourceSha, signature_algorithm: 'ed25519', signing_key_id: signingKeyId,
    artifacts: artifacts.map(({ os, arch, rust_target, name, sha256 }) => ({ os, arch, rust_target, name, sha256 })),
  }, null, 2) + '\n');
  let existing;
  try {
    existing = JSON.parse(execFileSync('gh', ['release', 'view', collector.release_tag, '--repo', repository, '--json', 'targetCommitish,assets'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }));
  } catch {
    // An authenticated API lookup below distinguishes a missing tag from other failures.
    const tags = JSON.parse(execFileSync('gh', ['api', `repos/${repository}/releases`, '--paginate', '--slurp'], { encoding: 'utf8' })).flat();
    if (tags.some(item => item.tag_name === collector.release_tag)) throw new Error('Could not read existing collector release');
  }
  if (existing) {
    const expectedNames = fs.readdirSync(releaseDir);
    if (existing.targetCommitish !== sourceSha || !expectedNames.every(name => existing.assets.some(asset => asset.name === name))) {
      throw new Error('Collector release is immutable; bump its version before publishing changed sources');
    }
    process.stdout.write(`${collector.release_tag}: existing immutable release retained\n`);
    continue;
  }
  const notes = path.join(releaseDir, 'notes.md');
  fs.writeFileSync(notes, `Native client collector for Codex. Runs on the client computer; it is not a server runtime extension.\n\nSource: ${sourceSha}\n\nInstallers read the application API Key locally. SHA-256 checksums and an Ed25519 signature are included.\n`);
  const assets = fs.readdirSync(releaseDir).filter(name => name !== 'notes.md').map(name => path.join(releaseDir, name));
  execFileSync('gh', ['release', 'create', collector.release_tag, ...assets, '--repo', repository, '--target', sourceSha,
    '--title', `${collector.display_name} logs collector ${collector.version}`, '--notes-file', notes, '--latest=false'], { stdio: 'inherit' });
}
