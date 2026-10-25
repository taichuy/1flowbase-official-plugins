import assert from 'node:assert/strict';

/** Central QA owns authentication, extension installation, services, and viewport.
 * Exercise the real mounted settings component through the real request bridge,
 * replacing only this plugin's HTTP responses. Run once per desktop/mobile viewport.
 */
export async function exerciseBrowserObscuraSettings(page, { setHostLocale } = {}) {
  const prefix = '/api/console/managed-services/browser-obscura';
  const calls = [];
  const browsers = new Map();
  const tabs = new Map();
  const makeBrowser = (browser_id, name) => ({ browser_id, name, status: 'running', created_at_unix_ms: 1792886400000, last_activity_at_unix_ms: 1792886400000, tab_count: 1, idle_timeout_ms: 300000 });
  for (let i = 0; i < 21; i++) {
    const browser_id = `existing-${i}`;
    browsers.set(browser_id, makeBrowser(browser_id, `AI browser ${String(i).padStart(2, '0')}`));
    tabs.set(browser_id, [{ browser_id, tab_id: `tab-existing-${i}`, title: 'Existing AI tab', url: 'https://example.org/', status: 'ready', snapshot_id: null }]);
  }
  let created = 0;
  let failScreenshot = true;
  const languageMenu = page.locator('.app-shell-language-menu');
  const originalLocale = (await languageMenu.locator('.app-shell-language-label').innerText()) === '中' ? 'zh_Hans' : 'en_US';
  const switchLocale = setHostLocale ?? (async locale => {
    await languageMenu.getByRole('menuitem').first().hover();
    await page.getByRole('menuitem', { name: locale === 'en_US' ? 'English' : /^(Chinese|简体中文)$/, exact: true }).click();
  });
  const handler = async route => {
    const request = route.request();
    const url = new URL(request.url());
    const operation = url.pathname.slice(prefix.length);
    const method = request.method();
    const body = method === 'GET' || method === 'DELETE' ? undefined : request.postDataJSON();
    calls.push({ operation, method, body, query: Object.fromEntries(url.searchParams) });
    let result;
    if (operation === '/browsers' && method === 'GET') {
      const currentPage = Number(url.searchParams.get('page'));
      const page_size = Number(url.searchParams.get('page_size'));
      assert.ok(currentPage > 0);
      assert.equal(page_size, 20);
      const search = url.searchParams.get('search') ?? '';
      const all = [...browsers.values()].filter(browser => browser.name.includes(search));
      result = { success: true, items: all.slice((currentPage - 1) * page_size, currentPage * page_size), page: currentPage, page_size, total: all.length };
    } else if (operation === '/browsers' && method === 'POST') {
      assert.equal(created++, 0, 'Create only the authorized fixture browser');
      assert.deepEqual(body, { name: 'Review browser', idle_timeout_ms: 120000 });
      const browser = { ...makeBrowser('created-browser', body.name), tab_count: 0, idle_timeout_ms: body.idle_timeout_ms };
      // Present most recently created first, as the backend's list truth.
      const previous = [...browsers]; browsers.clear(); browsers.set(browser.browser_id, browser);
      for (const [key, value] of previous) browsers.set(key, value);
      tabs.set(browser.browser_id, []);
      result = { success: true, browser };
    } else {
      const parts = operation.split('/').filter(Boolean);
      assert.equal(parts[0], 'browsers');
      const browser_id = parts[1];
      assert.ok(browsers.has(browser_id));
      const browser = browsers.get(browser_id);
      if (parts.length === 2) {
        if (method === 'GET') result = { success: true, browser, tabs: tabs.get(browser_id) };
        else {
          assert.equal(method, 'DELETE');
          assert.equal(browser_id, 'created-browser', 'Never close a pre-existing AI browser');
          browsers.delete(browser_id); tabs.delete(browser_id);
          result = { success: true, closed: true };
        }
      } else if (parts.length === 3) {
        assert.equal(parts[2], 'tabs');
        if (method === 'POST') {
          assert.deepEqual(body, { url: 'https://example.com/' });
          const tab = { browser_id, tab_id: 'created-tab', title: 'Example Domain', url: body.url, status: 'ready', snapshot_id: null };
          tabs.get(browser_id).push(tab); browser.tab_count = tabs.get(browser_id).length;
          result = { success: true, tab };
        } else {
          assert.equal(method, 'GET');
          result = { success: true, items: tabs.get(browser_id), total: tabs.get(browser_id).length };
        }
      } else {
        const tab_id = parts[3];
        const tab = tabs.get(browser_id).find(item => item.tab_id === tab_id);
        assert.ok(tab);
        if (parts.length === 4) {
          assert.equal(method, 'DELETE');
          tabs.set(browser_id, tabs.get(browser_id).filter(item => item.tab_id !== tab_id));
          browser.tab_count = tabs.get(browser_id).length;
          result = { success: true, closed: true };
        } else if (parts[4] === 'navigate') {
          assert.equal(method, 'POST');
          assert.deepEqual(body, { url: 'https://example.com/next' });
          Object.assign(tab, { url: body.url, title: 'Next page', status: 'ready', snapshot_id: 'snapshot-next' });
          result = { success: true, tab };
        } else {
          assert.equal(parts[4], 'screenshot'); assert.equal(method, 'POST'); assert.deepEqual(body, {});
          if (failScreenshot) {
            failScreenshot = false;
            result = { success: false, error: { code: 'navigation_timeout', message: 'PRIVATE_ENGINE_DETAIL_TOKEN' } };
          } else result = {
            success: true, data: 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aZeEAAAAASUVORK5CYII=', mime_type: 'image/png',
            metadata: { browser_id, tab_id, url: tab.url, title: tab.title, width: 1, height: 1 },
          };
        }
      }
    }
    await route.fulfill({ contentType: 'application/json', body: JSON.stringify({ data: result }) });
  };
  await page.route(`**${prefix}/**`, handler);
  const actions = id => page.getByTestId(`browser-actions-${id}`);
  const dialog = name => page.getByRole('dialog', { name, exact: true });
  const confirm = async name => {
    await page.getByRole('tooltip').getByRole('button', { name, exact: true }).click();
  };
  try {
    await switchLocale('en_US');
    await page.goto(new URL('/settings/browser-obscura', page.url()).toString());
    const settings = page.getByTestId('browser-obscura-settings');
    await actions('existing-0').waitFor();
    assert.ok(await settings.locator('.data-table').count(), 'Use the public shared DataTable');
    assert.equal(await settings.getByRole('link', { name: 'MCP settings', exact: true }).getAttribute('href'), '/settings/mcp-management');
    // Deterministic fixture: automatic polling is separately exercised by central QA with its clock.
    await settings.getByRole('switch', { name: 'Auto refresh', exact: true }).click();
    if ((page.viewportSize()?.width ?? 1280) >= 768) {
      await settings.getByTitle('2', { exact: true }).click();
      await actions('existing-20').waitFor();
      await settings.getByTitle('1', { exact: true }).click();
      await actions('existing-0').waitFor();
    } else {
      // The shared mobile list supports both intersection loading and an explicit button.
      const more = settings.locator('.data-table-mobile__pagination button');
      if (await more.count()) await more.click();
      await actions('existing-20').waitFor();
    }
    const search = settings.getByRole('textbox', { name: 'Search browsers', exact: true });
    await search.fill('AI browser 00'); await search.press('Enter');
    await actions('existing-1').waitFor({ state: 'hidden' });
    await actions('existing-0').waitFor();
    assert.equal(await actions('existing-1').count(), 0);
    await search.fill(''); await search.press('Enter');
    await actions('existing-1').waitFor();
    await settings.getByRole('button', { name: 'Create browser', exact: true }).click();
    const create = dialog('Create browser');
    await create.getByLabel('Name', { exact: true }).fill('Review browser');
    await create.getByLabel('Idle timeout (milliseconds)', { exact: true }).fill('120000');
    await create.getByRole('button', { name: 'Create browser', exact: true }).click();
    await create.waitFor({ state: 'hidden' });
    await settings.getByText('Browser created', { exact: true }).waitFor();
    await actions('created-browser').getByRole('button', { name: 'View details', exact: true }).click();
    const detail = dialog('Browser details');
    await detail.getByText('No tabs. Create one to start browsing.', { exact: true }).waitFor();
    await detail.getByLabel('Initial URL (optional)', { exact: true }).fill('https://example.com/');
    await detail.getByRole('button', { name: 'Create tab', exact: true }).click();
    await detail.getByTestId('browser-tab-created-tab').getByText('Example Domain', { exact: true }).waitFor();
    await detail.getByLabel('URL', { exact: true }).fill('https://example.com/next');
    await detail.getByRole('button', { name: 'Navigate', exact: true }).click();
    await detail.getByTestId('browser-tab-created-tab').getByText('https://example.com/next', { exact: true }).waitFor();
    await detail.getByRole('button', { name: 'Take screenshot', exact: true }).click();
    await detail.getByText('The browser did not respond in time. Refresh to check its current state.', { exact: true }).waitFor();
    assert.equal(await settings.getByText(/PRIVATE_ENGINE_DETAIL_TOKEN/).count(), 0);
    assert.equal(await detail.locator('pre').count(), 0);
    await detail.getByRole('button', { name: 'Take screenshot', exact: true }).click();
    const preview = detail.getByTestId('browser-screenshot');
    await preview.getByRole('img', { name: 'Browser tab screenshot', exact: true }).waitFor();
    assert.match(await preview.innerText(), /https:\/\/example.com\/next/);
    await switchLocale('zh_Hans');
    await detail.getByRole('button', { name: '截取屏幕', exact: true }).waitFor();
    await detail.getByTestId('browser-tab-created-tab').getByText('就绪', { exact: true }).waitFor();
    await switchLocale('en_US');
    await detail.getByTestId('browser-tab-created-tab').getByRole('button', { name: 'Close tab', exact: true }).click();
    await confirm('Close tab');
    await detail.getByText('Tab closed', { exact: true }).waitFor();
    assert.equal(await detail.getByTestId('browser-tab-created-tab').count(), 0);
    assert.equal(await detail.getByTestId('browser-screenshot').count(), 0);
    await detail.getByRole('button', { name: 'Done', exact: true }).click();
    await detail.waitFor({ state: 'hidden' });
    await actions('created-browser').getByRole('button', { name: 'Close browser', exact: true }).click();
    await confirm('Close browser');
    await settings.getByText('Browser closed', { exact: true }).waitFor();
    assert.equal(await actions('created-browser').count(), 0);
    await settings.getByRole('button', { name: 'Refresh', exact: true }).click();
    await settings.getByText('Refreshed', { exact: true }).waitFor();
    await page.goto(new URL('/settings/mcp-management', page.url()).toString());
    assert.equal(calls.filter(call => call.method === 'DELETE' && /^\/browsers\/existing-/.test(call.operation)).length, 0, 'Unmount must keep existing browsers alive');
    assert.ok(calls.some(call => call.query.page === '2'), 'Desktop pagination / mobile load more must reach the backend');
    assert.ok(calls.some(call => call.query.search === 'AI browser 00'), 'Filter must be server-side');
    return { created, operations: calls.map(({ operation, method }) => ({ operation, method })), originalLocale };
  } finally {
    await page.unroute(`**${prefix}/**`, handler);
    await switchLocale(originalLocale);
  }
}
