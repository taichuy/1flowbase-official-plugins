import { EightChar, DecadeFortune } from 'tyme4ts';

export interface BaziResult {
  solarDate: string;
  lunarDate: string;
  gender: string;
  age: number; // 周岁
  chineseAge: number; // 虚岁
  eightChar: EightChar;
  dayMaster: string;
  daYun: DecadeFortune[];
  wuxingCounts: Record<string, number>;
  wuxingDetail: Record<string, { count: number; sources: string[] }>;
  hiddenStems: Record<string, string[]>;
  tenGods: Record<string, string>;
  tenGodsWuxing: Record<string, string>;
  xunKong: string;
  shenSha: string[];
  geJu: string;
  taiYuan: string;
  mingGong: string;
  shenGong: string;
  qiyun: string;
  daYunDetails: {
    name: string;
    ganZhi: string;
    wuxing: string;
    startYear: number;
    endYear: number;
    startAge: number;
    endAge: number;
    tenGod: string;
    years: {
      age: number;
      year: number;
      ganZhi: string;
      wuxing: string;
      tenGod: string;
      xiaoYun: string;
      xiaoYunGanZhi: string;
      xiaoYunWuXing: string;
      xiaoYunTenGod: string;
      months: {
        month: string;
        ganZhi: string;
        wuxing: string;
        tenGod: string;
      }[];
    }[];
  }[];
  huangDao: {
    duty: string;
    twelveStar: string;
    ecliptic: string;
    gods: string[];
  };
}
