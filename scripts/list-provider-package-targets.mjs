import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(import.meta.dirname, '..');

function readManifestField(content, fieldName, fallback = '') {
  const escapedField = fieldName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = content.match(new RegExp(`^${escapedField}:\\s*(.+)$`, 'm'));
  return match ? match[1].trim() : fallback;
}

function readExecutablePath(content) {
  const lines = content.split(/\r?\n/);
  let insideExecutable = false;

  for (const line of lines) {
    if (/^runtime:\s*$/.test(line)) {
      insideExecutable = false;
      continue;
    }

    if (/^  executable:\s*$/.test(line)) {
      insideExecutable = true;
      continue;
    }

    if (insideExecutable) {
      const match = line.match(/^    path:\s*(.+)\s*$/);
      if (match) {
        return match[1].trim();
      }

      if (line && !line.startsWith('    ')) {
        break;
      }
    }
  }

  return '';
}

function toRelativePluginDir(pluginDir, baseRoot) {
  return path.relative(baseRoot, pluginDir).split(path.sep).join('/');
}

export function readProviderPackageTarget(pluginDir, baseRoot = repoRoot) {
  const resolvedPluginDir = path.resolve(pluginDir);
  const manifestPath = path.join(resolvedPluginDir, 'manifest.yaml');
  const manifest = fs.readFileSync(manifestPath, 'utf8');
  const providerCode =
    readManifestField(manifest, 'plugin_code') || path.basename(resolvedPluginDir);
  const executablePath = readExecutablePath(manifest);
  const binaryName = path.basename(executablePath || `bin/${providerCode}-provider`);

  return {
    provider_code: providerCode,
    plugin_dir: toRelativePluginDir(resolvedPluginDir, baseRoot),
    binary_name: binaryName,
  };
}

export function listProviderPackageTargets(rootDir = repoRoot) {
  const modelsDir = path.join(rootDir, 'models');
  if (!fs.existsSync(modelsDir)) {
    return [];
  }

  return fs
    .readdirSync(modelsDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => path.join(modelsDir, entry.name))
    .filter((pluginDir) => fs.existsSync(path.join(pluginDir, 'manifest.yaml')))
    .map((pluginDir) => readProviderPackageTarget(pluginDir, rootDir))
    .sort((left, right) => left.provider_code.localeCompare(right.provider_code));
}

function parseCliArgs(argv) {
  const options = {
    format: 'json',
    pluginDir: null,
    field: null,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = argv[index + 1];

    if (arg === '--format') {
      if (!next) {
        throw new Error('--format 需要值');
      }
      options.format = next;
      index += 1;
      continue;
    }

    if (arg === '--plugin-dir') {
      if (!next) {
        throw new Error('--plugin-dir 需要值');
      }
      options.pluginDir = next;
      index += 1;
      continue;
    }

    if (arg === '--field') {
      if (!next) {
        throw new Error('--field 需要值');
      }
      options.field = next;
      index += 1;
      continue;
    }

    throw new Error(`未知参数：${arg}`);
  }

  return options;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const options = parseCliArgs(process.argv.slice(2));
  const payload = options.pluginDir
    ? readProviderPackageTarget(path.resolve(repoRoot, options.pluginDir), repoRoot)
    : listProviderPackageTargets(repoRoot);

  if (options.field) {
    if (Array.isArray(payload)) {
      throw new Error('--field 只能与 --plugin-dir 一起使用');
    }

    const value = payload[options.field];
    if (typeof value !== 'string') {
      throw new Error(`未知字段：${options.field}`);
    }
    process.stdout.write(`${value}\n`);
  } else if (options.format === 'github-matrix') {
    process.stdout.write(`${JSON.stringify({ include: payload })}\n`);
  } else {
    process.stdout.write(`${JSON.stringify(payload, null, 2)}\n`);
  }
}
