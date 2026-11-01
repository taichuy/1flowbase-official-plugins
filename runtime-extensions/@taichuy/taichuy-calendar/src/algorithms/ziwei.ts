import type { Dayjs } from 'dayjs';
import { astro } from 'iztro';
import type { IFunctionalAstrolabe } from 'iztro/lib/astro/FunctionalAstrolabe';

/** Retains the source's hour-index policy, including folding 23:00 to index zero. */
export function calculateZiWei(date: Dayjs, time: Dayjs, gender: 'male' | 'female'): IFunctionalAstrolabe {
  const hour = time.hour();
  let timeIndex = Math.floor((hour + 1) / 2);
  if (timeIndex >= 12) timeIndex = 0;
  const solarDateStr = date.format('YYYY-M-D');
  return astro.bySolar(solarDateStr, timeIndex, gender === 'male' ? '男' : '女', true, 'zh-CN') as unknown as IFunctionalAstrolabe;
}
