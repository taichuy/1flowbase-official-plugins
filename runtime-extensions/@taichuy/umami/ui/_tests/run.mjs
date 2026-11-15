import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Uses existing host modules and Chrome only; never installs dependencies or starts a service.
const here = dirname(fileURLToPath(import.meta.url));
const hostRoot = process.env.UMAMI_HOST_ROOT ?? '/home/taichuy/git/1flowbase';
const evidenceRoot = process.env.UMAMI_EVIDENCE_ROOT ?? resolve(hostRoot, 'tmp/test-governance/umami-ui');
const hostRequire = createRequire(resolve(hostRoot, 'web/app/package.json'));
function tool(name) {
  for (const base of [resolve(hostRoot, 'web'), resolve(hostRoot, 'web/app')]) {
    try { return createRequire(resolve(base, 'package.json'))(name); }
    catch (error) { if (error.code !== 'MODULE_NOT_FOUND') throw error; }
  }
  throw new Error(`Existing host tool ${name} is unavailable; no installation attempted.`);
}
const { build } = await import(hostRequire.resolve('vite'));
await mkdir(evidenceRoot, { recursive: true });
const outfile = resolve(evidenceRoot, 'settings.js');
const shared = resolve(hostRoot, 'web/app/src/shared/ui/data-table');
await build({
  configFile: false, root: resolve(hostRoot, 'web/app'), logLevel: 'warn',
  resolve: { alias: ['react', 'react-dom', 'antd'].map(name => ({ find: name, replacement: dirname(hostRequire.resolve(`${name}/package.json`)) })) },
  define: { 'process.env.NODE_ENV': '"production"' },
  build: { outDir: evidenceRoot, emptyOutDir: false, minify: false, target: 'es2022',
    lib: { entry: resolve(here, 'settings.fixture.tsx'), name: 'UmamiFixture', formats: ['iife'], fileName: () => 'settings.js', cssFileName: 'settings' },
  },
  plugins: [{ name: 'host-capabilities', resolveId(id) {
    if (id === '@1flowbase/plugin-settings') return resolve(here, 'settings.fixture.tsx');
    if (id === 'host-auth') return resolve(hostRoot, 'web/app/src/state/auth-store.ts');
    if (id === 'host-i18n') return resolve(hostRoot, 'web/app/src/shared/i18n/app-i18n.ts');
    if (id === 'host-theme') return resolve(hostRoot, 'web/packages/ui/src/theme.ts');
    if (id === 'host-shared-ui-en') return resolve(hostRoot, 'web/app/src/shared/ui/i18n/en_US.json');
    if (id === 'host-shared-ui-zh') return resolve(hostRoot, 'web/app/src/shared/ui/i18n/zh_Hans.json');
    if (id === '@1flowbase/data-table') return '\0host-table';
  }, load(id) {
    if (id === '\0host-table') return `export { DataTable, DataTableColumnSettings } from ${JSON.stringify(resolve(shared, 'DataTable.tsx'))};\nexport { DataTableLayout } from ${JSON.stringify(resolve(shared, 'DataTableLayout.tsx'))};\nexport { usePersistedDataTableConfiguration } from ${JSON.stringify(resolve(shared, 'data-table-state.ts'))};`;
  } }],
});
if (process.argv.includes('--build-only')) { console.log(JSON.stringify({ status: 'compiled', outfile })); process.exit(0); }

