import React from 'react';
import { Tooltip, App } from 'antd';
import { QuestionCircleOutlined, CopyOutlined } from '@ant-design/icons';
import { createStyles } from 'antd-style';
import { Dayjs } from 'dayjs';
import 'dayjs/locale/zh-cn';
import { Lunar } from 'lunar-typescript';
import { SolarTime } from 'tyme4ts';
import { copyToClipboard } from '../../utils/clipboard';

const XIU_ANIMALS: Record<string, string> = {
  角: '蛟',
  亢: '龙',
  氐: '貉',
  房: '兔',
  心: '狐',
  尾: '虎',
  箕: '豹',
  斗: '蟹',
  牛: '牛',
  女: '蝠',
  虚: '鼠',
  危: '燕',
  室: '猪',
  壁: '貐',
  奎: '狼',
  娄: '狗',
  胃: '雉',
  昴: '鸡',
  毕: '乌',
  觜: '猴',
  参: '猿',
  井: '犴',
  鬼: '羊',
  柳: '獐',
  星: '马',
  张: '鹿',
  翼: '蛇',
  轸: '蚓',
};

const useStyle = createStyles(({ token, css }) => ({
  mobileHidden: css`
    @media (max-width: 600px) {
      display: none !important;
    }
  `,
  lunarContainer: css`
    position: relative;
    background: ${token.colorBgContainer};
    border-radius: 12px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.05);
    padding: 24px;
    margin-bottom: 24px;
    border: 1px solid ${token.colorBorderSecondary};
    &:hover .copy-btn {
      opacity: 1;
      visibility: visible;
    }
  `,
  copyButton: css`
    position: absolute;
    top: 12px;
    right: 12px;
    cursor: pointer;
    opacity: 0;
    visibility: hidden;
    transition: all 0.3s;
    color: ${token.colorTextSecondary};
    padding: 4px;
    border-radius: 4px;
    &:hover {
      color: ${token.colorPrimary};
      background: ${token.colorFillQuaternary};
    }
  `,
  lunarHeader: css`
    text-align: center;
    margin-bottom: 24px;
  `,
  solarDate: css`
    color: ${token.colorTextSecondary};
    font-size: 16px;
    margin-bottom: 8px;
  `,
  lunarDateLarge: css`
    color: ${token.colorWarningText};
    font-size: 36px;
    font-weight: bold;
    font-family: 'KaiTi', 'STKaiti', serif;
    margin-bottom: 8px;
    line-height: 1.2;
  `,
  ganZhiRow: css`
    color: ${token.colorTextSecondary};
    font-size: 16px;
  `,
  yiJiRow: css`
    display: flex;
    gap: 24px;
    margin-bottom: 24px;
    padding: 0 12px;
  `,
  yiJiItem: css`
    display: flex;
    align-items: center;
    gap: 12px;
    flex: 1;
  `,
  yiBadge: css`
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: #e6f7ff;
    color: #1890ff;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: bold;
    flex-shrink: 0;
  `,
  jiBadge: css`
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: #fff1f0;
    color: #ff4d4f;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: bold;
    flex-shrink: 0;
  `,
  yiJiContent: css`
    font-size: 14px;
    color: ${token.colorText};
    line-height: 1.6;
    flex: 1;
  `,
  lunarTable: css`
    border: 1px solid ${token.colorBorderSecondary};
    border-radius: 8px;
    overflow: hidden;
  `,
  tableRow: css`
    display: flex;
    border-bottom: 1px solid ${token.colorBorderSecondary};
    &:last-child {
      border-bottom: none;
    }
  `,
  tableCell: css`
    flex: 1;
    padding: 12px;
    text-align: center;
    border-right: 1px solid ${token.colorBorderSecondary};
    &:last-child {
      border-right: none;
    }
    display: flex;
    flex-direction: column;
    gap: 4px;
  `,
  cellTitle: css`
    color: #d48806;
    font-size: 14px;
    margin-bottom: 4px;
  `,
  cellValue: css`
    color: ${token.colorText};
    font-size: 15px;
  `,
  timeLuckRow: css`
    display: flex;
    border-bottom: 1px solid ${token.colorBorderSecondary};
  `,
  timeLuckTitle: css`
    width: 80px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #d48806;
    background: #fffbe6;
    border-right: 1px solid ${token.colorBorderSecondary};
    padding: 12px;
    font-weight: 500;
  `,
  timeLuckContent: css`
    flex: 1;
    display: grid;
    grid-template-columns: repeat(12, 1fr);
  `,
  timeLuckItem: css`
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 8px 0;
    gap: 4px;
    font-size: 12px;
    border-right: 1px solid ${token.colorBorderSecondary};
    &:last-child {
      border-right: none;
    }
  `,
  lucky: css`
    color: #ff4d4f;
  `,
  unlucky: css`
    color: ${token.colorTextSecondary};
  `,
  bottomSection: css`
    display: flex;
  `,
  sideColumn: css`
    width: 40px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    background: #fff;
    padding: 12px 0;
    color: #d48806;
    font-size: 14px;
    line-height: 1.2;
    gap: 4px;
    border-right: 1px solid ${token.colorBorderSecondary};
    &:last-child {
      border-right: none;
      border-left: 1px solid ${token.colorBorderSecondary};
    }
  `,
  centerGrid: css`
    flex: 1;
    display: grid;
    grid-template-columns: 1fr 1fr;
    grid-template-rows: 1fr 1fr;
  `,
  lunarCenterCell: css`
    padding: 12px;
    border-right: 1px solid ${token.colorBorderSecondary};
    border-bottom: 1px solid ${token.colorBorderSecondary};
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    text-align: center;
    &:nth-child(2n) {
      border-right: none;
    }
    &:nth-child(n + 3) {
      border-bottom: none;
    }
  `,
  lunarCenterTitle: css`
    color: ${token.colorTextSecondary};
    font-size: 12px;
  `,
  centerValue: css`
    color: ${token.colorTextSecondary};
    font-size: 13px;
  `,
}));

