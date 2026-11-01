import { readFile, readdir, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { root } from './toolchain.mjs';

/** Keep full upstream license texts for every bundled package, not just minifier-preserved notices. */
export async function writeBundledLicenses(inputs) {
  const packages = new Map();
  for (const input of Object.keys(inputs)) {
    if (!input.includes('node_modules/')) continue;
    let directory = dirname(resolve(root, input));
    while (directory !== dirname(directory)) {
      try {
        const metadata = JSON.parse(await readFile(resolve(directory, 'package.json'), 'utf8'));
        if (metadata.name && metadata.version) packages.set(`${metadata.name}@${metadata.version}`, { directory, metadata });
        break;
      } catch (error) {
        if (error.code !== 'ENOENT') throw error;
        directory = dirname(directory);
      }
    }
  }
  const sections = [];
  for (const [identity, { directory, metadata }] of [...packages].sort(([a], [b]) => a.localeCompare(b))) {
    const files = (await readdir(directory)).filter((file) => /^(licen[cs]e|copying|notice)(\.|$)/i.test(file)).sort();
    const text = await Promise.all(files.map(async (file) => `--- ${file} ---\n${await readFile(resolve(directory, file), 'utf8')}`));
    if (!text.length && metadata.name === 'astronomy-engine') {
      // This npm tarball ships its complete MIT notice in the source header, not LICENSE.
      const source = await readFile(resolve(directory, 'astronomy.js'), 'utf8');
      const notice = source.match(/^\/\*\*[\s\S]*?\*\//)?.[0];
      if (!notice?.includes('Permission is hereby granted') || !notice.includes('THE SOFTWARE IS PROVIDED')) {
        throw new Error('astronomy-engine full MIT source notice is missing');
      }
      text.push(`--- astronomy.js @preserve header ---\n${notice}`);
    }
    if (!text.length && identity === 'toggle-selection@1.0.6') {
      // The npm tarball omits LICENSE; retain the upstream copyright-bearing text.
      text.push('--- upstream LICENSE (see vendored provenance) ---\n' + await readFile(resolve(root, '_assets/licenses/toggle-selection-1.0.6.LICENSE'), 'utf8'));
    }
    if (!text.length) throw new Error(`Bundled package ${identity} has no license text; stop publication and resolve attribution`);
    sections.push(`## ${identity}\nDeclared license: ${JSON.stringify(metadata.license ?? metadata.licenses ?? 'See upstream text')}\n\n${text.join('\n')}`);
  }
  await writeFile(resolve(root, '_assets/ui/THIRD_PARTY_LICENSES.txt'),
    'Third-party packages bundled into the Taichuy Calendar frontend. Host-provided packages are not bundled.\n\n' + sections.join('\n\n') + '\n');
}
