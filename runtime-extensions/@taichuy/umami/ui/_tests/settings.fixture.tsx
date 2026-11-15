import React from 'react';
import { createRoot } from 'react-dom/client';
import { ConfigProvider } from 'antd';
import { useAuthStore } from 'host-auth';
import { appI18n } from 'host-i18n';
import { emeraldLightTheme } from 'host-theme';
import sharedUiEn from 'host-shared-ui-en';
import sharedUiZh from 'host-shared-ui-zh';
import UmamiSettingsPage from '../page';
import type { HostSummary } from '../api';

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
  mount(currentUser: { isRoot?: boolean; permissions?: string[] } | undefined = { isRoot: true }, locale = 'en_US') {
    useAuthStore.setState({ actor: { effective_display_role: 'root' } });
    void appI18n.changeLanguage(locale);
    root.render(<ConfigProvider theme={emeraldLightTheme}><UmamiSettingsPage key={++mounted} ctx={{ ui: { locale }, currentUser }} /></ConfigProvider>);
  },
  unmount() { root.render(null); }, reset,
  calls: () => calls, aborts: () => aborts,
  failNext(code: string) { nextFailure = code; }, holdNext() { hold = true; },
};
Object.assign(window, { umamiFixture: fixture });
reset(); fixture.mount();
