import React from 'react';
import { Flex, Tabs, Tooltip } from 'antd';
import { CompassOutlined, CalculatorOutlined } from '@ant-design/icons';
import { createStyles } from 'antd-style';
import BaziCalculator from '../components/divination/BaziCalculator';
import ZiWeiCalculator from '../components/divination/ZiWeiCalculator';
import LunarInfoCard from '../components/divination/LunarInfoCard';
import TaichuyCalendarCard from '../components/TaichuyCalendarCard';
import BirthInfoForm from './BirthInfo';
import { useDateContext } from '../state/DateContext';
import { usePluginContext } from '../state/PluginContext';
import { usePageStyles } from './styles';

// The old site's right rail becomes a local calendar, not an embedded site shell.
const useLayoutStyles = createStyles(({ css }) => ({

  layout: css`width:100%;`,
  spacer: css`
    flex: 0.3 1 0;
    min-width: 0;
    @container calendar-page (max-width: 768px) { display: none; }
  `,
  leadingSpace: css`
    flex: 0.3 1 0;
    min-width: 0;
  `,
  main: css`
    flex: 6 1 0;
    min-width: 0;
    @container calendar-page (max-width: 768px) { flex: 1 1 100%; }
  `,
  aside: css`
    flex: 3.4 1 0;
    min-width: 0;
    position: sticky;
    top: 16px;
    /* Keep short-window controls reachable below the host toolbar. */
    max-height: calc(100dvh - 160px);
    overflow-y: auto;
    overscroll-behavior-y: contain;
    scrollbar-gutter: stable;
    @container calendar-page (min-width: 769px) and (max-width: 1199px) { min-width: 280px; }
    @container calendar-page (max-width: 768px) { display: none; }
  `,

}));

export default function CalendarPageContent() {
  const { styles } = usePageStyles();
  const { styles: layout } = useLayoutStyles();
  const { selectedDate } = useDateContext();
  const { birthDate, time, trueSolarDateTime, gender, locationInfo, chartRevision } = usePluginContext();
  const civilDateTime = birthDate.hour(time.hour()).minute(time.minute()).second(time.second());
  const chartProps = { date: trueSolarDateTime.date, time: trueSolarDateTime.time, civilDateTime, gender, locationInfo };
  const items = [
    {
      key: 'ziwei',
      label: <Tooltip title="看具体事：性格、婚姻、事业、财运等细节"><span><CompassOutlined />紫微斗数</span></Tooltip>,
      children: <ZiWeiCalculator key={`ziwei-${chartRevision}`} {...chartProps} />,
    },
    {
      key: 'bazi',
      label: <Tooltip title="看大方向：五行缺什么、大运好坏、何时转运"><span><CalculatorOutlined />八字排盘</span></Tooltip>,
      children: <BaziCalculator key={`bazi-${chartRevision}`} {...chartProps} />,
    },
  ];
  return (
    <section className={styles.container} data-calendar-component="CalendarPage">
      <Flex align="start" className={layout.layout}>
        <div className={layout.leadingSpace} aria-hidden="true" />
        <main className={layout.main}>
          <div className={styles.mobileCalendar}><TaichuyCalendarCard /></div>
          <div data-calendar-section="almanac"><LunarInfoCard date={selectedDate} /></div>
          <BirthInfoForm />
          <Tabs defaultActiveKey="ziwei" items={items} type="card" size="large" className={styles.tabCard} />
        </main>
        <div className={layout.spacer} aria-hidden="true" />
        <aside className={layout.aside} aria-label="日历" tabIndex={0}><TaichuyCalendarCard /></aside>
      </Flex>
    </section>
  );
}
