import { BaziResult } from './types';
import { LunarYear } from 'tyme4ts';
import dayjs from 'dayjs';
import { Solar } from 'lunar-typescript';

export const generateBaziAIDescription = (
  baziResult: BaziResult | null,
  locationInfo?: {
    location?: string;
    longitude: number;
    latitude: number;
    useTrueSolarTime: boolean;
    trueSolarTimeStr?: string;
  }
) => {
  if (!baziResult) return '';

  const now = dayjs();
  const solar = Solar.fromYmd(now.year(), now.month() + 1, now.date());
  const lunar = solar.getLunar();

  let text = `【八字命盘客观数据】\n\n`;
  text += `基本信息：\n`;

  // Current Time Info
  text += `- 当前时间：${now.format('YYYY年MM月DD日 HH:mm:ss')}\n`;
  text += `- 当前农历：${lunar.getYearInChinese()}年 ${lunar.getMonthInChinese()}月 ${lunar.getDayInChinese()}\n`;

  if (locationInfo) {
    text += `- 出生地点：${locationInfo.location || '未指定'}\n`;
    text += `- 经纬度：${locationInfo.longitude.toFixed(2)}°E, ${locationInfo.latitude.toFixed(2)}°N\n`;
    text += `- 真太阳时：${locationInfo.useTrueSolarTime ? '已启用' : '未启用'}\n`;
    if (locationInfo.useTrueSolarTime && locationInfo.trueSolarTimeStr) {
      text += `- 真太阳时时间：${locationInfo.trueSolarTimeStr}\n`;
    }
  }
  text += `- 阳历：${baziResult.solarDate}\n`;
  text += `- 农历：${baziResult.lunarDate}\n`;
  text += `- 性别：${baziResult.gender}\n`;
  text += `- 年龄：${baziResult.age}岁 (虚岁${baziResult.chineseAge})\n`;
  text += `- 日主：${baziResult.dayMaster}\n`;
  text += `- 格局：${baziResult.geJu}\n`;
  text += `- 胎元：${baziResult.taiYuan}\n`;
  text += `- 命宫：${baziResult.mingGong}\n`;
  text += `- 身宫：${baziResult.shenGong}\n`;
  text += `- 空亡：${baziResult.xunKong}\n`;
  text += `- 黄道：${baziResult.huangDao.ecliptic} (${baziResult.huangDao.duty}日 / ${baziResult.huangDao.twelveStar})\n`;
  text += `- 神煞：${baziResult.shenSha.join('、')}\n`;

  // Current Year (Liu Nian)
  const currentYear = LunarYear.fromYear(new Date().getFullYear());
  text += `- 当前流年：${currentYear.getName()} (${currentYear.getSixtyCycle().getName()})\n\n`;

  text += `八字排盘：\n`;
  const pillars = [
    {
      name: '年柱',
      stem: baziResult.eightChar.getYear().getHeavenStem(),
      branch: baziResult.eightChar.getYear().getEarthBranch(),
      nayin: baziResult.eightChar.getYear().getSound().getName(),
    },
    {
      name: '月柱',
      stem: baziResult.eightChar.getMonth().getHeavenStem(),
      branch: baziResult.eightChar.getMonth().getEarthBranch(),
      nayin: baziResult.eightChar.getMonth().getSound().getName(),
    },
    {
      name: '日柱',
      stem: baziResult.eightChar.getDay().getHeavenStem(),
      branch: baziResult.eightChar.getDay().getEarthBranch(),
      nayin: baziResult.eightChar.getDay().getSound().getName(),
    },
    {
      name: '时柱',
      stem: baziResult.eightChar.getHour().getHeavenStem(),
      branch: baziResult.eightChar.getHour().getEarthBranch(),
      nayin: baziResult.eightChar.getHour().getSound().getName(),
    },
  ];

  pillars.forEach((p) => {
    const tenGod = baziResult.tenGods[p.stem.getName()];
    const hidden = baziResult.hiddenStems[p.branch.getName()] || [];
    const hiddenStr = hidden.map((h) => `${h}(${baziResult.tenGods[h] || ''})`).join(' ');
    text += `${p.name}：${p.stem.getName()}${p.branch.getName()} (${p.nayin}) [${tenGod}]\n`;
    text += `  藏干：${hiddenStr}\n`;
  });
  text += `\n`;

  text += `五行统计（详细过程）：\n`;
  Object.entries(baziResult.wuxingDetail).forEach(([wx, detail]) => {
    text += `- ${wx} (共${detail.count}个)：${detail.sources.join('，')}\n`;
  });
  text += `\n`;

  text += `大运：\n`;
  baziResult.daYun.forEach((dy) => {
    text += `${dy.getStartAge()}-${dy.getEndAge()}岁：${dy.getName()}运\n`;
    // LiuNian for DaYun
    const startYear = dy.getStartLunarYear();
    let yearsStr = '';
    let curr = startYear;
    for (let i = 0; i < 10; i++) {
      yearsStr += `${curr.getYear()}(${curr.getSixtyCycle().getName()}) `;
      curr = curr.next(1);
    }
    text += `  流年：${yearsStr}\n`;
  });

  text += `\n请基于以上八字数据，进行详细的命理分析，包括喜用神、忌神分析，格局判断（已计算为${baziResult.geJu}，请复核），以及大势流年运势预测（结合藏干、十神、空亡、神煞等信息）。`;
  return text;
};
