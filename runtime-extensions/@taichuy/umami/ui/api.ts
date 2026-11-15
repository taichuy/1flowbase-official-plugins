import { request } from '@1flowbase/plugin-settings';

export type HostSummary = { host_id: string; name: string; base_url: string; username: string; enabled: boolean; has_password: boolean };
export type HostList = { items: HostSummary[]; total: number; page: number; page_size: number };
export type HostDraft = { name: string; base_url: string; username: string; enabled: boolean; password?: string };
type Result<T = object> = { success: true } & T;
const root = '/api/console/managed-services/umami';
const hostPath = (host_id: string) => `/hosts/${encodeURIComponent(host_id)}`;
const call = <T,>(path: string, method: 'GET' | 'POST' | 'PUT' | 'DELETE', signal: AbortSignal, body?: unknown) =>
  request<Result<T>>({ path: `${root}${path}`, method, body, signal });

export const hostsApi = {
  list(signal: AbortSignal, page: number, page_size: number, search: string) {
    const query = new URLSearchParams({ page: String(page), page_size: String(page_size) });
    if (search) query.set('search', search);
    return call<HostList>(`/hosts?${query}`, 'GET', signal);
  },
  save(signal: AbortSignal, body: HostDraft, host_id?: string) {
    return call<{ host: HostSummary }>(host_id ? hostPath(host_id) : '/hosts', host_id ? 'PUT' : 'POST', signal, body);
  },
  delete: (signal: AbortSignal, host_id: string) => call(hostPath(host_id), 'DELETE', signal),
  test: (signal: AbortSignal, host_id: string) => call<{ authenticated: true; user: object }>(`${hostPath(host_id)}/test`, 'POST', signal),
  testDraft: (signal: AbortSignal, body: HostDraft, host_id?: string) =>
    call<{ authenticated: true; user: object }>('/hosts/test', 'POST', signal, { ...body, ...(host_id ? { host_id } : {}) }),
};

export function draftBody(values: HostDraft): HostDraft {
  return {
    name: values.name.trim(), base_url: values.base_url.trim(), username: values.username.trim(), enabled: values.enabled,
    // An empty password is intentionally omitted: editing retains the backend secret.
    ...(values.password ? { password: values.password } : {}),
  };
}
