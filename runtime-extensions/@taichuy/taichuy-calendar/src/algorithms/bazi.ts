import dayjs, { type Dayjs } from 'dayjs';
import { SolarTime, ChildLimit, Gender, type DecadeFortune } from 'tyme4ts';
import type { BaziResult } from '../components/divination/types';

/** Algorithm extracted verbatim from the approved page's BaziCalculator effect.
 * The injected clock only replaces the two source 'now' reads for deterministic fixtures.
 */
export function calculateBazi(date: Dayjs, time: Dayjs, gender: 'male' | 'female', asOf = dayjs()) {
  const solarTime = SolarTime.fromYmdHms(
    date.year(),
    date.month() + 1,
    date.date(),
    time.hour(),
    time.minute(),
    0
  );

  const lunarHour = solarTime.getLunarHour();

  // Calculate Age
  const now = asOf;
  const age = now.diff(date, 'year');
  const chineseAge = now.year() - date.year() + 1;

  const eightChar = lunarHour.getEightChar();
  const dayMaster = eightChar.getDay().getHeavenStem();

  // Calculate Da Yun
  const childLimit = ChildLimit.fromSolarTime(
    solarTime,
    gender === 'male' ? Gender.MAN : Gender.WOMAN
  );
  const daYun: DecadeFortune[] = [];
  const daYunDetails: BaziResult['daYunDetails'] = [];
  let currentDaYun = childLimit.getStartDecadeFortune();

  // Find current Da Yun index
  const currentYear = asOf.year();
  let initialDaYunIndex = 0;

  for (let i = 0; i < 8; i++) {
    daYun.push(currentDaYun);

    const startYear = currentDaYun.getStartLunarYear().getYear();
    const endYear = currentDaYun.getEndLunarYear().getYear();
    if (currentYear >= startYear && currentYear <= endYear) {
      initialDaYunIndex = i;
    }

    // Calculate Years (Liu Nian) and Xiao Yun
    const years: BaziResult['daYunDetails'][0]['years'] = [];
    let currentFortune = currentDaYun.getStartFortune();

    for (let j = 0; j < 10; j++) {
      const lunarYear = currentFortune.getLunarYear();
      const yearGanZhi = lunarYear.getSixtyCycle();
      const xiaoYunGanZhi = currentFortune.getSixtyCycle();

      // Calculate Months (Liu Yue)
      const months = lunarYear.getMonths().map((m) => {
        const mGanZhi = m.getSixtyCycle();
        const mStem = mGanZhi.getHeavenStem();
        const mBranch = mGanZhi.getEarthBranch();
        return {
          month: m.toString(),
          ganZhi: mGanZhi.getName(),
          wuxing: `${mStem.getElement().getName()}${mBranch.getElement().getName()}`,
          tenGod: dayMaster.getTenStar(mStem).getName(),
        };
      });

      const yearStem = yearGanZhi.getHeavenStem();
      const yearBranch = yearGanZhi.getEarthBranch();
      const xiaoYunStem = xiaoYunGanZhi.getHeavenStem();
      const xiaoYunBranch = xiaoYunGanZhi.getEarthBranch();

      years.push({
        age: currentFortune.getAge(),
        year: lunarYear.getYear(),
        ganZhi: yearGanZhi.getName(),
        wuxing: `${yearStem.getElement().getName()}${yearBranch.getElement().getName()}`,
        tenGod: dayMaster.getTenStar(yearStem).getName(),
        xiaoYun: xiaoYunGanZhi.getName(),
        xiaoYunGanZhi: xiaoYunGanZhi.getName(),
        xiaoYunWuXing: `${xiaoYunStem.getElement().getName()}${xiaoYunBranch.getElement().getName()}`,
        xiaoYunTenGod: dayMaster.getTenStar(xiaoYunStem).getName(),
        months: months,
      });

      currentFortune = currentFortune.next(1);
    }

    const dyGanZhi = currentDaYun.getSixtyCycle();
    const dyStem = dyGanZhi.getHeavenStem();
    const dyBranch = dyGanZhi.getEarthBranch();

    daYunDetails.push({
      name: currentDaYun.getName(),
      ganZhi: dyGanZhi.getName(),
      wuxing: `${dyStem.getElement().getName()}${dyBranch.getElement().getName()}`,
      startYear: startYear,
      endYear: endYear,
      startAge: currentDaYun.getStartAge(),
      endAge: currentDaYun.getEndAge(),
      tenGod: dayMaster.getTenStar(dyStem).getName(),
      years: years,
    });
    currentDaYun = currentDaYun.next(1);
  }


  // Qi Yun Info
  const qiyunEndTime = childLimit.getEndTime();
  const qiyun = `${childLimit.getYearCount()}年${childLimit.getMonthCount()}个月${childLimit.getDayCount()}天${childLimit.getHourCount()}小时${childLimit.getMinuteCount()}分后起运 (公历 ${qiyunEndTime.getYear()}年${qiyunEndTime.getMonth()}月${qiyunEndTime.getDay()}日)`;

  // Wu Xing & Details
  const wuxingCounts: Record<string, number> = { 金: 0, 木: 0, 水: 0, 火: 0, 土: 0 };
  const wuxingDetail: Record<string, { count: number; sources: string[] }> = {
    金: { count: 0, sources: [] },
    木: { count: 0, sources: [] },
    水: { count: 0, sources: [] },
    火: { count: 0, sources: [] },
    土: { count: 0, sources: [] },
  };

  const stems = [
    { name: '年干', val: eightChar.getYear().getHeavenStem() },
    { name: '月干', val: eightChar.getMonth().getHeavenStem() },
    { name: '日干', val: eightChar.getDay().getHeavenStem() },
    { name: '时干', val: eightChar.getHour().getHeavenStem() },
  ];
  const branches = [
    { name: '年支', val: eightChar.getYear().getEarthBranch() },
    { name: '月支', val: eightChar.getMonth().getEarthBranch() },
    { name: '日支', val: eightChar.getDay().getEarthBranch() },
    { name: '时支', val: eightChar.getHour().getEarthBranch() },
  ];

  // Count Stems
  stems.forEach((s) => {
    const wx = s.val.getElement().getName();
    if (wx && wuxingCounts[wx] !== undefined) {
      wuxingCounts[wx]++;
      wuxingDetail[wx].count++;
      wuxingDetail[wx].sources.push(`${s.name}：${s.val.getName()}`);
    }
  });

  // Count Hidden Stems in Branches
  const hiddenStems: Record<string, string[]> = {};
  const tenGods: Record<string, string> = {};
  const tenGodsWuxing: Record<string, string> = {};

  branches.forEach((b) => {
    const hidden = b.val.getHideHeavenStems();
    hiddenStems[b.val.getName()] = hidden.map((h) => h.getName());

    hidden.forEach((hGan) => {
      const stem = hGan.getHeavenStem();
      const wx = stem.getElement().getName();
      if (wx && wuxingCounts[wx] !== undefined) {
        wuxingCounts[wx]++;
        wuxingDetail[wx].count++;
        wuxingDetail[wx].sources.push(`${b.name}(${b.val.getName()})藏：${hGan.getName()}`);
      }

      if (!tenGods[hGan.getName()]) {
        tenGods[hGan.getName()] = dayMaster.getTenStar(stem).getName();
      }
      if (!tenGodsWuxing[hGan.getName()]) {
        tenGodsWuxing[hGan.getName()] = wx;
      }
    });
  });

  stems.forEach((s) => {
    const wx = s.val.getElement().getName();
    if (s.name === '日干') {
      tenGods[s.val.getName()] = '日主';
    } else {
      tenGods[s.val.getName()] = dayMaster.getTenStar(s.val).getName();
    }
    tenGodsWuxing[s.val.getName()] = wx;
  });

  let geJu = '未知格局';
  try {
    const monthZhi = eightChar.getMonth().getEarthBranch();
    const mainQi = monthZhi.getHideHeavenStemMain();
    if (mainQi) {
      const tenGod = dayMaster.getTenStar(mainQi).getName();
      geJu = `${tenGod}格`;
    }
  } catch (e) {
    console.error('Ge Ju calculation error', e);
  }

  const dayKong = eightChar
    .getDay()
    .getExtraEarthBranches()
    .map((b) => b.getName())
    .join('');
  const yearKong = eightChar
    .getYear()
    .getExtraEarthBranches()
    .map((b) => b.getName())
    .join('');
  const xunKong = `日空: ${dayKong} / 年空: ${yearKong}`;

  const lunarDay = solarTime.getSolarDay().getLunarDay();
  const shenSha = lunarDay.getGods().map((g) => g.getName());

  const duty = lunarDay.getDuty().getName();
  const twelveStar = lunarDay.getTwelveStar().getName();
  const ecliptic = lunarDay.getTwelveStar().getEcliptic().getName();

  const result: BaziResult = {
    solarDate: solarTime.getSolarDay().toString(),
    lunarDate: lunarHour.getLunarDay().toString(),
    gender: gender === 'male' ? '乾造' : '坤造',
    age,
    chineseAge,
    eightChar,
    dayMaster: dayMaster.getName(),
    daYun,
    daYunDetails,
    wuxingCounts,
    wuxingDetail,
    hiddenStems,
    tenGods,
    tenGodsWuxing,
    geJu,
    xunKong,
    shenSha,
    taiYuan: eightChar.getFetalOrigin().getName(),
    mingGong: eightChar.getOwnSign().getName(),
    shenGong: eightChar.getBodySign().getName(),
    qiyun,
    huangDao: {
      duty,
      twelveStar,
      ecliptic,
      gods: shenSha,
    },
  };
  return { result, initialDaYunIndex };
}