interface LunarInfoCardProps {
  date: Dayjs;
}

interface TermTooltipProps {
  title: string;
  vertical?: boolean;
}

const TermTooltip = ({ title, vertical = false }: TermTooltipProps) => (
  <Tooltip title={title}>
    <QuestionCircleOutlined
      style={{
        marginLeft: vertical ? 0 : 4,
        marginBottom: vertical ? 4 : 0,
        cursor: 'help',
        fontSize: 12,
        color: 'rgba(0, 0, 0, 0.45)',
      }}
    />
  </Tooltip>
);

export default function LunarInfoCard({ date }: LunarInfoCardProps) {
  const { styles } = useStyle();
  const { message } = App.useApp();
  const d = Lunar.fromDate(date.toDate());

  // Calculate duty
  const solarTime = SolarTime.fromYmdHms(date.year(), date.month() + 1, date.date(), 12, 0, 0);
  const lunar = solarTime.getSolarDay().getLunarDay();
  const duty = lunar.getDuty().getName();

  // Calculate timeLuck
  interface TimeLuckItem {
    time: string;
    luck: string;
    range: string;
  }
  const timeLuck: TimeLuckItem[] = [];
  const zhi = ['子', '丑', '寅', '卯', '辰', '巳', '午', '未', '申', '酉', '戌', '亥'];
  const timeRanges = [
    '23:00\n01:00',
    '01:00\n03:00',
    '03:00\n05:00',
    '05:00\n07:00',
    '07:00\n09:00',
    '09:00\n11:00',
    '11:00\n13:00',
    '13:00\n15:00',
    '15:00\n17:00',
    '17:00\n19:00',
    '19:00\n21:00',
    '21:00\n23:00',
  ];
  for (let i = 0; i < 12; i++) {
    const h = i * 2;
    const dHour = Lunar.fromDate(date.hour(h).minute(0).toDate());
    const luck = dHour.getTimeTianShenLuck();
    timeLuck.push({ time: zhi[i], luck, range: timeRanges[i] });
  }

  const handleCopy = () => {
    const text = [
      `公历：${date.locale('zh-cn').format('YYYY年MM月DD日 dddd')}`,
      `农历：${d.getMonthInChinese()}月${d.getDayInChinese()}`,
      `干支：${d.getYearInGanZhi()}(${d.getYearShengXiao()})年 ${d.getMonthInGanZhi()}月 ${d.getDayInGanZhi()}日`,
      `宜：${d.getDayYi().join(' ')}`,
      `忌：${d.getDayJi().join(' ')}`,
      `纳音：${d.getDayNaYin()}`,
      `冲煞：冲${d.getDayChongDesc()} 煞${d.getDaySha()}`,
      `值神：${d.getZhiXing()}(${d.getDayTianShenType() === '黄道' ? '吉' : '凶'})`,
      `建除十二神：${duty}`,
      `二十八星宿：${d.getXiu()}${XIU_ANIMALS[d.getXiu()] || ''} (${d.getXiuLuck()})`,
      `吉神宜趋：${d.getDayJiShen().join(' ')}`,
      `凶神宜忌：${d.getDayXiongSha().join(' ')}`,
      `今日胎神：${d.getDayPositionTai()}`,
      `彭祖百忌：${d.getPengZuGan()} ${d.getPengZuZhi()}`,
      `时辰吉凶：${timeLuck.map((t) => `${t.time}${t.luck}(${t.range.replace('\n', '-')})`).join(' ')}`,
    ].join('\n');

    copyToClipboard(text, () => {
      message.success('已复制黄历信息');
    });
  };

  return (
    <div className={styles.lunarContainer}>
      <Tooltip title="复制黄历信息">
        <CopyOutlined aria-label="复制黄历信息" className={`${styles.copyButton} copy-btn`} onClick={handleCopy} />
      </Tooltip>
      {/* Header */}
      <div className={styles.lunarHeader}>
        <div className={styles.solarDate}>{date.locale('zh-cn').format('YYYY年MM月DD日 dddd')}</div>
        <div className={styles.lunarDateLarge}>
          {d.getMonthInChinese()}月{d.getDayInChinese()}
        </div>
        <div className={styles.ganZhiRow}>
          {d.getYearInGanZhi()}({d.getYearShengXiao()})年 {d.getMonthInGanZhi()}月{' '}
          {d.getDayInGanZhi()}日
        </div>
      </div>

      {/* Yi / Ji */}
      <div className={styles.yiJiRow}>
        <div className={styles.yiJiItem}>
          <div className={styles.yiBadge}>宜</div>
          <div className={styles.yiJiContent}>{d.getDayYi().join(' ')}</div>
        </div>
        <div className={styles.yiJiItem}>
          <div className={styles.jiBadge}>忌</div>
          <div className={styles.yiJiContent}>{d.getDayJi().join(' ')}</div>
        </div>
      </div>

      {/* Main Table */}
      <div className={styles.lunarTable}>
        {/* Row 1 */}
        <div className={styles.tableRow}>
          <div className={styles.tableCell}>
            <div className={styles.cellTitle}>
              纳音
              <TermTooltip title="六十甲子纳音，结合五行（金木水火土）与声音韵律，常用于推断运势属性。" />
            </div>
            <div className={styles.cellValue}>{d.getDayNaYin()}</div>
          </div>
          <div className={styles.tableCell}>
            <div className={styles.cellTitle}>
              冲煞
              <TermTooltip title="冲：与当日地支相冲的属相，易有变动。煞：当日煞气所在的方位，不宜动土或修造。" />
            </div>
            <div className={styles.cellValue}>
              冲{d.getDayChongDesc()} 煞{d.getDaySha()}
            </div>
          </div>
          <div className={styles.tableCell}>
            <div className={styles.cellTitle}>
              值神
              <TermTooltip title="值神即当值的神煞，分为黄道（吉）和黑道（凶）。黄道吉日诸事皆宜，黑道凶日行事需谨慎。" />
            </div>
            <div className={styles.cellValue}>
              {d.getZhiXing()}({d.getDayTianShenType() === '黄道' ? '吉' : '凶'})
            </div>
          </div>
        </div>

        {/* Row 2: Time Luck (Original combined) */}
        <div className={styles.timeLuckRow}>
          <div className={styles.timeLuckTitle}>
            时辰
            <br />
            吉凶
          </div>
          <div className={styles.timeLuckContent}>
            {timeLuck.map((t, idx) => (
              <div key={idx} className={styles.timeLuckItem}>
                <div>{t.time}</div>
                <div className={t.luck === '吉' ? styles.lucky : styles.unlucky}>{t.luck}</div>
              </div>
            ))}
          </div>
        </div>

        {/* Row 3: Time Range (New separate row) */}
        <div className={`${styles.timeLuckRow} ${styles.mobileHidden}`}>
          <div className={styles.timeLuckTitle} style={{ fontSize: 13 }}>
            时间
          </div>
          <div className={styles.timeLuckContent}>
            {timeLuck.map((t, idx) => (
              <div key={idx} className={styles.timeLuckItem}>
                <div
                  style={{
                    fontSize: 10,
                    color: '#999',
                    lineHeight: 1.1,
                    whiteSpace: 'pre-line',
                  }}
                >
                  {t.range}
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Row 3: Complex Grid */}
        <div className={styles.bottomSection}>
          {/* Jian Chu */}
          <div className={styles.sideColumn}>
            <TermTooltip
              vertical
              title="古代择吉术语，十二位神煞轮流值日，以此判断当日适合做（宜）与不适合做（忌）的事情。"
            />
            <div>建</div>
            <div>除</div>
            <div>十</div>
            <div>二</div>
            <div>神</div>
            <div
              style={{
                marginTop: 8,
                fontWeight: 'bold',
                writingMode: 'vertical-rl',
                textOrientation: 'upright',
                letterSpacing: 2,
              }}
            >
              {duty}
            </div>
          </div>

          {/* Center 4 Cells */}
          <div className={styles.centerGrid}>
            <div className={styles.lunarCenterCell}>
              <div className={styles.lunarCenterTitle}>
                吉神宜趋
                <TermTooltip title="今日值班的吉神，适合前往其所在方位或进行相关吉利活动。" />
              </div>
              <div className={styles.centerValue}>{d.getDayJiShen().join(' ')}</div>
            </div>
            <div className={styles.lunarCenterCell}>
              <div className={styles.lunarCenterTitle}>
                今日胎神
                <TermTooltip title="传统习俗中守护胎儿的神灵方位，孕妇家中今日不宜在此方位敲打或移动重物。" />
              </div>
              <div className={styles.centerValue}>{d.getDayPositionTai()}</div>
            </div>
            <div className={styles.lunarCenterCell}>
              <div className={styles.lunarCenterTitle}>
                凶神宜忌
                <TermTooltip title="今日值班的凶神，应避开其所在方位，或避免进行相关的高风险活动。" />
              </div>
              <div className={styles.centerValue}>{d.getDayXiongSha().join(' ')}</div>
            </div>
            <div className={styles.lunarCenterCell}>
              <div className={styles.lunarCenterTitle}>
                彭祖百忌
                <TermTooltip title="传说中彭祖总结的每日禁忌，根据当日天干地支指出应当避免的具体事项。" />
              </div>
              <div className={styles.centerValue}>
                <div>{d.getPengZuGan()}</div>
                <div>{d.getPengZuZhi()}</div>
              </div>
            </div>
          </div>

          {/* 28 Stars */}
          <div className={styles.sideColumn}>
            <TermTooltip
              vertical
              title="中国古代天文学的二十八个星区，每日一星宿值班，以此预示当日的吉凶祸福。"
            />
            <div>二</div>
            <div>十</div>
            <div>八</div>
            <div>星</div>
            <div>宿</div>
            <div
              style={{
                marginTop: 8,
                fontWeight: 'bold',
                writingMode: 'vertical-rl',
                textOrientation: 'upright',
                letterSpacing: 2,
              }}
            >
              {d.getXiu()}
              {XIU_ANIMALS[d.getXiu()] || ''}
              <span style={{ marginTop: 4 }}>{d.getXiuLuck()}</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
