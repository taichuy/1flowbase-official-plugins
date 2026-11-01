/** QA ONLY. Bundled separately into the Root evidence folder; never shipped in the extension. */
import React, * as ReactNamespace from 'react';
import * as AntdNamespace from 'antd';
import * as DayjsNamespace from 'dayjs';
import * as IconsNamespace from '@ant-design/icons';
import * as StyleNamespace from 'antd-style';
import * as ClsxNamespace from 'clsx';
import { createFrontendModule } from '../../_assets/ui/browser.js';
import { hostModules as contract, publicExports } from '../../scripts/module-contract.mjs';

const all: Record<string, Record<string, any>> = {
  react: ReactNamespace, antd: AntdNamespace, dayjs: DayjsNamespace,
  '@ant-design/icons': IconsNamespace, 'antd-style': StyleNamespace,
  clsx: ClsxNamespace,
};
// Emulate frozen native ESM namespaces WITHOUT the CJS __esModule interop flag.
const hostModules = Object.freeze(Object.fromEntries(contract.map(({ source, exports: names }) => {
  const namespace = Object.fromEntries(names.map((name) => {
    if (!(name in all[source])) throw new Error(`Fixture host missing declared export ${source}:${name}`);
    return [name, all[source][name]];
  }));
  return [source, Object.freeze(namespace)];
})));
const incomplete = { ...hostModules };
delete incomplete.dayjs;
let rejected = false;
try { createFrontendModule(incomplete); } catch (error) {
  rejected = /Missing frontend host module: dayjs/.test(String(error));
}
if (!rejected) throw new Error('Factory accepted a missing host binding');
// Do not preload locale or clipboard in the harness: the shipped factory must supply them.
for (const source of ['copy-to-clipboard', 'antd/locale/zh_CN', 'dayjs/locale/zh-cn']) {
  if (source in hostModules) throw new Error(`Unexpected host dependency ${source}`);
}
if (typeof hostModules.dayjs.default !== 'function' || typeof hostModules.react.default.createElement !== 'function') {
  throw new Error('Actual host default exports do not match the factory ABI');
}
const components = createFrontendModule(hostModules);
if (hostModules.dayjs.default('2026-11-01').locale('zh-cn').format('dddd') !== '星期日') {
  throw new Error('Bundled Chinese locale did not register on the external host dayjs singleton');
}
if (Object.keys(components).sort().join(',') !== [...publicExports].sort().join(',')) {
  throw new Error('Factory must return exactly the public export contract');
}
for (const namespace of Object.values(hostModules)) {
  if (!Object.isFrozen(namespace) || '__esModule' in namespace) throw new Error('Factory mutated a host namespace');
}

export { components, hostModules };
export const { CalendarProvider, Calendar, Almanac, BirthInfo, ZiWei, Bazi,
  useBirthInfo, useCalendarDate } = components;
