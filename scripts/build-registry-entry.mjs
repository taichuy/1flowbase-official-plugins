import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const OFFICIAL_PLUGIN_RAW_BASE_URL =
  'https://raw.githubusercontent.com/taichuy/1flowbase-official-plugins/main';

function readField(content, fieldName, fallback = '') {
  const escapedField = fieldName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = content.match(new RegExp(`^${escapedField}:\\s*(.+)$`, 'm'));
  return match ? match[1].trim() : fallback;
}

function nullableField(content, fieldName) {
  const value = readField(content, fieldName, '');
  return value || null;
}

function parseManifestMetadata(content) {
  const lines = content.split(/\r?\n/);
  const metadata = {
    label: {},
    description: {},
  };
  let currentSection = null;

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];

    if (/^metadata:\s*$/.test(line)) {
      for (let cursor = index + 1; cursor < lines.length; cursor += 1) {
        const nested = lines[cursor];
        if (!nested.trim()) {
          continue;
        }
        if (!nested.startsWith('  ')) {
          break;
        }
        const sectionMatch = nested.match(/^  (label|description):\s*$/);
        if (sectionMatch) {
          currentSection = sectionMatch[1];
          continue;
        }
        const localeMatch = nested.match(/^    ([^:]+):\s*(.+)\s*$/);
        if (currentSection && localeMatch) {
          metadata[currentSection][localeMatch[1].trim()] = localeMatch[2].trim();
        }
      }
      break;
    }
  }

  return metadata;
}

function buildI18nSummary(i18nDir) {
  if (!fs.existsSync(i18nDir)) {
    return {
      default_locale: null,
      available_locales: [],
      bundles: {},
    };
  }

  const availableLocales = fs
    .readdirSync(i18nDir)
    .filter((name) => name.endsWith('.json'))
    .map((name) => path.basename(name, '.json'))
    .sort((left, right) => left.localeCompare(right));
  const defaultLocale = availableLocales.includes('en_US') ? 'en_US' : (availableLocales[0] ?? null);
  const bundles = Object.fromEntries(
    availableLocales.map((locale) => [
      locale,
      JSON.parse(fs.readFileSync(path.join(i18nDir, `${locale}.json`), 'utf8')),
    ])
  );

  return {
    default_locale: defaultLocale,
    available_locales: availableLocales,
    bundles,
  };
}

function applyManifestMetadataToI18nSummary(i18nSummary, manifestMetadata) {
  const locales = new Set([
    ...Object.keys(i18nSummary?.bundles || {}),
    ...Object.keys(manifestMetadata.label || {}),
    ...Object.keys(manifestMetadata.description || {}),
  ]);

  for (const locale of locales) {
    const existingBundle = i18nSummary.bundles[locale];
    const nextBundle =
      existingBundle && typeof existingBundle === 'object' && !Array.isArray(existingBundle)
        ? { ...existingBundle }
        : {};
    const existingPlugin =
      nextBundle.plugin && typeof nextBundle.plugin === 'object' && !Array.isArray(nextBundle.plugin)
        ? { ...nextBundle.plugin }
        : {};

    if (manifestMetadata.label[locale]) {
      existingPlugin.label = manifestMetadata.label[locale];
    }
    if (manifestMetadata.description[locale]) {
      existingPlugin.description = manifestMetadata.description[locale];
    }

    if (Object.keys(existingPlugin).length > 0) {
      nextBundle.plugin = existingPlugin;
    }

    i18nSummary.bundles[locale] = nextBundle;
  }

  i18nSummary.available_locales = Array.from(locales).sort((left, right) =>
    left.localeCompare(right)
  );
  if (!i18nSummary.default_locale && i18nSummary.available_locales.length > 0) {
    i18nSummary.default_locale = i18nSummary.available_locales.includes('en_US')
      ? 'en_US'
      : i18nSummary.available_locales[0];
  }

  return i18nSummary;
}

function compareArtifacts(left, right) {
  return [
    left.os || '',
    left.arch || '',
    left.libc || '',
    left.rust_target || '',
  ]
    .join(':')
    .localeCompare(
      [
        right.os || '',
        right.arch || '',
        right.libc || '',
        right.rust_target || '',
      ].join(':')
    );
}

function isExternalAssetUrl(value) {
  return /^https?:\/\//.test(value) || value.startsWith('data:');
}

function toPosixPath(value) {
  return value.split(path.sep).join('/');
}

function resolveRegistryIcon(pluginDir, manifestIcon) {
  const trimmedIcon = manifestIcon.trim();
  if (!trimmedIcon) {
    return null;
  }

  if (isExternalAssetUrl(trimmedIcon)) {
    return trimmedIcon;
  }

  const assetPath = [
    path.join(pluginDir, '_assets', trimmedIcon),
    path.join(pluginDir, trimmedIcon),
  ].find((candidatePath) => fs.existsSync(candidatePath));
  if (!assetPath) {
    return null;
  }

  const relativeAssetPath = path.relative(repoRoot, assetPath);
  if (relativeAssetPath.startsWith('..')) {
    return null;
  }

  return `${OFFICIAL_PLUGIN_RAW_BASE_URL}/${toPosixPath(relativeAssetPath)}`;
}

export function buildRegistryEntry({ pluginDir, providerCode, version, artifacts }) {
  const manifest = fs.readFileSync(path.join(pluginDir, 'manifest.yaml'), 'utf8');
  const pluginType = readField(manifest, 'plugin_type', 'model_provider');
  const manifestMetadata = parseManifestMetadata(manifest);
  const icon = resolveRegistryIcon(pluginDir, readField(manifest, 'icon', ''));
  const providerPath = path.join(pluginDir, 'provider', `${providerCode}.yaml`);
  const providerYaml = fs.readFileSync(providerPath, 'utf8');
  const i18nSummary = applyManifestMetadataToI18nSummary(
    buildI18nSummary(path.join(pluginDir, 'i18n')),
    manifestMetadata
  );

  return {
    plugin_id: `1flowbase.${providerCode}`,
    plugin_type: pluginType,
    provider_code: providerCode,
    display_name:
      readField(providerYaml, 'display_name', '') ||
      i18nSummary.bundles[i18nSummary.default_locale]?.provider?.label ||
      providerCode,
    icon,
    protocol: readField(providerYaml, 'protocol', providerCode),
    latest_version: version,
    help_url: nullableField(providerYaml, 'help_url'),
    model_discovery_mode: readField(providerYaml, 'model_discovery', 'hybrid'),
    i18n_summary: i18nSummary,
    artifacts: [...(Array.isArray(artifacts) ? artifacts : [])].sort(compareArtifacts),
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const pluginDir = process.argv[2];
  const providerCode = process.argv[3];
  const version = process.argv[4];
  const artifactsJson = process.argv[5];

  if (!pluginDir || !providerCode || !version || !artifactsJson) {
    throw new Error(
      'Usage: node scripts/build-registry-entry.mjs <plugin-dir> <provider-code> <version> <artifacts-json>'
    );
  }

  const entry = buildRegistryEntry({
    pluginDir,
    providerCode,
    version,
    artifacts: JSON.parse(artifactsJson),
  });
  process.stdout.write(`${JSON.stringify(entry)}\n`);
}
