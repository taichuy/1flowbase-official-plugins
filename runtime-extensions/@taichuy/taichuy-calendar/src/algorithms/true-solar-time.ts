import type { Dayjs } from 'dayjs';
import { SiderealTime, Body, Equator, Observer } from 'astronomy-engine';

export function calculateTrueSolarTime(
  date: Dayjs,
  time: Dayjs,
  longitude: number,
  latitude: number
): Dayjs {
  // 1. 组合日期和时间
  const dt = date.hour(time.hour()).minute(time.minute()).second(time.second());
  const jsDate = dt.toDate();

  // 2. 创建观察者
  const observer = new Observer(latitude, longitude, 0);

  // 3. 获取太阳赤道坐标 (Right Ascension)
  // 使用 Equator of Date (True Equator) 以包含章动等影响
  const sunEq = Equator(Body.Sun, jsDate, observer, true, true);
  const ra = sunEq.ra; // 赤经 (小时)

  // 4. 获取格林威治恒星时 (GMST)
  // SiderealTime 返回的是 GMST (小时)
  const gmst = SiderealTime(jsDate);

  // 5. 计算地方时角 (Local Hour Angle)
  // LHA = GMST + Longitude - RA
  // 经度转换为小时: deg / 15
  const longHours = longitude / 15.0;
  let lha = gmst + longHours - ra;

  // 归一化到 [0, 24)
  while (lha < 0) lha += 24;
  while (lha >= 24) lha -= 24;

  // 6. 计算真太阳时
  // 当太阳在子午线时(LHA=0)，是真太阳时12:00
  let solarTimeHours = lha + 12;
  while (solarTimeHours >= 24) solarTimeHours -= 24;

  // 7. 计算与标准时间的差值 (分钟)
  // 标准时间 (小时)
  const standardTimeHours = dt.hour() + dt.minute() / 60 + dt.second() / 3600;

  // 处理跨天情况
  // 如果差值超过 12 小时，说明跨天了
  let diffHours = solarTimeHours - standardTimeHours;
  if (diffHours > 12) diffHours -= 24;
  if (diffHours < -12) diffHours += 24;

  // 8. 应用调整
  return dt.add(diffHours * 60, 'minute');
}
