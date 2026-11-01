/** QA ONLY: actual shipped factory plus the same editable example used in the template. */
import React from 'react';
import { createRoot } from 'react-dom/client';
import 'antd/dist/reset.css';
import { components, CalendarProvider, useBirthInfo, useCalendarDate } from './public-components.fixture';
import { componentExports } from '../../scripts/module-contract.mjs';
import ComposedPage from '../../examples/composed-page';
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
    if (![...componentExports, 'ComposedPage', 'Isolation'].includes(name)) throw new Error(`Unknown fixture export ${name}`);
    const Component = name === 'ComposedPage' ? ComposedPage : name === 'Isolation' ? Isolation : components[name];
    root.render(<Component key={`${name}-${JSON.stringify(props)}`} {...props} />);
  },
  unmount() { root.render(null); },
};
