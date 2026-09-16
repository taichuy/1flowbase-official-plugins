import process from 'node:process';
import { fileURLToPath } from 'node:url';

const LEGACY_DIGEST_SUFFIX = /^[a-f0-9]{64}\.1flowbasepkg$/;

export function matchesProviderPlatformAsset(assetName, assetBase) {
  if (assetName === `${assetBase}.1flowbasepkg`) {
    return true;
  }

  const legacyPrefix = `${assetBase}@`;
  return (
    assetName.startsWith(legacyPrefix) &&
    LEGACY_DIGEST_SUFFIX.test(assetName.slice(legacyPrefix.length))
  );
}

export function cleanupAssetLines(release, assetBase) {
  if (!release || !Array.isArray(release.assets)) {
    throw new Error('release assets payload must contain an assets array');
  }

  return release.assets
    .filter((asset) => matchesProviderPlatformAsset(asset.name, assetBase))
    .map((asset) => `${asset.id}\t${asset.name}`)
    .join('\n');
}

async function main(argv) {
  if (argv[0] !== 'cleanup-lines' || argv[1] !== '--base' || !argv[2] || argv.length !== 3) {
    throw new Error('usage: node scripts/provider-release/assets.mjs cleanup-lines --base <asset-base>');
  }

  const chunks = [];
  for await (const chunk of process.stdin) {
    chunks.push(chunk);
  }
  const release = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  process.stdout.write(cleanupAssetLines(release, argv[2]));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await main(process.argv.slice(2));
}
