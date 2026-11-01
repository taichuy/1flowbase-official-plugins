import { Lunar, Solar } from 'lunar-typescript';
import { SolarDay } from 'tyme4ts';
import { astro } from 'iztro';
import { MakeTime, SunPosition } from 'astronomy-engine';
import china, { provinces, cities, areas, pc, pcC, pca, pcaC } from 'china-division';
import type { LunarDate } from 'lunar-lite/lib/types';
import type { i18n } from 'i18next';
import dayjs from 'dayjs';

type IsAny<T> = 0 extends (1 & T) ? true : false;
type NotAny<T extends false> = T;
export type LunarIsTyped = NotAny<IsAny<ReturnType<typeof Lunar.fromYmd>>>;
export type SolarIsTyped = NotAny<IsAny<ReturnType<typeof SolarDay.fromYmd>>>;
const lunar = Solar.fromYmd(2024, 2, 10).getLunar();
const year: number = lunar.getYear();
const tymeLunar = SolarDay.fromYmd(2024, 2, 10).getLunarDay();
const tymeMonth: number = tymeLunar.getMonth();
const chart = astro.bySolar('2024-2-10', 2, '男', true, 'zh-CN');
export type ChartIsTyped = NotAny<IsAny<typeof chart>>;
export type NestedLunarIsTyped = NotAny<IsAny<typeof chart.rawDates.lunarDate>>;
const rawDate: LunarDate = chart.rawDates.lunarDate;
const lunarYear: number = rawDate.lunarYear;
const palaceName: string = chart.palaces[0].name;
const horoscope = chart.horoscope(dayjs('2026-11-01').toDate());
export type HoroscopeIsTyped = NotAny<IsAny<typeof horoscope>>;
const longitude: number = SunPosition(MakeTime(new Date())).elon;
const provinceCode: string = provinces[0].code;
const parentProvince: string = cities[0].provinceCode;
const parentCity: string = areas[0].cityCode;
const cityName: string = pc['北京市'][0];
const areaName: string = pca['北京市']['市辖区'][0];
const twoLevelCode: string = pcC[0].children[0].code;
const threeLevelCode: string = pcaC[0].children[0].children[0].code;
const defaultAreas: typeof areas = china.areas;
export type I18nIsTyped = NotAny<IsAny<i18n>>;

// These must remain errors: permissive placeholder types would make TS2578 fail this fixture.
// @ts-expect-error calendar years are numeric
const badYear: string = year;
// @ts-expect-error tyme returns a typed calendar object
const missingTyme = tymeLunar.notAnUpstreamMethod();
// @ts-expect-error complete chart typing must reject nonexistent fields
const missingChart = chart.notAnUpstreamField;
// @ts-expect-error transitive lunar-lite declaration is not any
const badRawYear: string = chart.rawDates.lunarDate.lunarYear;
// @ts-expect-error astronomy positions are numeric
const badLongitude: string = longitude;
// @ts-expect-error numeric division codes lose leading zeroes and are not the upstream shape
const badCode: number = pcaC[0].children[0].children[0].code;
// @ts-expect-error pcC is precisely two levels, not an arbitrary recursive tree
const excessLevel = pcC[0].children[0].children;
// @ts-expect-error streets are deliberately outside the browser namespace
const streets = china.streets;
// @ts-expect-error villages are deliberately outside the browser namespace
import { villages } from 'china-division';
// @ts-expect-error no JSON or other china-division subpath is registered for consumers
import villageData from 'china-division/dist/villages.json';
void [year, tymeMonth, lunarYear, palaceName, horoscope, longitude, provinceCode, parentProvince,
  parentCity, cityName, areaName, twoLevelCode, threeLevelCode, defaultAreas];
