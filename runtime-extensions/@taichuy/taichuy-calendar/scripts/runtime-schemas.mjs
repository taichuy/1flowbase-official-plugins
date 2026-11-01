import { object, civilDate, civilTime } from './runtime-contract.mjs';
const s = { type: 'string' }, i = { type: 'integer' }, n = { type: 'number' }, b = { type: 'boolean' };
const array = items => ({ type: 'array', items });
const nullable = schema => ({ anyOf: [schema, { type: 'null' }] });
const strings = array(s), integers = array(i), dict = schema => ({ type: 'object', additionalProperties: schema });
const lunar = object({ year: i, month: i, day: i, is_leap_month: b, month_text: s, day_text: s, year_gan_zhi: s, month_gan_zhi: s, day_gan_zhi: s, zodiac: s });
const dateFields = { civil_date: civilDate, weekday: i, lunar, solar_term: s, festivals: strings };
const location = object({ codes: strings, labels: strings, longitude: nullable(n), latitude: nullable(n), coordinate_code: nullable(s) });
const birth = object({ civil_date: civilDate, civil_time: civilTime, utc_offset_minutes: i, utc_datetime: s, gender: { enum: ['male', 'female'] }, location });
const monthFortune = object({ month: s, ganZhi: s, wuxing: s, tenGod: s });
const yearFortune = object({ age: i, year: i, ganZhi: s, wuxing: s, tenGod: s, xiaoYun: s, xiaoYunGanZhi: s, xiaoYunWuXing: s, xiaoYunTenGod: s, months: array(monthFortune) });
const decadeFortune = object({ name: s, ganZhi: s, wuxing: s, startYear: i, endYear: i, startAge: i, endAge: i, tenGod: s, years: array(yearFortune) });
const pillar = object({ gan_zhi: s, heavenly_stem: s, earthly_branch: s });
const star = object({ name: s, type: s, scope: s, brightness: nullable(s), mutagen: nullable(s) });
const period = nullable(object({ index: i, name: s, heavenlyStem: s, earthlyBranch: s, palaceNames: strings, mutagen: strings, nominalAge: nullable(i), yearlyDecStar: nullable(object({ suiqian12: strings, jiangqian12: strings })), stars: nullable(array(array(star))) }));
export const dataSchemas = {
  'calendar.month': object({ year: i, month: i, first_weekday: i, days: array(object(dateFields)) }),
  'calendar.convert': object(dateFields),
  'almanac.day': object({ ...dateFields, yi: strings, ji: strings, na_yin: s, chong: s, sha: s, zhi_xing: s, tian_shen_type: s, duty: s, xiu: s, xiu_luck: s, ji_shen: strings, xiong_sha: strings, tai_shen: s, peng_zu: strings, time_luck: array(object({ branch: s, hour: i, luck: s })) }),
  'birth.normalize': birth,
  'time.true-solar': object({ civil_date: civilDate, civil_time: civilTime, utc_offset_minutes: i, longitude: n, latitude: n, correction_seconds: i }),
  'bazi.calculate': object({ birth, as_of: civilDate, solarDate: s, lunarDate: s, gender: s, age: i, chineseAge: i, dayMaster: s, daYunDetails: array(decadeFortune), wuxingCounts: dict(i), wuxingDetail: dict(object({ count: i, sources: strings })), hiddenStems: dict(strings), tenGods: dict(s), tenGodsWuxing: dict(s), xunKong: s, shenSha: strings, geJu: s, taiYuan: s, mingGong: s, shenGong: s, qiyun: s, huangDao: object({ duty: s, twelveStar: s, ecliptic: s, gods: strings }), eightChar: object({ year: pillar, month: pillar, day: pillar, hour: pillar }), initialDaYunIndex: i }),
  'ziwei.calculate': object({ birth, gender: s, solarDate: s, lunarDate: s, chineseDate: s, time: s, timeRange: s, sign: s, zodiac: s, earthlyBranchOfBodyPalace: s, earthlyBranchOfSoulPalace: s, soul: s, body: s, fiveElementsClass: s, copyright: s, palaces: array(object({ index: i, name: s, isBodyPalace: b, isOriginalPalace: b, heavenlyStem: s, earthlyBranch: s, changsheng12: s, boshi12: s, jiangqian12: s, suiqian12: s, decadal: object({ range: integers, heavenlyStem: s, earthlyBranch: s }), ages: integers, majorStars: array(star), minorStars: array(star), adjectiveStars: array(star) })), horoscope: object({ as_of: civilDate, as_of_time: civilTime, lunarDate: s, solarDate: s, decadal: period, age: period, yearly: period, monthly: period, daily: period, hourly: period }) }),
  'regions.children': object({ parent_codes: strings, children: array(object({ code: s, name: s, has_children: b })) }),
};
