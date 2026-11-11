import { test } from 'node:test';
import assert from 'node:assert/strict';
import dayjs from 'dayjs';
import { Lunar } from 'lunar-typescript';
import { calculateBazi } from '../algorithms/bazi';
import { calculateZiWei } from '../algorithms/ziwei';
import { calculateTrueSolarTime } from '../algorithms/true-solar-time';
import { sourceBazi, sourceZiWei, calculateTrueSolarTime as sourceSolarTime } from 'approved-source-oracles';
import { generateBaziAIDescription } from '../components/divination/baziPrompts';
import { generateZiWeiAIDescription, generateZiWeiAIDescriptionWithHoroscope } from '../components/divination/ziweiPrompts';
import expected from './source-expectations.json';

// Both original and migrated code see the same clock. Not production clock replacement.
const NativeDate = Date;
const fixedNow = new NativeDate(expected.clock).getTime();
class FixedDate extends NativeDate {
  constructor(...args: any[]) { super(...(args.length ? args : [fixedNow]) as [any]); }
  static now() { return fixedNow; }
}

function normalizeBazi(calculation: ReturnType<typeof calculateBazi>) {
  const { eightChar, daYun, ...plain } = calculation.result;
  return { ...plain, pillars: [eightChar.getYear(), eightChar.getMonth(), eightChar.getDay(), eightChar.getHour()].map((item) => item.getName()),
    daYun: daYun.map((item) => item.getName()), initialDaYunIndex: calculation.initialDaYunIndex };
}
function normalizeZiWei(result: ReturnType<typeof calculateZiWei>, viewDate: string) {
  // Keep all natal data, star groups, decadal, ages and current horoscope on BOTH sides.
  // FunctionalPalace._astrolabe is a private reverse link to its owning astrolabe,
  // not additional chart data. Exclude it alongside the existing astrolabe link
  // and methods; do not use a generic cycle filter that could hide data differences.
  return JSON.parse(JSON.stringify({ ...result, horoscope: result.horoscope(dayjs(viewDate).toDate()) },
    (key, value) => key === 'astrolabe' || key === '_astrolabe' || typeof value === 'function' ? undefined : value));
}
const cases = [
  ['2026-11-01', '09:02:00', 'male'],
  ['1990-01-01', '00:00:00', 'female'],
  ['2000-02-29', '23:00:00', 'male'],
  ['1988-08-08', '22:59:00', 'female'],
  ['2024-02-04', '16:27:00', 'male'], // solar-term vicinity
  ['2023-03-22', '01:00:00', 'female'], // lunar leap-month boundary
] as const;

test('migrated Bazi/ZiWei agree with approved original calculations across birth boundaries', () => {
  (globalThis as any).Date = FixedDate;
  try {
    assert.equal(Intl.DateTimeFormat().resolvedOptions().timeZone, 'Asia/Shanghai', 'Run fixture with TZ=Asia/Shanghai');
    for (const [dateText, timeText, gender] of cases) {
      const date = dayjs(dateText);
      const time = dayjs(`2000-01-01T${timeText}`);
      assert.deepEqual(normalizeBazi(calculateBazi(date, time, gender)), normalizeBazi(sourceBazi(date, time, gender)), `${dateText} ${timeText} ${gender}: Bazi`);
      assert.deepEqual(normalizeZiWei(calculateZiWei(date, time, gender), expected.clock),
        normalizeZiWei(sourceZiWei(date, time, gender), expected.clock), `${dateText} ${timeText} ${gender}: ZiWei`);
      for (const [longitude, latitude] of [[116.4, 39.9], [87.62, 43.82], [121.47, 31.23], [-74.0, 40.7]]) {
        const actual = calculateTrueSolarTime(date, time, longitude, latitude);
        const source = sourceSolarTime(date, time, longitude, latitude);
        assert.equal(actual.valueOf(), source.valueOf(), `true solar time ${dateText}/${timeText}/${longitude}/${latitude}`);
      }
    }
  } finally { (globalThis as any).Date = NativeDate; }
});

