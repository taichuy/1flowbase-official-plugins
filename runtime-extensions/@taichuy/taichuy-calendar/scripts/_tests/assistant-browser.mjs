import assert from 'node:assert/strict';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { hostRoot, loadHostTool } from '../toolchain.mjs';

// Real generated factory; no server, auth, downloaded dependencies, or network.
const out = process.env.TAICHUY_EVIDENCE_ROOT ?? resolve(hostRoot, 'tmp/test-governance/calendar-editable-helper');
const script = await readFile(resolve(out, 'compiled/factory-browser.js'), 'utf8');
const css = await readFile(resolve(out, 'compiled/factory-browser.css'), 'utf8');
const { chromium } = loadHostTool('playwright');
const browser = await chromium.launch({ headless: true, executablePath: '/usr/bin/google-chrome' });
const report = { checks: [], errors: [] };
await mkdir(out, { recursive: true });
try {
  for (const viewport of [{ width: 1440, height: 1100 }, { width: 390, height: 844 }].filter(v => !process.env.ASSISTANT_VIEWPORT || v.width === Number(process.env.ASSISTANT_VIEWPORT))) {
    const context = await browser.newContext({ viewport, timezoneId: 'Asia/Shanghai', locale: 'zh-CN' });
    try {
      await context.route('**/*', route => route.abort());
      const page = await context.newPage();
      page.on('pageerror', error => report.errors.push(error.message));
      await page.clock.setFixedTime(new Date('2026-11-04T10:00:00+08:00'));
      await page.setContent('<html><head></head><body><div id="fixture-root"></div></body></html>');
      await page.addStyleTag({ content: css });
      await page.addScriptTag({ content: script });
      const props = { initialBirthDate: '1990-01-01', initialTime: '10:30', initialUseTrueSolarTime: false };
      const mount = async (name, options = props) => {
        await page.evaluate(({ name, options }) => window.calendarFixture.mount(name, options), { name, options });
        await page.waitForFunction(key => document.querySelector('[data-fixture-key]')?.getAttribute('data-fixture-key') === key, `${name}-${JSON.stringify(options)}`);
      };
      for (const kind of ['ziwei', 'bazi']) {
        await mount('AssistantSlot', { ...props, kind });
        const panel = page.locator(`[data-custom-assistant="${kind}"]`);
        await panel.waitFor();
        assert.equal(await page.getByText(/AI.*解读助手/).count(), 0, 'Custom renderer replaces, rather than appends to, the original panel');
        const natal = panel.getByLabel('custom-natal');
        const before = await natal.inputValue();
        assert.ok(before.length > 1000, 'Full chart description reaches the page');
        await page.clock.setFixedTime(new Date('2026-11-04T10:00:02+08:00'));
        await panel.getByRole('button', { name: 'custom-copy-natal', exact: true }).click();
        await page.waitForFunction(text => window.calendarFixture.clipboard.at(-1) === text, before);
        await page.getByRole('button', { name: 'Birth +1', exact: true }).click();
        await page.waitForFunction(before => document.querySelector('[aria-label="custom-natal"]').value !== before, before);
        const after = await natal.inputValue();
        await panel.getByRole('button', { name: 'custom-copy-natal', exact: true }).click();
        await page.waitForFunction(text => window.calendarFixture.clipboard.at(-1) === text, after);
        if (kind === 'ziwei') {
          const horoscope = panel.getByLabel('custom-horoscope');
          const original = await horoscope.inputValue();
          const originalDate = await panel.locator('[data-view-date]').textContent();
          await page.locator('[aria-label="year-next"]:visible').click();
          await page.waitForFunction(before => document.querySelector('[aria-label="custom-horoscope"]').value !== before, original);
          assert.notEqual(await panel.locator('[data-view-date]').textContent(), originalDate);
          assert.equal(await natal.inputValue(), after, 'Horoscope navigation does not change the natal chart');
          const current = await horoscope.inputValue();
          await panel.getByRole('button', { name: 'custom-copy-horoscope', exact: true }).click();
          await page.waitForFunction(text => window.calendarFixture.clipboard.at(-1) === text, current);
        } else {
          const originalIndex = Number(await panel.locator('[data-decade-index]').textContent());
          const tabs = page.locator('[data-calendar-section="bazi"]').getByRole('tab');
          const target = tabs.nth((originalIndex + 1) % 8);
          // Keyboard activation avoids rc-tabs' horizontal scroll animation on narrow screens.
          await target.focus();
          await target.press('Enter');
          await page.waitForFunction(index => Number(document.querySelector('[data-decade-index]').textContent) !== index, originalIndex);
        }
        await mount('AssistantSlot', { ...props, kind, hidden: true });
        await page.locator(`[data-calendar-section="${kind}"]`).waitFor();
        assert.equal(await page.locator('[data-custom-assistant]').count(), 0);
        assert.equal(await page.getByText(/AI.*解读助手/).count(), 0, 'Null renderer omits only the assistant');
        await mount(kind === 'ziwei' ? 'ZiWei' : 'Bazi');
        await page.getByText(/AI.*解读助手/).waitFor();
        assert.ok((await page.locator('textarea:not([aria-hidden="true"])').inputValue()).length > 1000, 'Default assistant remains available');
        report.checks.push({ width: viewport.width, kind, custom: true, birthUpdates: true, selectionUpdates: true, copyMatchesData: true, nullAndDefault: true });
      }
      await mount('ComposedPage');
      await page.locator('[data-calendar-ai-helper="ziwei"]').waitFor();
      assert.equal(await page.getByText('你我悉数应知，此世必当载于青史，', { exact: true }).count(), 1);
      await page.getByRole('tab', { name: '八字排盘' }).click();
      await page.locator('[data-calendar-ai-helper="bazi"]').waitFor();
      report.checks.push({ width: viewport.width, templateOwnsBothAssistants: true });
      await page.screenshot({ path: resolve(out, `assistant-${viewport.width}.png`), fullPage: true });
    } finally { await context.close(); }
  }
  assert.deepEqual(report.errors, []);
  report.status = 'PASS';
  console.log(JSON.stringify(report));
} finally {
  await browser.close();
  await writeFile(resolve(out, 'assistant-browser.json'), JSON.stringify(report, null, 2));
}
