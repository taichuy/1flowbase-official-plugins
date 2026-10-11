import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { listCollectors } from './catalog.mjs';
import { planCollectorRelease } from './release-plan.mjs';
import { buildDistribution, verifyDistributionSignature, distributionName } from './distribution.mjs';
import { updateCategoryCatalog } from '../extension-catalog.mjs';

const repoRoot = path.resolve(import.meta.dirname, '../..');
const repository = process.env.GITHUB_REPOSITORY || 'taichuy/1flowbase-official-plugins';
const sourceSha = process.env.GITHUB_SHA;
const signingPem = process.env.OFFICIAL_PLUGIN_SIGNING_KEY_PEM;
const keyId = process.env.OFFICIAL_PLUGIN_SIGNING_KEY_ID;
if (!sourceSha || !signingPem || !keyId) throw new Error('Release source SHA and official signing configuration are required');
const privateKey = crypto.createPrivateKey(signingPem);
const publicKey = crypto.createPublicKey(privateKey);
for (const collector of listCollectors(repoRoot)) {
  const outputDirectory = path.join(repoRoot, 'dist/release', collector.collector_code);
  const plan = planCollectorRelease({ repoRoot, collector, repository, sourceSha });
  if (plan.action === 'reuse') {
    const download = fs.mkdtempSync(path.join(os.tmpdir(), 'collector-release-reuse-'));
    try {
      execFileSync('gh', ['release', 'download', collector.release_tag, '--repo', repository, '--pattern', distributionName(collector), '--dir', download]);
      if (plan.entry.signature.key_id !== keyId) throw new Error('Published collector signing key differs from the configured key');
      verifyDistributionSignature(fs.readFileSync(path.join(download, distributionName(collector))), plan.entry, publicKey);
    } finally { fs.rmSync(download, { recursive: true, force: true }); }
    process.stdout.write(`${collector.release_tag}: verified and retained unchanged signed distribution\n`);
    continue;
  }
  const { archive, entry } = buildDistribution({ repoRoot, collector, nativeDirectory: path.join(repoRoot, 'dist/collectors'),
    outputDirectory, sourceSha: plan.sourceSha, privateKey, keyId });
  const name = distributionName(collector);
  entry.download_locator.locator = `https://github.com/${repository}/releases/download/${collector.release_tag}/${name}`;
  const existing = plan.existing;
  if (existing) {
    if (existing.targetCommitish !== plan.sourceSha || existing.assets.length !== 1 || existing.assets[0].name !== name) {
      throw new Error('Collector release is immutable; bump its version before publishing changed sources');
    }
    const download = fs.mkdtempSync(path.join(os.tmpdir(), 'collector-release-verify-'));
    try {
      execFileSync('gh', ['release', 'download', collector.release_tag, '--repo', repository, '--pattern', name, '--dir', download]);
      verifyDistributionSignature(fs.readFileSync(path.join(download, name)), entry, publicKey);
    } finally { fs.rmSync(download, { recursive: true, force: true }); }
  } else {
    const notes = path.join(outputDirectory, 'notes.md');
    fs.writeFileSync(notes, `Native client collector for ${collector.display_name}. Retain the signed distribution in 1flowbase, then copy client commands from the application.\n\nSource: ${sourceSha}\n\nIncludes six native archives and public installers. Keys stay on the client. Catalog installation does not start a collector.\n`);
    execFileSync('gh', ['release', 'create', collector.release_tag, archive, '--repo', repository, '--target', sourceSha,
      '--title', `${collector.display_name} logs collector ${collector.version}`, '--notes-file', notes, '--latest=false'], { stdio: 'inherit' });
    // Verify published bytes before advertising the asset in the official catalog.
    const download = fs.mkdtempSync(path.join(os.tmpdir(), 'collector-release-verify-'));
    try {
      execFileSync('gh', ['release', 'download', collector.release_tag, '--repo', repository, '--pattern', name, '--dir', download]);
      verifyDistributionSignature(fs.readFileSync(path.join(download, name)), entry, publicKey);
    } finally { fs.rmSync(download, { recursive: true, force: true }); }
  }
  fs.writeFileSync(path.join(repoRoot, collector.plugin_dir, 'catalog-entry.json'), `${JSON.stringify(entry, null, 2)}\n`);
  process.stdout.write(`${collector.release_tag}: verified signed distribution and canonical catalog entry\n`);
}
updateCategoryCatalog({ repoRoot, category: 'runtime-extensions' });
