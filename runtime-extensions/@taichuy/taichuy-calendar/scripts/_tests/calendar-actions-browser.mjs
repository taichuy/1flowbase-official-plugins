import assert from 'node:assert/strict';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { hostRoot, loadHostTool } from '../toolchain.mjs';

// Real factory + actual public compositions, without a server or network access.
const out = process.env.TAICHUY_EVIDENCE_ROOT ?? resolve(hostRoot, 'tmp/test-governance/calendar-today-only');
const script = await readFile(resolve(out, 'compiled/factory-browser.js'), 'utf8');
const css = await readFile(resolve(out, 'compiled/factory-browser.css'), 'utf8');
const { chromium } = loadHostTool('playwright');
const browser = await chromium.launch({ headless: true, executablePath: process.env.TAICHUY_CHROME_EXECUTABLE ?? '/usr/bin/google-chrome' });
const report = { checks: [], errors: [] };
await mkdir(out, { recursive: true });
try {
  for (const viewport of [{ width: 1440, height: 1100 }, { width: 390, height: 844 }]) {
    const context = await browser.newContext({ viewport, timezoneId: 'Asia/Shanghai', locale: 'zh-CN' });
    try {
      await context.route('**/*', route => route.abort());
      const page = await context.newPage();
      page.on('pageerror', error => report.errors.push(error.message));
      await page.clock.setFixedTime(new Date('2026-11-04T10:00:00+08:00'));
      await page.setContent('<html><head></head><body><div id="fixture-root"></div></body></html>');
      await page.addStyleTag({ content: css });
      await page.addScriptTag({ content: script });
      for (const name of ['Calendar', 'ComposedPage', 'CalendarPage']) {
        const props = { initialDate: '2025-08-15', initialBirthDate: '1990-01-01', initialTime: '10:30', initialUseTrueSolarTime: false };
        await page.evaluate(({ name, props }) => window.calendarFixture.mount(name, props), { name, props });
        await page.waitForFunction(key => document.querySelector('[data-fixture-key]')?.getAttribute('data-fixture-key') === key, `${name}-${JSON.stringify(props)}`);
        const calendar = page.locator('[data-calendar-section="calendar"]:visible');
        await calendar.locator('[data-calendar-date="2025-08-15"]').waitFor();
        const card = calendar.locator('..');
        assert.equal(await card.getByRole('button').count(), 1, 'Only Today remains in the action bar');
        assert.equal(await card.getByRole('button', { name: /^签\s*到$/ }).count(), 0);
        assert.equal(await card.getByRole('button', { name: /^黄\s*历$/ }).count(), 0);
        await calendar.getByText('年', { exact: true }).click();
        assert.equal(await calendar.getAttribute('data-calendar-mode'), 'year');
        await card.getByRole('button', { name: /^今\s*天$/ }).click();
        await page.waitForFunction(() => document.querySelector('[data-calendar-section="calendar"][data-calendar-mode="month"] [data-calendar-date="2026-11-04"]'));
        if (name !== 'Calendar') {
          await page.waitForFunction(() => document.querySelector('[data-calendar-section="almanac"]')?.textContent.includes('2026年11月04日'));
          assert.equal(await page.getByRole('textbox', { name: '出生日期', exact: true }).inputValue(), '1990-01-01');
          await calendar.locator('[data-calendar-date="2026-11-05"]').click();
          await page.waitForFunction(() => document.querySelector('[data-calendar-section="almanac"]')?.textContent.includes('2026年11月05日'));
          await card.getByRole('button', { name: /^今\s*天$/ }).click();
          await page.waitForFunction(() => document.querySelector('[data-calendar-section="almanac"]')?.textContent.includes('2026年11月04日'));
          assert.equal(await page.getByRole('textbox', { name: '出生日期', exact: true }).inputValue(), '1990-01-01');
        }
        report.checks.push({ width: viewport.width, component: name, onlyToday: true, resetsYearAndMonth: true, almanacAndBirthSeparation: name !== 'Calendar' });
        if (name === 'ComposedPage') await card.screenshot({ path: resolve(out, `calendar-${viewport.width}.png`) });
      }
    } finally { await context.close(); }
  }
  assert.deepEqual(report.errors, []);
  report.status = 'PASS';
  console.log(JSON.stringify(report));
} finally {
  await browser.close();
  await writeFile(resolve(out, 'calendar-actions-browser.json'), JSON.stringify(report, null, 2));
}
