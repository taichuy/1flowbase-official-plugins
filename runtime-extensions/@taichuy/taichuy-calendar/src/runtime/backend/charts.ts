import { calculateBazi } from '../../algorithms/bazi';
import { calculateZiWei } from '../../algorithms/ziwei';
import { calculateTrueSolarTime } from '../../algorithms/true-solar-time';
import { civil, normalizeBirth, fail } from './input';
export function trueSolar(input: any) {
  const dt = civil(input.civil_date, input.civil_time, input.utc_offset_minutes);
  const adjusted = calculateTrueSolarTime(dt, dt, input.longitude, input.latitude);
  if (!adjusted.isValid()) fail('calculation_failed', 'True solar time calculation failed');
  // DTO precision is seconds; no exposed floating-angle epsilon is needed.
  const wholeSecond = adjusted.millisecond(0);
  return { civil_date: wholeSecond.format('YYYY-MM-DD'), civil_time: wholeSecond.format('HH:mm:ss'), utc_offset_minutes: input.utc_offset_minutes, longitude: input.longitude, latitude: input.latitude, correction_seconds: Math.round((wholeSecond.valueOf() - dt.valueOf()) / 1000) };
}
export function bazi(input: any) {
  const birth = normalizeBirth(input);
  const dt = civil(input.civil_date, input.civil_time, input.utc_offset_minutes);
  const asOf = civil(input.as_of, '12:00:00', input.utc_offset_minutes);
  if (asOf.valueOf() < dt.startOf('day').valueOf()) fail('invalid_input', 'as_of must not precede the birth date');
  const { result, initialDaYunIndex } = calculateBazi(dt, dt, input.gender, asOf);
  // Explicitly project algorithm classes; never serialize methods or rely on toJSON.
  const { eightChar, daYun: _daYun, ...data } = result;
  const pillar = (p: any) => ({ gan_zhi: p.getName(), heavenly_stem: p.getHeavenStem().getName(), earthly_branch: p.getEarthBranch().getName() });
  return { birth, as_of: input.as_of, ...data, eightChar: { year: pillar(eightChar.getYear()), month: pillar(eightChar.getMonth()), day: pillar(eightChar.getDay()), hour: pillar(eightChar.getHour()) }, initialDaYunIndex };
}
const select = (value: any, keys: string[]) => Object.fromEntries(keys.map(key => [key, value[key] ?? null]));
const star = (s: any) => select(s, ['name', 'type', 'scope', 'brightness', 'mutagen']);
function horoscopePeriod(p: any) {
  if (!p) return null;
  return { ...select(p, ['index', 'name', 'heavenlyStem', 'earthlyBranch', 'palaceNames', 'mutagen', 'nominalAge', 'yearlyDecStar']), stars: p.stars ? p.stars.map((stars: any[]) => stars.map(star)) : null };
}
export function ziwei(input: any) {
  const birth = normalizeBirth(input);
  const dt = civil(input.civil_date, input.civil_time, input.utc_offset_minutes);
  const asOf = civil(input.as_of, input.as_of_time, input.utc_offset_minutes);
  if (asOf.valueOf() < dt.startOf('day').valueOf()) fail('invalid_input', 'as_of must not precede the birth date');
  // iztro 2.5.3 lib/calendar/convertor.js: solar2lunar's actual inclusive bounds.
  if (dt.year() < 1900 || dt.year() > 2100 || (dt.year() === 1900 && (dt.month() === 0 && dt.date() < 31)) || asOf.year() < 1900 || asOf.year() > 2100 || (asOf.year() === 1900 && asOf.month() === 0 && asOf.date() < 31)) fail('invalid_date', 'Ziwei requires dates from 1900-01-31 through 2100-12-31 (iztro 2.5.3)');
  const a = calculateZiWei(dt, dt, input.gender);
  // Iztro's object contains enumerable methods and astrolabe cycles; project the public chart DTO only.
  const h = a.horoscope(asOf.format('YYYY-M-D'), Math.floor((asOf.hour() + 1) / 2) % 12);
  return { birth, ...select(a, ['gender', 'solarDate', 'lunarDate', 'chineseDate', 'time', 'timeRange', 'sign', 'zodiac', 'earthlyBranchOfBodyPalace', 'earthlyBranchOfSoulPalace', 'soul', 'body', 'fiveElementsClass', 'copyright']), palaces: a.palaces.map(p => ({ ...select(p, ['index', 'name', 'isBodyPalace', 'isOriginalPalace', 'heavenlyStem', 'earthlyBranch', 'changsheng12', 'boshi12', 'jiangqian12', 'suiqian12', 'decadal', 'ages']), majorStars: p.majorStars.map(star), minorStars: p.minorStars.map(star), adjectiveStars: p.adjectiveStars.map(star) })), horoscope: { as_of: input.as_of, as_of_time: input.as_of_time, lunarDate: h.lunarDate, solarDate: h.solarDate, decadal: horoscopePeriod(h.decadal), age: horoscopePeriod(h.age), yearly: horoscopePeriod(h.yearly), monthly: horoscopePeriod(h.monthly), daily: horoscopePeriod(h.daily), hourly: horoscopePeriod(h.hourly) } };
}
