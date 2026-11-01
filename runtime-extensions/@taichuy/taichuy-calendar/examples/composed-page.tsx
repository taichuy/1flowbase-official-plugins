import React, { useRef } from 'react';
import { Flex, Tabs, Tooltip } from 'antd';
import { CompassOutlined, CalculatorOutlined } from '@ant-design/icons';
import { createStyles } from 'antd-style';
import {
  CalendarProvider, Calendar, Almanac, BirthInfo, ZiWei, Bazi,
} from '@taichuy/taichuy-calendar';

// This page is editable composition, not a hidden full-page implementation.
// Move/remove panels here, or import the registered calculation packages directly.
const useStyles = createStyles(({ css, prefixCls }) => ({
  // Fractions of the block content width: leading space / details / gap / calendar.
  container: css`width:100%; box-sizing:border-box; padding:24px 16px; container:calendar-page / inline-size;`,

  layout: css`width:100%;`,
  spacer: css`
    flex: 1 1 0;
    min-width: 0;
    @container calendar-page (max-width: 1199px) { flex: 0 0 24px; }
    @container calendar-page (max-width: 768px) { display: none; }
  `,
  leadingSpace: css`
    flex: 1 1 0;
    min-width: 0;
    @container calendar-page (max-width: 1199px) { display: none; }
  `,
  main: css`
    flex: 6 1 0;
    min-width: 0;
    @container calendar-page (max-width: 1199px) { flex: 1 1 0; }
    @container calendar-page (max-width: 768px) { flex: 1 1 100%; }
  `,
  aside: css`
    flex: 2 1 0;
    min-width: 0;
    position: sticky;
    top: 16px;
    /* Keep short-window controls reachable below the host toolbar. */
    max-height: calc(100dvh - 160px);
    overflow-y: auto;
    overscroll-behavior-y: contain;
    scrollbar-gutter: stable;
    @container calendar-page (max-width: 1199px) { flex: 0 0 280px; }
    @container calendar-page (max-width: 768px) { display: none; }
  `,
  mobileCalendar: css`display:none; @container calendar-page (max-width:768px){display:block; margin-bottom:16px;}`,
  tabs: css`.${prefixCls}-tabs-nav{margin-bottom:16px;}`,
}));

function CalendarPanels() {
  const { styles } = useStyles();
  const almanac = useRef<HTMLDivElement>(null);
  const goAlmanac = () => almanac.current?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  return (
    <section className={styles.container} data-calendar-composition="editable" data-calendar-component="ComposedPage">
      <Flex align="start" className={styles.layout}>
        <div className={styles.leadingSpace} aria-hidden="true" />
        <main className={styles.main}>
          <div className={styles.mobileCalendar}><Calendar onAlmanac={goAlmanac} /></div>
          <div ref={almanac} data-calendar-section="almanac"><Almanac /></div>
          <BirthInfo />
          <Tabs defaultActiveKey="ziwei" type="card" size="large" className={styles.tabs} items={[
            { key: 'ziwei', label: <Tooltip title="看具体事：性格、婚姻、事业、财运等细节"><span><CompassOutlined />紫微斗数</span></Tooltip>, children: <ZiWei /> },
            { key: 'bazi', label: <Tooltip title="看大方向：五行缺什么、大运好坏、何时转运"><span><CalculatorOutlined />八字排盘</span></Tooltip>, children: <Bazi /> },
          ]} />
        </main>
        <div className={styles.spacer} aria-hidden="true" />
        <aside className={styles.aside} aria-label="日历" tabIndex={0}><Calendar onAlmanac={goAlmanac} /></aside>
      </Flex>
    </section>
  );
}

export default function TaichuyCalendarBlock(props = {}) {
  return <CalendarProvider {...props}><CalendarPanels /></CalendarProvider>;
}
