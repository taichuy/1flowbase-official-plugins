import { Solar, Lunar } from 'lunar-typescript';
import { SolarTime } from 'tyme4ts';
import { civil, fail } from './input';
export function calendarDate(solar: Solar) {
  const lunar = solar.getLunar();
  return { civil_date: solar.toYmd(), weekday: solar.getWeek(), lunar: { year: lunar.getYear(), month: Math.abs(lunar.getMonth()), day: lunar.getDay(), is_leap_month: lunar.getMonth() < 0, month_text: lunar.getMonthInChinese(), day_text: lunar.getDayInChinese(), year_gan_zhi: lunar.getYearInGanZhi(), month_gan_zhi: lunar.getMonthInGanZhi(), day_gan_zhi: lunar.getDayInGanZhi(), zodiac: lunar.getYearShengXiao() }, solar_term: lunar.getJieQi(), festivals: [...solar.getFestivals(), ...lunar.getFestivals()] };
}
export function month(input: any) {
  const first = civil(`${input.year}-${String(input.month).padStart(2, '0')}-01`);
  return { year: input.year, month: input.month, first_weekday: first.day(), days: Array.from({ length: first.daysInMonth() }, (_, i) => calendarDate(Solar.fromYmd(input.year, input.month, i + 1))) };
}
export function convert(input: any) {
  if (input.calendar === 'solar') {
    if (input.is_leap_month) fail('invalid_input', 'Solar dates cannot select a leap lunar month');
    civil(`${input.year}-${String(input.month).padStart(2, '0')}-${String(input.day).padStart(2, '0')}`);
    return calendarDate(Solar.fromYmd(input.year, input.month, input.day));
  }
  const m = input.is_leap_month ? -input.month : input.month;
  try {
    const lunar = Lunar.fromYmd(input.year, m, input.day);
    const roundtrip = lunar.getSolar().getLunar();
    if (roundtrip.getYear() !== input.year || roundtrip.getMonth() !== m || roundtrip.getDay() !== input.day) fail('invalid_date', 'Invalid lunar date or leap month');
    return calendarDate(lunar.getSolar());
  } catch { return fail('invalid_date', 'Invalid lunar date or leap month'); }
}
export function almanac(input: any) {
  const dt = civil(input.civil_date);
  const solar = Solar.fromYmd(dt.year(), dt.month() + 1, dt.date());
  const d = solar.getLunar();
  const lunarDay = SolarTime.fromYmdHms(dt.year(), dt.month() + 1, dt.date(), 12, 0, 0).getSolarDay().getLunarDay();
  const zhi = ['子', '丑', '寅', '卯', '辰', '巳', '午', '未', '申', '酉', '戌', '亥'];
  return { ...calendarDate(solar), yi: d.getDayYi(), ji: d.getDayJi(), na_yin: d.getDayNaYin(), chong: d.getDayChongDesc(), sha: d.getDaySha(), zhi_xing: d.getZhiXing(), tian_shen_type: d.getDayTianShenType(), duty: lunarDay.getDuty().getName(), xiu: d.getXiu(), xiu_luck: d.getXiuLuck(), ji_shen: d.getDayJiShen(), xiong_sha: d.getDayXiongSha(), tai_shen: d.getDayPositionTai(), peng_zu: [d.getPengZuGan(), d.getPengZuZhi()], time_luck: zhi.map((branch, index) => ({ branch, hour: index * 2, luck: Solar.fromYmdHms(dt.year(), dt.month() + 1, dt.date(), index * 2, 0, 0).getLunar().getTimeTianShenLuck() })) };
}
