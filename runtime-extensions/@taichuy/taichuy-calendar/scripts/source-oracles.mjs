import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { root, sourceRoot } from './toolchain.mjs';

/** Compile reference calculations from approved original source, never from migrated algorithms. */
export async function sourceOracles() {
  const inventory = JSON.parse(await readFile(resolve(root, 'migration/inventory.json'), 'utf8'));
  async function approved(path) {
    const text = await readFile(resolve(sourceRoot, path), 'utf8');
    const expected = inventory.files.find((item) => item.source === path)?.sourceSha256;
    const actual = createHash('sha256').update(text).digest('hex');
    if (!expected || expected !== actual) throw new Error(`Approved source has changed: ${path}`);
    return text;
  }
  function between(text, start, end) {
    const from = text.indexOf(start);
    const to = text.indexOf(end, from + start.length);
    if (from < 0 || to < 0) throw new Error(`Source oracle boundary missing: ${start}`);
    return text.slice(from, to);
  }
  const bazi = await approved('src/components/taichuy/divination/BaziCalculator.tsx');
  const ziwei = await approved('src/components/taichuy/divination/ZiWeiCalculator.tsx');
  const page = await approved('src/app/bazi/page.tsx');
  const baziBody = between(bazi, '        const solarTime = SolarTime.fromYmdHms(', '      } catch (error)');
  const ziweiBody = between(ziwei, '        const hour = time.hour();', '        setAstrolabe(result);');
  const solarFunction = between(page, 'function calculateTrueSolarTime(', '\nconst useStyle = createStyles(');
  return `import dayjs, { type Dayjs } from 'dayjs';
import { SolarTime, ChildLimit, Gender, type DecadeFortune } from 'tyme4ts';
import { astro } from 'iztro';
import type { IFunctionalAstrolabe } from 'iztro/lib/astro/FunctionalAstrolabe';
import type { BaziResult } from '${resolve(root, 'src/components/divination/types.ts').replaceAll('\\', '/')}';
import { SiderealTime, Body, Equator, Observer } from 'astronomy-engine';
export ${solarFunction}
export function sourceBazi(date: Dayjs, time: Dayjs, gender: 'male' | 'female') {
  let result: BaziResult | undefined;
  let selected = -1;
  const setBaziResult = (value: BaziResult) => { result = value; };
  const setSelectedDaYunIndex = (value: number) => { selected = value; };
  ${baziBody}
  return { result: result!, initialDaYunIndex: selected };
}
export function sourceZiWei(date: Dayjs, time: Dayjs, gender: 'male' | 'female') {
  ${ziweiBody}
  return result;
}
`;
}
