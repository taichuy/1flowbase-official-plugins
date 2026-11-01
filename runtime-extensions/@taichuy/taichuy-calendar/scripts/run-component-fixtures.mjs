import assert from 'node:assert/strict';
import { constants } from 'node:fs';
import { access, readFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { hostRoot, loadHostTool } from './toolchain.mjs';

// No web server, auth API, network, downloads, or persistent browser profile.
const evidenceRoot = process.env.TAICHUY_EVIDENCE_ROOT ?? resolve(hostRoot, 'tmp/test-governance/taichuy-calendar');
const script = await readFile(resolve(evidenceRoot, 'compiled/factory-browser.js'), 'utf8');
const css = await readFile(resolve(evidenceRoot, 'compiled/factory-browser.css'), 'utf8');
const expected = JSON.parse(await readFile(new URL('../src/_tests/source-expectations.json', import.meta.url), 'utf8'));
const { chromium } = loadHostTool('playwright');
// Use an existing browser only, never Playwright's downloaded headless_shell.
const executablePath = process.env.TAICHUY_CHROME_EXECUTABLE ?? '/usr/bin/google-chrome';
let browser;
const report = { status: 'running', timezone: expected.timezone, scenarios: [], errors: [], requests: [], failures: [] };
const props = { initialDate: expected.birthDate, initialBirthDate: expected.birthDate,
  initialTime: expected.time, initialUseTrueSolarTime: false };
await mkdir(evidenceRoot, { recursive: true });
async function waitFor(page, predicate, message) {
  await page.waitForFunction(predicate, null, { timeout: 15_000 }).catch((error) => { throw new Error(`${message}: ${error.message}`); });
}
async function mount(page, name, options = props) {
  await page.evaluate(({ name, options }) => window.calendarFixture.mount(name, options), { name, options });
  await page.locator(`[data-calendar-component="${name}"]`).waitFor();
}
async function clipboardCopy(page, click, expectedText) {
  const before = await page.evaluate(() => window.calendarFixture.clipboard.length);
  await click();
  await page.waitForFunction((before) => window.calendarFixture.clipboard.length > before, before);
  const text = await page.evaluate(() => window.calendarFixture.clipboard.at(-1));
  assert.match(text, expectedText);
}
try {
  await access(executablePath, constants.X_OK).catch((error) => {
    throw new Error(`Chrome is not executable at ${executablePath}; set TAICHUY_CHROME_EXECUTABLE to an existing Chrome/Chromium executable. No browser is downloaded.`, { cause: error });
  });
  browser = await chromium.launch({ headless: true, executablePath });
  for (const viewport of [{ width: 1440, height: 1100 }, { width: 390, height: 844 }]) {
    const label = viewport.width > 768 ? 'desktop' : 'mobile';
    const context = await browser.newContext({ viewport, timezoneId: expected.timezone, locale: 'zh-CN' });
    try {
      await context.route('**/*', (route) => { report.requests.push(route.request().url()); return route.abort(); });
      const page = await context.newPage();
      page.on('pageerror', (error) => report.errors.push(`${label}: ${error.message}`));
      page.on('console', (message) => { if (message.type() === 'error') report.errors.push(`${label}: ${message.text()}`); });
      // Freeze Date while leaving animation/timers live for Antd and React.
      await page.clock.setFixedTime(new Date(expected.clock));
      await page.setContent('<!doctype html><html><head><meta charset="utf-8"></head><body><div id="fixture-root"></div></body></html>');
      await page.addStyleTag({ content: css });
      await page.addScriptTag({ content: script });
      assert.equal(await page.evaluate(() => window.calendarFixture.factoryBound), true);
      for (const name of ['Calendar', 'Almanac', 'BirthInfo', 'ZiWei', 'Bazi']) {
        await mount(page, name);
        if (name === 'ZiWei') await page.locator('[data-calendar-section="ziwei"]').waitFor();
        if (name === 'Bazi') await page.locator('[data-calendar-section="bazi"]').waitFor();
        assert.ok((await page.locator(`[data-calendar-component="${name}"]`).innerText()).length > 0);
        report.scenarios.push(`${label}: independent ${name} through real factory`);
      }
      await mount(page, 'CalendarPage');
      assert.ok(await page.locator('[data-calendar-component=CalendarPage]').count(), 'Legacy export stays usable');
      await mount(page, 'Isolation');
      await page.locator('[data-provider=a]').getByText('Calendar +1', { exact: true }).click();
      assert.equal(await page.locator('[data-provider=sibling] [data-value=calendar]').textContent(), '2026-11-02');
      assert.equal(await page.locator('[data-provider=b] [data-value=calendar]').textContent(), '2026-11-01');
      await page.locator('[data-provider=a]').getByText('Birth +1', { exact: true }).click();
      assert.equal(await page.locator('[data-provider=sibling] [data-value=birth]').textContent(), '2000-01-02');
      assert.equal(await page.locator('[data-provider=b] [data-value=birth]').textContent(), '2000-01-01');
      report.scenarios.push(`${label}: Provider sibling linkage, date/birth independence and tree isolation`);
      await mount(page, 'ComposedPage');
      await page.screenshot({ path: resolve(evidenceRoot, `plugin-${label}-initial.png`), fullPage: true });
      const calendar = page.locator('[data-calendar-section="calendar"]:visible');
      const birthDate = page.getByRole('textbox', { name: '出生日期', exact: true });
      assert.equal(await birthDate.inputValue(), '2026-11-01');
      await calendar.locator('[data-calendar-date="2026-11-02"]').click();
      await waitFor(page, () => document.querySelector('[data-calendar-section="almanac"]')?.textContent.includes('2026年11月02日'), 'Calendar -> almanac');
      assert.equal(await birthDate.inputValue(), '2026-11-01', 'Almanac selection must not change birth state');
      await calendar.getByText('年', { exact: true }).click();
      assert.equal(await calendar.getAttribute('data-calendar-mode'), 'year');
      await calendar.getByText('月', { exact: true }).click();
      // Scope actions to this visible calendar's Card body, not a hidden responsive twin.
      const calendarCard = calendar.locator('xpath=ancestor::*[contains(concat(" ", normalize-space(@class), " "), " ant-card-body ")][1]');
      // Antd inserts a space into two-character Chinese button names.
      await calendarCard.getByRole('button', { name: /^今\s*天$/ }).click();
      await calendarCard.getByRole('button', { name: /^签\s*到$/ }).click();
      assert.equal(await calendar.locator('[data-calendar-date="2026-11-01"]').getAttribute('data-signed'), 'true');
      report.scenarios.push(`${label}: calendar selection, month/year, today, in-memory sign and birth separation`);
      // Source styling reveals the copy control only while its almanac card is hovered.
      await page.locator('[data-calendar-section="almanac"]').hover();
      await clipboardCopy(page, () => page.getByLabel('复制黄历信息', { exact: true }).click(), /2026.*11/);
      // DatePicker popup still uses source lunar cells, now Antd 6 popup.root classNames.
      await birthDate.click();
      await page.locator('.ant-picker-dropdown:visible [title="2026-10-31"]').click();
      const timeInput = page.getByRole('textbox', { name: '出生时间', exact: true });
      await timeInput.fill('11:30');
      await timeInput.press('Enter');
      await page.getByText('出生地点', { exact: true }).click();
      // Click the actual input; the placeholder is covered by the Cascader combobox.
      await page.locator('.ant-cascader input[role="combobox"]').click();
      const locationPopup = page.locator('.ant-cascader-dropdown:visible');
      await locationPopup.getByTitle('北京市', { exact: true }).click();
      await locationPopup.getByTitle('市辖区', { exact: true }).click();
      await locationPopup.getByTitle('东城区', { exact: true }).click();
      assert.equal(Number(await page.getByRole('spinbutton', { name: '经度', exact: true }).inputValue()), 116.416334);
      assert.equal(Number(await page.getByRole('spinbutton', { name: '纬度', exact: true }).inputValue()), 39.928359);
      await page.getByRole('radio', { name: '女', exact: true }).check();
      await page.getByRole('spinbutton', { name: '经度', exact: true }).fill('87.62');
      await page.getByRole('spinbutton', { name: '纬度', exact: true }).fill('43.82');
      const ziwei = page.locator('[data-calendar-section="ziwei"]');
      await waitFor(page, () => {
        const chart = document.querySelector('[data-calendar-section="ziwei"]');
        return chart?.getAttribute('data-birth-date') === '2026-10-31'
          && chart?.getAttribute('data-birth-time') === '11:30:00' && chart?.getAttribute('data-gender') === 'female';
      }, 'birth date/time/gender -> ZiWei');
      await page.getByRole('switch', { name: '真太阳时' }).click();
      await waitFor(page, () => document.querySelector('[data-calendar-section="ziwei"]')?.getAttribute('data-birth-time') !== '11:30:00', 'longitude and true-solar toggle -> ZiWei');
      const adjustedTime = await ziwei.getAttribute('data-birth-time');
      await page.getByRole('button', { name: '开始排盘', exact: true }).click();
      // Antd icon labels prefix the accessible name (copy / file-text).
      await clipboardCopy(page, () => page.getByRole('button', { name: /复制本命数据$/ }).click(), /紫微斗数/);
      await clipboardCopy(page, () => page.getByRole('button', { name: /复制运限数据$/ }).click(), /运势信息/);
      const visiblePalaces = page.locator('[data-palace-index]:visible');
      const targetPalace = visiblePalaces.nth(4);
      await targetPalace.click();
      assert.equal(await targetPalace.getAttribute('data-palace-focused'), 'true');
      const viewBefore = await ziwei.getAttribute('data-view-date');
      await page.locator('[aria-label="day-next"]:visible').click();
      await page.waitForFunction((before) => document.querySelector('[data-calendar-section="ziwei"]')?.getAttribute('data-view-date') !== before, viewBefore);
      if (label === 'mobile') {
        await page.getByLabel('horoscope-reset', { exact: true }).click();
        assert.equal(await ziwei.getAttribute('data-view-date'), '2026-11-01 10:00');
      }
      report.scenarios.push(`${label}: birth inputs/true solar/gender, ZiWei palace focus/horoscope navigation and clipboard`);
      await page.screenshot({ path: resolve(evidenceRoot, `plugin-${label}-ziwei.png`), fullPage: true });
      await page.getByRole('tab', { name: '八字排盘' }).click();
      const bazi = page.locator('[data-calendar-section="bazi"]');
      await bazi.waitFor();
      assert.equal(await bazi.getAttribute('data-birth-date'), '2026-10-31');
      assert.equal(await bazi.getAttribute('data-birth-time'), adjustedTime);
      assert.equal(await bazi.getAttribute('data-gender'), 'female');
      await bazi.getByRole('tab').nth(1).click();
      const expand = bazi.locator('.ant-table-row-expand-icon:visible').first();
      await expand.click();
      assert.match(await bazi.innerText(), /流月运势/);
      await clipboardCopy(page, () => bazi.getByRole('button', { name: /复制/ }).click(), /八字命盘客观数据/);
      report.scenarios.push(`${label}: Bazi live linkage, decade selection, year/month expansion and clipboard`);
      assert.equal(await page.locator('iframe, a[href]').count(), 0, 'No old navigation or iframe');
      await page.screenshot({ path: resolve(evidenceRoot, `plugin-${label}.png`), fullPage: true });
      await writeFile(resolve(evidenceRoot, `plugin-${label}-text.txt`), await page.locator('body').innerText());
    } catch (error) {
      // Attempt both viewports, but retain every failure as a failing assertion below.
      report.failures.push(`${label}: ${error.stack ?? error}`);
    } finally { await context.close(); }
  }
  assert.deepEqual(report.errors, [], 'Browser errors must be investigated, not swallowed');
  assert.deepEqual(report.requests, [], 'No frontend data/network requests are permitted');
  assert.deepEqual(report.failures, [], 'Complete desktop/mobile scenario failures');
  report.status = 'passed';
} catch (error) {
  report.status = 'failed'; report.failure = error.stack; process.exitCode = 1;
} finally {
  try { await browser?.close(); }
  finally {
    await writeFile(resolve(evidenceRoot, 'component-fixtures.json'), JSON.stringify(report, null, 2) + '\n');
  }
}
console.log(JSON.stringify({ status: report.status, scenarios: report.scenarios.length, failure: report.failure?.split('\n')[0] }));
