/** QA ONLY. Bundled separately into the Root evidence folder; never shipped in the extension. */
import React, * as ReactNamespace from 'react';
import * as AntdNamespace from 'antd';
import * as DayjsNamespace from 'dayjs';
import * as IconsNamespace from '@ant-design/icons';
import * as StyleNamespace from 'antd-style';
import * as ClsxNamespace from 'clsx';
import { createRoot } from 'react-dom/client';
import 'antd/dist/reset.css';
import { createFrontendModule } from '../../_assets/ui/browser.js';
import { hostModules as contract, componentExports } from '../../scripts/module-contract.mjs';

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
if (Object.keys(components).sort().join(',') !== [...componentExports].sort().join(',')) {
  throw new Error('Factory must return exactly the six public component exports');
}
for (const namespace of Object.values(hostModules)) {
  if (!Object.isFrozen(namespace) || '__esModule' in namespace) throw new Error('Factory mutated a host namespace');
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
    if (!componentExports.includes(name)) throw new Error(`Unknown fixture export ${name}`);
    const Component = components[name];
    root.render(<Component key={`${name}-${JSON.stringify(props)}`} {...props} />);
  },
  unmount() { root.render(null); },
};
