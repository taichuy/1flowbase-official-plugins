/** QA ONLY: actual shipped factory plus the same editable example used in the template. */
import React from 'react';
import { createRoot } from 'react-dom/client';
import 'antd/dist/reset.css';
import { components, CalendarProvider, useBirthInfo, useCalendarDate } from './public-components.fixture';
import { componentExports } from '../../scripts/module-contract.mjs';
import ComposedPage from '../../examples/composed-page';
import type { ChartAIHelperData } from '../../types';
function Probe({ name }: { name: string }) {
  const calendar = useCalendarDate();
  const birth = useBirthInfo();
  return <div data-provider={name}>
    <output data-value="calendar">{calendar.selectedDate.format('YYYY-MM-DD')}</output>
    <output data-value="birth">{birth.birthDate.format('YYYY-MM-DD')}</output>
    <button onClick={() => calendar.setSelectedDate(calendar.selectedDate.add(1, 'day'))}>Calendar +1</button>
    <button onClick={() => birth.setBirthDate(birth.birthDate.add(1, 'day'))}>Birth +1</button>
  </div>;
}
function Isolation() {
  return <div data-calendar-component="Isolation">
    <CalendarProvider initialDate="2026-11-01" initialBirthDate="2000-01-01"><Probe name="a" /><Probe name="sibling" /></CalendarProvider>
    <CalendarProvider initialDate="2026-11-01" initialBirthDate="2000-01-01"><Probe name="b" /></CalendarProvider>
  </div>;
}
function AssistantProbe({ data }: { data: ChartAIHelperData }) {
  return <section data-custom-assistant={data.kind}>
    <textarea aria-label="custom-natal" readOnly value={data.natalText} />
    <button onClick={data.copyNatal}>custom-copy-natal</button>
    {data.kind === 'ziwei' ? <>
      <textarea aria-label="custom-horoscope" readOnly value={data.horoscopeText} />
      <output data-view-date>{data.viewDate}</output>
      <button onClick={data.copyHoroscope}>custom-copy-horoscope</button>
    </> : <output data-decade-index>{data.selectedDaYunIndex}</output>}
  </section>;
}
function AssistantSlot({ kind = 'ziwei', hidden = false, ...props }) {
  const Chart = kind === 'ziwei' ? components.ZiWei : components.Bazi;
  return <CalendarProvider {...props}><Probe name="slot" />
    <Chart renderAIHelper={data => hidden ? null : <AssistantProbe data={data} />} />
  </CalendarProvider>;
}
const root = createRoot(document.getElementById('fixture-root')!);
const clipboard: string[] = [];
Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
  writeText: async (text: string) => { clipboard.push(text); },
} });
document.execCommand = (command: string) => {
  if (command !== 'copy') return false;
  clipboard.push(window.getSelection()?.toString() ?? '');
  return true;
};
// Expose test commands/results, not React or a global hostModules bridge.
(window as any).calendarFixture = {
  exports: Object.keys(components), clipboard, factoryBound: true,
  mount(name: string, props = {}) {
    if (![...componentExports, 'ComposedPage', 'Isolation', 'AssistantSlot'].includes(name)) throw new Error(`Unknown fixture export ${name}`);
    const Component = name === 'AssistantSlot' ? AssistantSlot : name === 'ComposedPage' ? ComposedPage : name === 'Isolation' ? Isolation : components[name];
    root.render(<div data-fixture-key={`${name}-${JSON.stringify(props)}`}><Component key={`${name}-${JSON.stringify(props)}`} {...props} /></div>);
  },
  unmount() { root.render(null); },
};
