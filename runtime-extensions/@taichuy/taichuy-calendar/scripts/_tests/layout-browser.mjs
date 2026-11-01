import assert from 'node:assert/strict';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { hostRoot, loadHostTool } from '../toolchain.mjs';

// Run after fixtures:prepare. Uses the real factory and editable template source,
// with a constrained scroll owner like the host; no auth, server or network.
const out = process.env.TAICHUY_EVIDENCE_ROOT ?? resolve(hostRoot, 'tmp/test-governance/taichuy-calendar');
const script = await readFile(resolve(out, 'compiled/factory-browser.js'), 'utf8');
const css = await readFile(resolve(out, 'compiled/factory-browser.css'), 'utf8');
const { chromium } = loadHostTool('playwright');
const browser = await chromium.launch({ headless: true, executablePath: process.env.TAICHUY_CHROME_EXECUTABLE ?? '/usr/bin/google-chrome' });
const report = { checks: [], errors: [] };
await mkdir(out, { recursive: true });
try {
  for (const name of ['ComposedPage', 'CalendarPage']) {
    for (const viewport of [{ width: 1440, height: 1000 }, { width: 1440, height: 600 }, { width: 390, height: 844 }]) {
      const context = await browser.newContext({ viewport, timezoneId: 'Asia/Shanghai', locale: 'zh-CN' });
      try {
        await context.route('**/*', route => route.abort());
        const page = await context.newPage();
        page.on('pageerror', error => report.errors.push(error.message));
        page.on('console', msg => { if (msg.type() === 'error') report.errors.push(msg.text()); });
        await page.setContent('<html><head></head><body style="margin:0"><div style="height:96px"></div><div id="scroll-owner" style="height:calc(100dvh - 96px);overflow:auto"><div id="fixture-root"></div></div></body></html>');
        await page.addStyleTag({ content: css });
        await page.addScriptTag({ content: script });
        await page.evaluate(name => window.calendarFixture.mount(name, { initialDate: '2026-11-01', initialBirthDate: '1990-01-01' }), name);
        const section = page.locator(`[data-calendar-component="${name}"]`);
        await section.waitFor();
        const rail = section.locator('aside');
        const owner = page.locator('#scroll-owner');
        if (viewport.width <= 768) {
          assert.equal(await rail.isVisible(), false);
          assert.equal(await section.locator('[data-calendar-date="2026-11-01"]:visible').count(), 1);
        } else {
          assert.equal(await rail.evaluate(n => getComputedStyle(n).position), 'sticky');
          await owner.evaluate(n => n.scrollTop = 250);
          const first = await rail.boundingBox();
          const leftFirst = await section.locator('main').boundingBox();
          await owner.evaluate(n => n.scrollTop = 650);
          const second = await rail.boundingBox();
          const leftSecond = await section.locator('main').boundingBox();
          assert.ok(Math.abs(first.y - second.y) < 2, 'Calendar must not move with the left panels');
          assert.ok(leftSecond.y < leftFirst.y - 200, 'Left content must actually scroll');
          assert.ok(second.y >= 96 && second.y + second.height <= viewport.height);
          if (viewport.height === 600) {
            assert.ok(await rail.evaluate(n => n.scrollHeight > n.clientHeight));
            const before = await owner.evaluate(n => n.scrollTop);
            await rail.hover();
            await page.mouse.wheel(0, 1000);
            await page.waitForFunction(() => document.querySelector('aside').scrollTop > 0);
            assert.equal(await owner.evaluate(n => n.scrollTop), before, 'Calendar wheel must not scroll left content');
          }
        }
        report.checks.push({ name, ...viewport, status: 'PASS' });
      } finally { await context.close(); }
    }
  }
  assert.deepEqual(report.errors, []);
  console.log(JSON.stringify(report));
} finally {
  await browser.close();
  await writeFile(resolve(out, 'layout-browser.json'), JSON.stringify(report, null, 2));
}
