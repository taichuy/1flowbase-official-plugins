import React, { useRef } from 'react';
import { Tabs, Tooltip } from 'antd';
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
  layout: css`
    display: grid;
    grid-template-columns: minmax(0, 1fr) 300px;
    gap: 24px;
    align-items: start;
    width: 100%;
    @media (max-width: 768px) { display: block; }
  `,
  main: css`min-width: 0;`,
  aside: css`
    position: sticky;
    top: 16px;
    /* Keep short-window controls reachable below the host toolbar. */
    max-height: calc(100dvh - 160px);
    overflow-y: auto;
    overscroll-behavior-y: contain;
    scrollbar-gutter: stable;
    @media (max-width: 768px) { display: none; }
  `,
}));

export default function CalendarPageContent() {
  const { styles } = usePageStyles();
  const { styles: layout } = useLayoutStyles();
  const { selectedDate } = useDateContext();
  const { trueSolarDateTime, gender, locationInfo, chartRevision } = usePluginContext();
  const almanacRef = useRef<HTMLDivElement>(null);
  const goAlmanac = () => almanacRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  const chartProps = { date: trueSolarDateTime.date, time: trueSolarDateTime.time, gender, locationInfo };
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
      <div className={layout.layout}>
        <main className={layout.main}>
          <div className={styles.mobileCalendar}><TaichuyCalendarCard onAlmanac={goAlmanac} /></div>
          <div ref={almanacRef} data-calendar-section="almanac"><LunarInfoCard date={selectedDate} /></div>
          <BirthInfoForm />
          <Tabs defaultActiveKey="ziwei" items={items} type="card" size="large" className={styles.tabCard} />
        </main>
        <aside className={layout.aside} aria-label="日历" tabIndex={0}><TaichuyCalendarCard onAlmanac={goAlmanac} /></aside>
      </div>
    </section>
  );
}
