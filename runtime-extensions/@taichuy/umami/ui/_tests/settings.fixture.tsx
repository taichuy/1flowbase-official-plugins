import React from 'react';
import * as ReactModule from 'react';
import * as JsxRuntime from 'react/jsx-runtime';
import { createRoot } from 'react-dom/client';
import { ConfigProvider } from 'antd';
import * as Antd from 'antd';
import * as DataTable from '@1flowbase/data-table';
import { compileNativeReactComponent } from 'host-native-compiler';
import { evaluateNativeReactComponentArtifact } from 'host-native-evaluator';
import { useAuthStore } from 'host-auth';
import { appI18n } from 'host-i18n';
import { emeraldLightTheme } from 'host-theme';
import sharedUiEn from 'host-shared-ui-en';
import sharedUiZh from 'host-shared-ui-zh';
import settingsSource from '../settings.tsx?raw';
import developmentSource from '../page.tsx?raw';
import type { HostSummary } from '../api';

const modules = Object.freeze({
  react: Object.freeze({ ...ReactModule, default: React }),
  'react/jsx-runtime': Object.freeze({ ...JsxRuntime }),
  antd: Object.freeze({ ...Antd }),
  '@1flowbase/data-table': Object.freeze({ ...DataTable }),
  '@1flowbase/plugin-settings': Object.freeze({ request }),
});
const moduleDefinitions = Object.entries(modules).map(([module_source, namespace]) => ({ module_source, exports: Object.keys(namespace) }));
const compiled = compileNativeReactComponent(settingsSource, moduleDefinitions);
if (!compiled.ok) throw new Error(`Delivered settings failed NativeReact compilation: ${JSON.stringify(compiled.diagnostics)}`);
// A plain Vite page import accepted this old layout; the actual host rejects its relative imports.
const rejectedDevelopmentSource = compileNativeReactComponent(developmentSource, moduleDefinitions);
if (rejectedDevelopmentSource.ok || !rejectedDevelopmentSource.diagnostics.some(item => item.message.includes('./api') || item.message.includes('./copy'))) {
  throw new Error('Compiler regression probe must reject the original page with relative imports.');
}
const evaluated = evaluateNativeReactComponentArtifact(compiled.artifact, modules);
if (!evaluated.ok) throw new Error(`Delivered settings failed NativeReact evaluation: ${JSON.stringify(evaluated.diagnostics)}`);
const UmamiSettingsPage = evaluated.component as React.ComponentType<{ ctx: object }>;

const root = createRoot(document.getElementById('root')!);
appI18n.addResourceBundle('en_US', 'sharedUi', sharedUiEn);
appI18n.addResourceBundle('zh_Hans', 'sharedUi', sharedUiZh);
const hosts = new Map<string, HostSummary>();
let sequence = 0;
let nextFailure: string | undefined;
let hold = false;
let mounted = 0;
const calls: { path: string; method: string; body?: unknown }[] = [];
let aborts = 0;

function reset() {
  hosts.clear(); calls.length = 0; nextFailure = undefined; hold = false; aborts = 0;
  for (let i = 0; i < 21; i++) hosts.set(`existing-${i}`, { host_id: `existing-${i}`, name: `Analytics ${String(i).padStart(2, '0')}`, base_url: 'https://analytics.example.org/umami', username: 'analyst', enabled: i !== 1, has_password: true });
}

// Only the request capability is replaced. The page, forms and shared DataTable are real.
export async function request({ path, method = 'GET', body, signal }: { path: string; method?: string; body?: any; signal: AbortSignal }) {
  calls.push({ path, method, body });
  const held = hold; hold = false;
  await new Promise<void>((resolve, reject) => {
    const abort = () => { aborts++; clearTimeout(timer); reject(new DOMException('Aborted', 'AbortError')); };
    const timer = held ? undefined : setTimeout(() => { signal.removeEventListener('abort', abort); resolve(); }, 15);
    signal.addEventListener('abort', abort, { once: true });
    if (signal.aborted) abort();
  });
  if (nextFailure) { const code = nextFailure; nextFailure = undefined; throw { code, message: 'PRIVATE_PROVIDER_SECRET_DETAIL' }; }
  const url = new URL(path, 'https://fixture.invalid');
  const parts = url.pathname.split('/').slice(5);
  if (parts[0] !== 'hosts') throw new Error('Unexpected API path');
  if (parts.length === 1 && method === 'GET') {
    const page = Number(url.searchParams.get('page'));
    const page_size = Number(url.searchParams.get('page_size'));
    const search = url.searchParams.get('search') ?? '';
    const items = [...hosts.values()].filter(host => host.name.includes(search));
    return { success: true, items: items.slice((page - 1) * page_size, page * page_size), total: items.length, page, page_size };
  }
  if (parts[1] === 'test' || parts[2] === 'test') return { success: true, authenticated: true, user: { username: 'analyst' } };
  if (method === 'DELETE') { hosts.delete(parts[1]); return { success: true }; }
  if (method === 'POST' || method === 'PUT') {
    const host_id = method === 'POST' ? `created-${++sequence}` : parts[1];
    const host = { host_id, name: body.name, base_url: body.base_url, username: body.username, enabled: body.enabled, has_password: Boolean(body.password) || Boolean(hosts.get(host_id)?.has_password) };
    const previous = [...hosts]; hosts.clear(); hosts.set(host_id, host);
    for (const [id, value] of previous) if (id !== host_id) hosts.set(id, value);
    return { success: true, host };
  }
  throw new Error('Unexpected fixture operation');
}

const fixture = {
  compiler: {
    source: 'ui/settings.tsx', sourceSha256: compiled.artifact.identity.source_sha256,
    modules: compiled.artifact.program.injectedModules.map(item => item.source),
    developmentSourceRejected: true, defaultExportEvaluated: true,
    developmentSourceDiagnostics: rejectedDevelopmentSource.diagnostics,
  },
  mount(currentUser: { isRoot?: boolean; permissions?: string[] } | undefined = { isRoot: true }, locale = 'en_US') {
    useAuthStore.setState({ actor: { effective_display_role: 'root' } });
    void appI18n.changeLanguage(locale);
    const ctx = Object.freeze({ ui: Object.freeze({ locale }), currentUser: currentUser && Object.freeze({ ...currentUser, permissions: currentUser.permissions && Object.freeze([...currentUser.permissions]) }) });
    root.render(<ConfigProvider theme={emeraldLightTheme}><UmamiSettingsPage key={++mounted} ctx={ctx} /></ConfigProvider>);
  },
  unmount() { root.render(null); }, reset,
  calls: () => calls, aborts: () => aborts,
  failNext(code: string) { nextFailure = code; }, holdNext() { hold = true; },
};
Object.assign(window, { umamiFixture: fixture });
reset(); fixture.mount();
