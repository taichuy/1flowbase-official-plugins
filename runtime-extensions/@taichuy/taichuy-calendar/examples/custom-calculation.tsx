import React from 'react';
import { Solar } from 'lunar-typescript';
import { SolarDay } from 'tyme4ts';
import { astro } from 'iztro';
import { SunPosition } from 'astronomy-engine';
import { pcaC } from 'china-division';

/** No full-page/calendar component required: calculations can drive your own UI. */
export default function CustomCalculation() {
  const date = new Date('2000-01-01T12:00:00Z');
  const lunar = Solar.fromYmd(2000, 1, 1).getLunar();
  const chart = astro.bySolar('2000-1-1', 6, '男', true, 'zh-CN');
  return <section data-calendar-custom="packages">
    <div data-package="lunar-typescript">{lunar.toString()}</div>
    <div data-package="tyme4ts">{SolarDay.fromYmd(2000, 1, 1).toString()}</div>
    <div data-package="iztro">{chart.palaces[0].name}</div>
    <div data-package="astronomy-engine">{SunPosition(date).elon.toFixed(3)}</div>
    <div data-package="china-division">{pcaC[0].name}</div>
  </section>;
}
