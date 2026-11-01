import React, { useRef } from 'react';
import { Tabs, Tooltip } from 'antd';
import { CompassOutlined, CalculatorOutlined } from '@ant-design/icons';
import { createStyles } from 'antd-style';
import {
  CalendarProvider, Calendar, Almanac, BirthInfo, ZiWei, Bazi,
} from '@taichuy/taichuy-calendar';

// This page is editable composition, not a hidden full-page implementation.
// Move/remove panels here, or import the registered calculation packages directly.
const useStyles = createStyles(({ css, prefixCls }) => ({
  container: css`max-width:1200px; margin:0 auto; padding:24px; @media(max-width:768px){padding:16px;}`,
  layout: css`display:grid; grid-template-columns:minmax(0,1fr) 300px; gap:24px; align-items:start; width:100%; @media(max-width:768px){display:block;}`,
  main: css`min-width:0;`,
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
  mobileCalendar: css`display:none; @media(max-width:768px){display:block; margin-bottom:16px;}`,
  tabs: css`.${prefixCls}-tabs-nav{margin-bottom:16px;}`,
}));

function CalendarPanels() {
  const { styles } = useStyles();
  const almanac = useRef<HTMLDivElement>(null);
  const goAlmanac = () => almanac.current?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  return (
    <section className={styles.container} data-calendar-composition="editable" data-calendar-component="ComposedPage">
      <div className={styles.layout}>
        <main className={styles.main}>
          <div className={styles.mobileCalendar}><Calendar onAlmanac={goAlmanac} /></div>
          <div ref={almanac} data-calendar-section="almanac"><Almanac /></div>
          <BirthInfo />
          <Tabs defaultActiveKey="ziwei" type="card" size="large" className={styles.tabs} items={[
            { key: 'ziwei', label: <Tooltip title="看具体事：性格、婚姻、事业、财运等细节"><span><CompassOutlined />紫微斗数</span></Tooltip>, children: <ZiWei /> },
            { key: 'bazi', label: <Tooltip title="看大方向：五行缺什么、大运好坏、何时转运"><span><CalculatorOutlined />八字排盘</span></Tooltip>, children: <Bazi /> },
          ]} />
        </main>
        <aside className={styles.aside} aria-label="日历" tabIndex={0}><Calendar onAlmanac={goAlmanac} /></aside>
      </div>
    </section>
  );
}

export default function TaichuyCalendarBlock(props = {}) {
  return <CalendarProvider {...props}><CalendarPanels /></CalendarProvider>;
}
