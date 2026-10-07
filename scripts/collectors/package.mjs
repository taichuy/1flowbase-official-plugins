import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { listCollectors, PLATFORM_TARGETS, archiveName } from './catalog.mjs';

const [code, target, binaryPath, outputDirectory = 'dist/collectors'] = process.argv.slice(2);
const repoRoot = path.resolve(import.meta.dirname, '../..');
const collector = listCollectors(repoRoot).find(item => item.collector_code === code);
const platform = PLATFORM_TARGETS.find(item => item.rust_target === target);
if (!collector || !platform || !binaryPath || !fs.statSync(binaryPath).isFile()) throw new Error('Invalid collector package arguments');
const stage = fs.mkdtempSync(path.join(os.tmpdir(), 'collector-package-'));
const output = path.resolve(outputDirectory);
fs.mkdirSync(output, { recursive: true });
try {
  const executable = collector.entry + (platform.os === 'windows' ? '.exe' : '');
  fs.copyFileSync(binaryPath, path.join(stage, executable));
  fs.chmodSync(path.join(stage, executable), 0o755);
  for (const file of ['collector-manifest.json', 'README.md', 'README.en.md']) {
    fs.copyFileSync(path.join(repoRoot, collector.plugin_dir, file), path.join(stage, file));
  }
  const archive = path.join(output, archiveName(collector, platform));
  if (platform.os === 'windows') {
    // Windows runners provide the native archive cmdlet; users need no Node.js.
    const quote = value => `'${value.replaceAll("'", "''")}'`;
    execFileSync('powershell.exe', ['-NoProfile', '-Command', `Compress-Archive -Path ${quote(path.join(stage, '*'))} -DestinationPath ${quote(archive)} -Force`], { stdio: 'inherit' });
  } else {
    execFileSync('tar', ['-czf', archive, '-C', stage, '.'], { stdio: 'inherit' });
  }
  process.stdout.write(`${archive}\n`);
} finally {
  fs.rmSync(stage, { recursive: true, force: true });
}