const { chromium } = tool('playwright');
const browser = await chromium.launch({ executablePath: process.env.UMAMI_CHROME_EXECUTABLE ?? '/usr/bin/google-chrome', headless: true });
const report = { status: 'running', scenarios: [], errors: [] };
try {
  for (const viewport of [{ width: 1365, height: 1000 }, { width: 390, height: 844 }]) {
    const label = viewport.width < 768 ? 'mobile' : 'desktop';
    const context = await browser.newContext({ viewport });
    try {
      const page = await context.newPage();
      page.setDefaultTimeout(10_000);
      page.on('pageerror', error => report.errors.push(`${label}: ${error.message}`));
      await page.route('https://fixture.invalid/', route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><html><body style="margin:0"><div id="root"></div></body></html>' }));
      await page.goto('https://fixture.invalid/');
      await page.addStyleTag({ content: await readFile(outfile.replace(/\.js$/, '.css'), 'utf8') });
      await page.addScriptTag({ content: await readFile(outfile, 'utf8') });
      const actions = id => page.getByTestId(`umami-actions-${id}`);
      const button = name => page.getByRole('button', { name, exact: true });
      const callFixture = (method, ...args) => page.evaluate(({ method, args }) => window.umamiFixture[method](...args), { method, args });
      const calls = () => callFixture('calls');
      const drawer = page.getByRole('dialog', { name: 'Add connection', exact: true });
      await actions('existing-0').waitFor();
      assert.ok(await page.locator('.data-table').count(), 'Real shared DataTable is mounted');
      assert.equal(await actions('existing-1').getByRole('button', { name: 'Test connection', exact: true }).isDisabled(), true);
      await button('Columns').click();
      await page.getByRole('checkbox', { name: 'Username', exact: true }).uncheck();
      await page.getByRole('checkbox', { name: 'Username', exact: true }).press('Escape');
      await page.locator('.data-table-columns').waitFor({ state: 'hidden' });
      await page.waitForFunction(() => !JSON.parse(localStorage.getItem('1flowbase.umami.hosts.columns.v1')).visibleColumnKeys.includes('username'));
      await callFixture('mount', { isRoot: true }); await actions('existing-0').waitFor();
      await button('Columns').click();
      assert.equal(await page.getByRole('checkbox', { name: 'Username', exact: true }).isChecked(), false, 'Column choice persists across mounts');
      await page.getByRole('button', { name: 'Reset columns', exact: true }).click();
      assert.equal(await page.getByRole('checkbox', { name: 'Username', exact: true }).isChecked(), true);
      await page.getByRole('checkbox', { name: 'Username', exact: true }).press('Escape');
      await page.locator('.data-table-columns').waitFor({ state: 'hidden' });
      await page.screenshot({ path: resolve(evidenceRoot, `${label}-list.png`), fullPage: true });
      if (label === 'desktop') {
        await page.getByTitle('2', { exact: true }).click(); await actions('existing-20').waitFor();
        await page.getByTitle('1', { exact: true }).click(); await actions('existing-0').waitFor();
      } else {
        const more = page.locator('.data-table-mobile__pagination button');
        await more.waitFor(); await more.click();
        await more.waitFor({ state: 'hidden' });
        await page.waitForFunction(() => {
          if (document.querySelector('[data-testid="umami-actions-existing-20"]')) return true;
          // Virtual rows are measured after each scroll; follow the changing height until the final row mounts.
          for (const child of document.querySelectorAll('.data-table-mobile__viewport *')) if (child.scrollHeight > child.clientHeight + 50 && getComputedStyle(child).overflowY !== 'visible') { child.scrollTop = child.scrollHeight; child.dispatchEvent(new Event('scroll')); }
          return false;
        });
        await actions('existing-20').waitFor();
      }
      const search = page.getByRole('searchbox', { name: 'Search connections', exact: true });
      await search.fill('Analytics 00'); await search.press('Enter');
      await actions('existing-1').waitFor({ state: 'hidden' });
      assert.ok((await calls()).some(call => call.path.includes('search=Analytics+00')), 'Search is sent to backend');
      await search.fill(''); await search.press('Enter'); await actions('existing-1').waitFor();
      await button('Add connection').click(); await drawer.waitFor();
      await drawer.getByLabel('Name', { exact: true }).fill('Review analytics');
      await drawer.getByLabel('Umami URL', { exact: true }).fill('https://review.example.org');
      await drawer.getByLabel('Username', { exact: true }).fill('reviewer');
      await drawer.getByLabel('Password', { exact: true }).fill('fixture-password');
      await callFixture('failNext', 'umami_authentication_failed');
      await drawer.getByRole('button', { name: 'Test these settings', exact: true }).click();
      await drawer.getByText('Umami could not authenticate this account. Check the username and password.', { exact: true }).waitFor();
      assert.equal((await calls()).filter(call => call.method === 'POST' && call.path.endsWith('/hosts')).length, 0, 'Draft test must not save');
      assert.equal(await page.getByText('PRIVATE_PROVIDER_SECRET_DETAIL').count(), 0);
      await drawer.getByRole('button', { name: 'Test these settings', exact: true }).click();
      await drawer.getByText('Connection authenticated successfully', { exact: true }).waitFor();
      await drawer.getByLabel('Username', { exact: true }).fill('changed-reviewer');
      await drawer.getByText('Connection authenticated successfully', { exact: true }).waitFor({ state: 'hidden' });
      await page.screenshot({ path: resolve(evidenceRoot, `${label}-drawer.png`), fullPage: true });
      await drawer.getByRole('button', { name: 'Save', exact: true }).click();
      await drawer.waitFor({ state: 'hidden' }); await actions('created-1').waitFor();
      const created = (await calls()).find(call => call.method === 'POST' && call.path.endsWith('/hosts'));
      assert.deepEqual(created.body, { name: 'Review analytics', base_url: 'https://review.example.org', username: 'changed-reviewer', enabled: true, password: 'fixture-password' });
      await actions('created-1').getByRole('button', { name: 'Edit', exact: true }).click();
      const edit = page.getByRole('dialog', { name: 'Edit connection', exact: true }); await edit.waitFor();
      assert.equal(await edit.getByLabel('Password', { exact: true }).inputValue(), '', 'Saved password must never repopulate');
      await edit.getByText('Leave empty to keep the saved password.', { exact: true }).waitFor();
      await edit.getByRole('button', { name: 'Test these settings', exact: true }).click();
      await edit.getByText('Connection authenticated successfully', { exact: true }).waitFor();
      const savedDraftTest = (await calls()).filter(call => call.path.endsWith('/hosts/test')).at(-1);
      assert.equal(savedDraftTest.body.host_id, 'created-1'); assert.equal('password' in savedDraftTest.body, false);
      await edit.getByLabel('Name', { exact: true }).fill('Updated analytics');
      await edit.getByRole('button', { name: 'Save', exact: true }).click(); await edit.waitFor({ state: 'hidden' });
      await page.getByText('Connection saved', { exact: true }).waitFor();
      const updated = (await calls()).find(call => call.method === 'PUT');
      assert.equal(updated.body.name, 'Updated analytics'); assert.equal('password' in updated.body, false, 'Blank edit password must be omitted');
      await actions('created-1').getByRole('button', { name: 'Test connection', exact: true }).click();
      await page.getByText('Connection authenticated successfully', { exact: true }).waitFor();
      const savedTest = (await calls()).find(call => call.path.endsWith('/hosts/created-1/test'));
      assert.equal(savedTest.body, undefined);
      await actions('created-1').getByRole('button', { name: 'More actions', exact: true }).click();
      await page.getByRole('menuitem', { name: 'Delete', exact: true }).click();
      await page.getByRole('dialog', { name: 'Delete connection?', exact: true }).getByRole('button', { name: 'Delete', exact: true }).click();
      await page.getByText('Connection deleted', { exact: true }).waitFor();
      assert.equal((await calls()).filter(call => call.method === 'DELETE').length, 1);
      assert.equal(await actions('created-1').count(), 0);
      report.scenarios.push(`${label}: server search, disabled host, create, draft failure and success, stale feedback, edit secret omission, saved test, confirmed delete`);

      await callFixture('mount', { permissions: ['umami.hosts.list'] }); await actions('existing-0').waitFor({ state: 'attached' });
      assert.equal(await button('Add connection').count(), 0); assert.equal(await actions('existing-0').getByRole('button').count(), 0);
      const before = (await calls()).length;
      await callFixture('mount', {}); await page.getByText('You do not have permission to view Umami connections.', { exact: true }).waitFor();
      assert.equal((await calls()).length, before, 'Absent permissions must not issue a list request');
      await callFixture('mount', { permissions: ['umami.hosts.list', 'umami.hosts.update'] }); await actions('existing-0').waitFor();
      assert.equal(await actions('existing-0').getByRole('button').count(), 1);
      await actions('existing-0').getByRole('button', { name: 'Edit', exact: true }).click();
      await page.getByRole('dialog', { name: 'Edit connection', exact: true }).waitFor();
      assert.equal(await button('Test these settings').count(), 0);
      await button('Cancel').click();
      await callFixture('mount', { isRoot: true }, 'zh_Hans');
      await page.getByRole('button', { name: '添加连接', exact: true }).waitFor();
      await page.getByText('Umami 连接', { exact: true }).waitFor();
      await callFixture('mount', { isRoot: true }); await actions('existing-0').waitFor();
      await button('Add connection').click(); await drawer.waitFor();
      for (const [field, value] of [['Name', 'Abort draft'], ['Umami URL', 'https://abort.example.org'], ['Username', 'abort'], ['Password', 'fixture-password']]) await drawer.getByLabel(field, { exact: true }).fill(value);
      const previousAborts = await callFixture('aborts');
      await callFixture('holdNext'); await drawer.getByRole('button', { name: 'Test these settings', exact: true }).click();
      await page.waitForFunction(() => window.umamiFixture.calls().at(-1)?.path.endsWith('/hosts/test'));
      assert.equal(await drawer.getByLabel('Username', { exact: true }).isDisabled(), true, 'Pending draft cannot change under a test result');
      await callFixture('unmount'); await page.waitForFunction(previous => window.umamiFixture.aborts() === previous + 1, previousAborts);
      report.scenarios.push(`${label}: read-only and missing permission gates, operation-specific edit/test gates, root override, Chinese locale, abort on unmount`);
    } finally { await context.close(); }
  }
  assert.deepEqual(report.errors, [], 'Page errors must not be swallowed');
  report.status = 'passed';
} catch (error) { report.status = 'failed'; report.failure = error.stack; process.exitCode = 1; }
finally { await browser.close(); await writeFile(resolve(evidenceRoot, 'report.json'), JSON.stringify(report, null, 2)); }
console.log(JSON.stringify(report));
