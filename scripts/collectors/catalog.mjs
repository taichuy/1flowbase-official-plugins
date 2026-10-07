import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const PLATFORM_TARGETS = [
  { os: 'linux', arch: 'amd64', rust_target: 'x86_64-unknown-linux-musl', runs_on: 'ubuntu-latest' },
  { os: 'linux', arch: 'arm64', rust_target: 'aarch64-unknown-linux-musl', runs_on: 'ubuntu-latest' },
  { os: 'darwin', arch: 'amd64', rust_target: 'x86_64-apple-darwin', runs_on: 'macos-latest' },
  { os: 'darwin', arch: 'arm64', rust_target: 'aarch64-apple-darwin', runs_on: 'macos-latest' },
  { os: 'windows', arch: 'amd64', rust_target: 'x86_64-pc-windows-msvc', runs_on: 'windows-latest' },
  { os: 'windows', arch: 'arm64', rust_target: 'aarch64-pc-windows-msvc', runs_on: 'windows-latest' },
];

export function listCollectors(repoRoot) {
  const root = path.join(repoRoot, 'runtime-extensions');
  const result = [];
  for (const organization of fs.readdirSync(root, { withFileTypes: true })) {
    if (!organization.isDirectory() || !organization.name.startsWith('@')) continue;
    for (const entry of fs.readdirSync(path.join(root, organization.name), { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const pluginDir = path.join('runtime-extensions', organization.name, entry.name).split(path.sep).join('/');
      const manifestPath = path.join(repoRoot, pluginDir, 'collector-manifest.json');
      if (!fs.existsSync(manifestPath)) continue;
      const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
      if (manifest.schema_version !== '1flowbase.client-collector/v1' || manifest.execution_target !== 'client' ||
          !/^[a-z][a-z0-9-]*$/.test(manifest.collector_code) || !/^\d+\.\d+\.\d+$/.test(manifest.version) ||
          manifest.entry !== manifest.collector_code || manifest.protocol_version !== '1flowbase.agent-logs/v1') {
        throw new Error(`Invalid client collector manifest: ${pluginDir}`);
      }
      if (fs.existsSync(path.join(repoRoot, pluginDir, 'manifest.yaml'))) {
        throw new Error(`Client collector must not enter server runtime discovery: ${pluginDir}`);
      }
      result.push({ ...manifest, plugin_dir: pluginDir, release_tag: `${manifest.collector_code}-v${manifest.version}` });
    }
  }
  return result.sort((a, b) => a.collector_code.localeCompare(b.collector_code));
}

export function archiveName(collector, platform) {
  return `${collector.collector_code}-${collector.version}-${platform.os}-${platform.arch}.${platform.os === 'windows' ? 'zip' : 'tar.gz'}`;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const repoRoot = path.resolve(import.meta.dirname, '../..');
  const code = process.argv[2] || 'codex-logs-collector';
  const collectors = listCollectors(repoRoot).filter(item => code === 'all' || item.collector_code === code);
  if (!collectors.length) throw new Error('Collector not found');
  process.stdout.write(JSON.stringify({ include: collectors.flatMap(collector => PLATFORM_TARGETS.map(platform => ({
    ...collector, ...platform, archive_name: archiveName(collector, platform),
  }))) }));
}
