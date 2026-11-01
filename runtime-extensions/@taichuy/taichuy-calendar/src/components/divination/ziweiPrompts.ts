import type { IFunctionalAstrolabe } from 'iztro/lib/astro/FunctionalAstrolabe';
import type { IFunctionalHoroscope } from 'iztro/lib/astro/FunctionalHoroscope';
import dayjs from 'dayjs';
import { Solar } from 'lunar-typescript';

export const generateZiWeiAIDescription = (
  astrolabe: IFunctionalAstrolabe | null,
  locationInfo?: {
    location?: string;
    longitude: number;
    latitude: number;
    useTrueSolarTime: boolean;
    trueSolarTimeStr?: string;
  },
  ageInfo?: {
    age: number;
    chineseAge: number;
  },
  gender?: 'male' | 'female'
) => {
  if (!astrolabe) return '';

  const now = dayjs();
  const solar = Solar.fromYmd(now.year(), now.month() + 1, now.date());
  const lunar = solar.getLunar();

  let text = `### 【基础信息】\n\n`;

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
  text += `- 阳历生日：${astrolabe.solarDate}\n`;
  text += `- 农历生日：${astrolabe.lunarDate}\n`;
  if (gender) {
    text += `- 性别：${gender === 'male' ? '男' : '女'}\n`;
  }
  if (ageInfo) {
    text += `- 年龄：${ageInfo.age}岁 (虚岁${ageInfo.chineseAge})\n`;
  }
  text += `- 八字：${astrolabe.chineseDate}\n`;
  text += `- 命主：${astrolabe.soul}\n`;
  text += `- 身主：${astrolabe.body}\n`;
  text += `- 五行局：${astrolabe.fiveElementsClass}\n\n`;

  text += `### 【命盘数据】\n\n`;
  astrolabe.palaces.forEach((p) => {
    text += `#### ${p.name} (${p.heavenlyStem}${p.earthlyBranch})\n`;
    const major = p.majorStars
      .map((s) => `${s.name}(${s.brightness})${s.mutagen ? `[${s.mutagen}]` : ''}`)
      .join('，');
    text += `- 主星：${major || '无'}\n`;
    const minor = p.minorStars.map((s) => s.name).join('，');
    text += `- 辅星：${minor || '无'}\n`;
    const adj = p.adjectiveStars.map((s) => s.name).join('，');
    text += `- 杂曜：${adj || '无'}\n`;
    text += `- 大限：${p.decadal.range[0]}-${p.decadal.range[1]}岁\n`;
    text += `- 小限：${p.ages.join('、')}岁\n\n`;
  });

  text += `\n请基于以上客观数据，进行详细的紫微斗数解读。`;
  return text;
};

export const generateZiWeiAIDescriptionWithHoroscope = (
  astrolabe: IFunctionalAstrolabe | null,
  horoscope: IFunctionalHoroscope | null,
  locationInfo?: {
    location?: string;
    longitude: number;
    latitude: number;
    useTrueSolarTime: boolean;
    trueSolarTimeStr?: string;
  },
  ageInfo?: {
    age: number;
    chineseAge: number;
  },
  gender?: 'male' | 'female'
) => {
  if (!astrolabe) return '';

  let text = generateZiWeiAIDescription(astrolabe, locationInfo, ageInfo, gender);

  // Remove the default closing to append horoscope info
  text = text.replace('\n请基于以上客观数据，进行详细的紫微斗数解读。', '');

  if (horoscope) {
    text += `\n\n### 【运势信息】\n`;

    // Decadal (Da Xian)
    if (horoscope.decadal) {
      const palace = astrolabe.palaces[horoscope.decadal.index];
      text += `- 当前大限：${horoscope.decadal.heavenlyStem}${horoscope.decadal.earthlyBranch} (命宫在${palace?.name})\n`;
    }

    // Yearly (Liu Nian)
    if (horoscope.yearly) {
      const palace = astrolabe.palaces[horoscope.yearly.index];
      text += `- 当前流年：${horoscope.yearly.heavenlyStem}${horoscope.yearly.earthlyBranch} (命宫在${palace?.name})\n`;
    }

    // Monthly (Liu Yue)
    if (horoscope.monthly) {
      const palace = astrolabe.palaces[horoscope.monthly.index];
      text += `- 当前流月：${horoscope.monthly.heavenlyStem}${horoscope.monthly.earthlyBranch} (命宫在${palace?.name})\n`;
    }

    // Daily (Liu Ri)
    if (horoscope.daily) {
      const palace = astrolabe.palaces[horoscope.daily.index];
      text += `- 当前流日：${horoscope.daily.heavenlyStem}${horoscope.daily.earthlyBranch} (命宫在${palace?.name})\n`;
    }
  }

  text += `\n请基于以上客观数据（含本命盘及当前运势），进行详细的紫微斗数运势解读。重点分析当前大限和流年的吉凶祸福，以及需要注意的事项。`;
  return text;
};
