import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(import.meta.dirname, '..');

function readManifestField(content, fieldName, fallback = '') {
  const escapedField = fieldName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = content.match(new RegExp(`^${escapedField}:\\s*(.+)$`, 'm'));
  return match ? match[1].trim() : fallback;
}

function replaceManifestField(content, fieldName, nextValue) {
  const escapedField = fieldName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return content.replace(new RegExp(`^${escapedField}:\\s*.+$`, 'm'), `${fieldName}: ${nextValue}`);
}

function resolveProviderCode(pluginId, fallbackProviderCode = '') {
  if (!pluginId) {
    return fallbackProviderCode;
  }

  if (pluginId.includes('@')) {
    return pluginId.slice(0, pluginId.indexOf('@')) || fallbackProviderCode;
  }

  return pluginId;
}

export function syncManifestContent(content, options = {}) {
  const manifestVersion = readManifestField(content, 'manifest_version');
  const version = readManifestField(content, 'version');
  const currentPluginId = readManifestField(content, 'plugin_id');

  if (manifestVersion !== '1' || !version || !currentPluginId) {
    return {
      changed: false,
      content,
      manifestVersion,
      version,
      currentPluginId,
      nextPluginId: currentPluginId,
      providerCode: resolveProviderCode(currentPluginId, options.fallbackProviderCode || ''),
    };
  }

  const providerCode = resolveProviderCode(currentPluginId, options.fallbackProviderCode || '');
  const nextPluginId = `${providerCode}@${version}`;
  if (!providerCode || currentPluginId === nextPluginId) {
    return {
      changed: false,
      content,
      manifestVersion,
      version,
      currentPluginId,
      nextPluginId: currentPluginId,
      providerCode,
    };
  }

  return {
    changed: true,
    content: replaceManifestField(content, 'plugin_id', nextPluginId),
    manifestVersion,
    version,
    currentPluginId,
    nextPluginId,
    providerCode,
  };
}

export function syncProviderManifestFile(manifestPath, options = {}) {
  const content = fs.readFileSync(manifestPath, 'utf8');
  const result = syncManifestContent(content, {
    fallbackProviderCode:
      options.fallbackProviderCode || path.basename(path.dirname(manifestPath)),
  });

  if (result.changed) {
    fs.writeFileSync(manifestPath, result.content);
  }

  return {
    ...result,
    manifestPath,
  };
}

export function syncProviderManifestVersions(rootDir = repoRoot) {
  const providersDir = path.join(rootDir, 'runtime-extensions', 'model-providers');
  if (!fs.existsSync(providersDir)) {
    return {
      scannedFiles: [],
      changedFiles: [],
    };
  }

  const manifestPaths = fs
    .readdirSync(providersDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => path.join(providersDir, entry.name, 'manifest.yaml'))
    .filter((manifestPath) => fs.existsSync(manifestPath))
    .sort((left, right) => left.localeCompare(right));

  const results = manifestPaths.map((manifestPath) => syncProviderManifestFile(manifestPath));

  return {
    scannedFiles: results.map((result) => result.manifestPath),
    changedFiles: results.filter((result) => result.changed).map((result) => result.manifestPath),
  };
}

function parseCliArgs(argv) {
  const options = {
    rootDir: repoRoot,
    pluginDir: null,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = argv[index + 1];

    if (arg === '--plugin-dir') {
      if (!next) {
        throw new Error('--plugin-dir 需要值');
      }
      options.pluginDir = next;
      index += 1;
      continue;
    }

    if (arg === '--root-dir') {
      if (!next) {
        throw new Error('--root-dir 需要值');
      }
      options.rootDir = path.resolve(repoRoot, next);
      index += 1;
      continue;
    }

    throw new Error(`未知参数：${arg}`);
  }

  return options;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const options = parseCliArgs(process.argv.slice(2));

  if (options.pluginDir) {
    const manifestPath = path.join(path.resolve(repoRoot, options.pluginDir), 'manifest.yaml');
    const result = syncProviderManifestFile(manifestPath);
    process.stdout.write(
      `${result.changed ? 'Synced' : 'Already aligned'} ${path.relative(repoRoot, manifestPath)}\n`
    );
  } else {
    const result = syncProviderManifestVersions(options.rootDir);
    process.stdout.write(
      `Scanned ${result.scannedFiles.length} manifest file(s); synced ${result.changedFiles.length}.\n`
    );
  }
}