test('captured source-site 2026-11-01 evidence is retained, including full Bazi luck tables and prompts', () => {
  const date = dayjs(expected.birthDate);
  const time = dayjs(`2000-01-01T${expected.time}`);
  const lunar = Lunar.fromDate(date.toDate());
  assert.equal(lunar.getMonthInChinese(), expected.almanac.lunarMonth);
  assert.equal(lunar.getDayInChinese(), expected.almanac.lunarDay);
  assert.equal(lunar.getYearInGanZhi(), expected.almanac.year);
  assert.equal(lunar.getMonthInGanZhi(), expected.almanac.month);
  assert.equal(lunar.getDayInGanZhi(), expected.almanac.day);
  assert.equal(lunar.getDayNaYin(), expected.almanac.naYin);
  expected.almanac.yi.forEach((item) => assert.ok(lunar.getDayYi().includes(item)));
  expected.almanac.ji.forEach((item) => assert.ok(lunar.getDayJi().includes(item)));
  const { result } = calculateBazi(date, time, expected.gender as 'male', dayjs(expected.clock));
  const normalized = normalizeBazi({ result, initialDaYunIndex: 0 });
  assert.deepEqual(normalized.pillars, expected.bazi.pillars);
  assert.deepEqual(result.wuxingCounts, expected.bazi.wuxingCounts);
  assert.equal(result.dayMaster, expected.bazi.dayMaster);
  assert.equal(result.geJu, expected.bazi.geJu);
  assert.equal(result.daYunDetails.length, 8);
  const first = result.daYunDetails[0];
  assert.equal(first.ganZhi, expected.bazi.firstDaYun);
  assert.equal(first.startYear, expected.bazi.firstYear);
  assert.equal(first.endYear, expected.bazi.lastYear);
  for (const decade of result.daYunDetails) {
    assert.equal(decade.years.length, 10);
    for (const year of decade.years) assert.ok(year.months.length >= 12);
  }
  const ziwei = calculateZiWei(date, time, expected.gender as 'male');
  for (const key of ['fiveElementsClass', 'soul', 'body', 'earthlyBranchOfSoulPalace', 'earthlyBranchOfBodyPalace'] as const) {
    assert.equal(ziwei[key], expected.ziwei[key]);
  }
  assert.equal(ziwei.palaces.length, expected.ziwei.palaceCount);
  const civilDateTime = date.hour(time.hour()).minute(time.minute()).second(time.second());
  const selectedTimeLine = `- 当前时间：${civilDateTime.format('YYYY年MM月DD日 HH:mm:ss')}`;
  const baziText = generateBaziAIDescription(result, civilDateTime);
  const ziweiText = generateZiWeiAIDescription(ziwei, civilDateTime, undefined, undefined, 'male');
  const horoscopeText = generateZiWeiAIDescriptionWithHoroscope(ziwei, ziwei.horoscope(dayjs(expected.clock).toDate()), civilDateTime, undefined, undefined, 'male');
  for (const text of [baziText, ziweiText, horoscopeText]) assert.ok(text.includes(selectedTimeLine));
  assert.match(baziText, /丙午/);
  assert.match(ziweiText, /紫微/);
  assert.match(horoscopeText, /紫微/);
});

test('AI helper headers use the selected civil date rather than the system or corrected date', () => {
  const selected = dayjs('2025-11-26T22:30:00');
  const corrected = dayjs('2025-11-27T00:01:00');
  const location = { longitude: 116.4, latitude: 39.9, useTrueSolarTime: true, trueSolarTimeStr: corrected.format('YYYY-MM-DD HH:mm:ss') };
  const { result } = calculateBazi(corrected, corrected, 'male', dayjs(expected.clock));
  const ziwei = calculateZiWei(corrected, corrected, 'male');
  const texts = [
    generateBaziAIDescription(result, selected, location),
    generateZiWeiAIDescription(ziwei, selected, location),
    generateZiWeiAIDescriptionWithHoroscope(ziwei, ziwei.horoscope(dayjs(expected.clock).toDate()), selected, location),
  ];
  for (const text of texts) {
    assert.ok(text.includes('- 当前时间：2025年11月26日 22:30:00'));
    assert.ok(text.includes('- 当前农历：二〇二五年 十月 初七'));
    assert.ok(text.includes('- 真太阳时时间：2025-11-27 00:01:00'));
  }
});
