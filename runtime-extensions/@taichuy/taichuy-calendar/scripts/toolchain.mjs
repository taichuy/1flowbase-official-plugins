import { createRequire } from 'node:module';
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { businessDependencies } from './module-contract.mjs';

export const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const hostRoot = process.env.TAICHUY_HOST_ROOT ?? '/home/taichuy/git/1flowbase';
export const sourceRoot = process.env.TAICHUY_SOURCE_ROOT ?? '/home/taichuy/git/taichu-y-web-master';
export const sourceNodeModules = resolve(sourceRoot, 'node_modules');

/** Resolve existing tools only. This deliberately never spawns npm/pnpm/npx or installs packages. */
export async function loadEsbuild() {
  if (process.env.ESBUILD_MODULE) return createRequire(import.meta.url)(resolve(process.env.ESBUILD_MODULE));
  const roots = [root, resolve(hostRoot, 'web/app'), resolve(hostRoot, 'web'), sourceRoot];
  for (const base of roots) {
    try { return createRequire(resolve(base, 'package.json'))('esbuild'); }
    catch (error) { if (error.code !== 'MODULE_NOT_FOUND') throw error; }
  }
  throw new Error('No existing esbuild module found; set ESBUILD_MODULE to a local esbuild/lib/main.js. No dependencies were installed.');
}

export async function verifyBusinessVersions() {
  for (const [name, version] of Object.entries(businessDependencies)) {
    const metadata = JSON.parse(await readFile(resolve(sourceNodeModules, name, 'package.json'), 'utf8'));
    if (metadata.version !== version) throw new Error(`${name}: expected approved ${version}, found ${metadata.version}`);
  }
}

/** Import test tools from the existing host installation; never install or download. */
export function loadHostTool(name) {
  for (const base of [resolve(hostRoot, 'web'), resolve(hostRoot, 'web/app')]) {
    try { return createRequire(resolve(base, 'package.json'))(name); }
    catch (error) { if (error.code !== 'MODULE_NOT_FOUND') throw error; }
  }
  throw new Error(`Missing existing host tool ${name}; provide TAICHUY_HOST_ROOT. No installation attempted.`);
}
