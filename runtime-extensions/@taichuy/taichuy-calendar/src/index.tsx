import React from 'react';
import TaichuyCalendarCard from './components/TaichuyCalendarCard';
import LunarInfoCard from './components/divination/LunarInfoCard';
import ZiWeiCalculator from './components/divination/ZiWeiCalculator';
import BaziCalculator from './components/divination/BaziCalculator';
import BirthInfoForm from './page/BirthInfo';
import CalendarPageContent from './page/CalendarPage';
import { PluginBoundary, usePluginContext, type CalendarComponentProps } from './state/PluginContext';
import { useDateContext } from './state/DateContext';

function AlmanacContent() {
  const { selectedDate } = useDateContext();
  return <LunarInfoCard date={selectedDate} />;
}

function ChartContent({ kind }: { kind: 'ziwei' | 'bazi' }) {
  const { trueSolarDateTime, gender, locationInfo, chartRevision } = usePluginContext();
  const Component = kind === 'ziwei' ? ZiWeiCalculator : BaziCalculator;
  return <Component key={chartRevision} date={trueSolarDateTime.date} time={trueSolarDateTime.time}
    gender={gender} locationInfo={locationInfo} />;
}

/** Standalone lunar month/year calendar, including today and session-only sign actions. */
export function Calendar(props: CalendarComponentProps = {}) {
  return <PluginBoundary {...props}><section data-calendar-component="Calendar"><TaichuyCalendarCard /></section></PluginBoundary>;
}
export function Almanac(props: CalendarComponentProps = {}) {
  return <PluginBoundary {...props}><section data-calendar-component="Almanac"><AlmanacContent /></section></PluginBoundary>;
}
export function BirthInfo(props: CalendarComponentProps = {}) {
  return <PluginBoundary {...props}><section data-calendar-component="BirthInfo"><BirthInfoForm /></section></PluginBoundary>;
}
export function ZiWei(props: CalendarComponentProps = {}) {
  return <PluginBoundary {...props}><section data-calendar-component="ZiWei"><ChartContent kind="ziwei" /></section></PluginBoundary>;
}
export function Bazi(props: CalendarComponentProps = {}) {
  return <PluginBoundary {...props}><section data-calendar-component="Bazi"><ChartContent kind="bazi" /></section></PluginBoundary>;
}
export function CalendarPage(props: CalendarComponentProps = {}) {
  return <PluginBoundary {...props}><CalendarPageContent /></PluginBoundary>;
}
