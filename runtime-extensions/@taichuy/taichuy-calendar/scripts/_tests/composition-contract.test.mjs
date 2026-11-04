import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { test } from 'node:test';
import { createRequire } from 'node:module';
import { root, hostRoot } from '../toolchain.mjs';
import { componentExports, publicExports } from '../module-contract.mjs';
const repo = resolve(root, '../../..');
const { readPackage, buildArchive } = await import(resolve(repo, 'scripts/application-template/archive.mjs'));
const source = await readFile(resolve(root, 'examples/composed-page.tsx'), 'utf8');
test('editable page source owns layout and uses public components instead of full-page wrapper', () => {
  assert.doesNotMatch(source, /\bCalendarPage\b/);
  for (const name of ['CalendarProvider', 'Calendar', 'Almanac', 'BirthInfo', 'ZiWei', 'Bazi']) assert.match(source, new RegExp(`<${name}(?:[ />])`));
  assert.match(source, /<Tabs/);
  assert.doesNotMatch(source, /onAlmanac|goAlmanac/);
  assert.ok(publicExports.includes('CalendarProvider'));
  assert.ok(!componentExports.includes('CalendarProvider'));
});
test('portable template ships the same editable source with remappable identities', async () => {
  const p = await readPackage(resolve(repo, 'applications-demo/@taichuy/calendar-composition'));
  assert.equal(p.pages.length, 1); assert.equal(p.pages[0].tabs.length, 1);
  assert.equal(p.pages[0].parent_id, null);
  assert.equal(p.pages[0].placement, 'topbar', 'Standalone templates need a routable root');
  assert.equal(p.pages[0].slug, 'calendar-composition');
  const b = p.pages[0].tabs[0].blocks[0];
  assert.equal(b.source_code, source);
  assert.equal(b.runtime_descriptor.contribution.code, 'CalendarComposition');
  const { version } = JSON.parse(await readFile(resolve(root, 'package.json'), 'utf8'));
  assert.equal(p.plugins[0].plugin_version, version);
  assert.equal(b.runtime_descriptor.contribution.pluginVersion, version);
  assert.equal(b.runtime_descriptor.runtime.code_template_version, version);
  assert.notEqual(p.pages[0].id, '01a1949d-0dee-7099-bd6c-b7c94e926c61');
  assert.deepEqual(await buildArchive(resolve(repo, 'applications-demo/@taichuy/calendar-composition')), await readFile(resolve(root, '_assets/templates/calendar-composition.zip')));
  // Reusing a published (template_id, release_version) with changed bytes must fail preflight.
  // The planner accepts either an exact already-published release or a new immutable version.
  const { buildReleasePlan } = await import(resolve(repo, 'scripts/application-template/publisher.mjs'));
  await buildReleasePlan({ repoRoot: repo });
});
test('public declarations match actual source types and allow composing hooks and primitives', async () => {
  const req=createRequire(resolve(hostRoot,'web/package.json')); const ts=req('typescript');
  const declarations=await readFile(resolve(root,'types/index.d.ts'),'utf8');
  const fixture=`import React from 'react'; import dayjs from 'dayjs';
import { CalendarProvider, Calendar, BirthInfo, useCalendarDate, useBirthInfo, LunarInfoCard, ZiWeiCalculator, BaziCalculator, LocationSelector, ZiWei, Bazi, type ChartAIHelperData } from '@taichuy/taichuy-calendar';
function Assistant({ data }: { data: ChartAIHelperData }) { if (data.kind === 'ziwei') return <button onClick={data.copyHoroscope}>{data.horoscopeText}{data.viewDate}</button>; return <button onClick={data.copyNatal}>{data.natalText}{data.selectedDaYunIndex}</button>; }
function Custom() { const calendar=useCalendarDate(); const birth=useBirthInfo(); birth.setBirthDate(dayjs()); birth.setGender('female'); calendar.setSelectedDate(dayjs()); return <><Calendar/><BirthInfo/><ZiWei renderAIHelper={data=><Assistant data={data}/>}/><Bazi renderAIHelper={()=>null}/><LunarInfoCard date={calendar.selectedDate}/><ZiWeiCalculator date={birth.birthDate} time={birth.time} gender={birth.gender} locationInfo={birth.locationInfo}/><BaziCalculator date={birth.birthDate} time={birth.time} gender={birth.gender}/><LocationSelector onChange={(codes,lng,lat)=>{ codes.join(); lng?.toFixed(); lat?.toFixed(); }}/></>; }
export default function Block() { return <CalendarProvider><Custom/></CalendarProvider>; }`;
  const filename=resolve(hostRoot,'web/app/calendar-composition-type.fixture.tsx');
  const exampleName=resolve(hostRoot,'web/app/calendar-assistant-page.fixture.tsx');
  const declname=resolve(hostRoot,'web/app/node_modules/@taichuy/taichuy-calendar/index.d.ts');
  const host=ts.createCompilerHost({}); const read=host.readFile.bind(host),exists=host.fileExists.bind(host);
  host.readFile=p=>p===filename?fixture:p===exampleName?source:p===declname?declarations:read(p);
  host.fileExists=p=>p===filename||p===exampleName||p===declname||exists(p);
  const options={noEmit:true,strict:true,skipLibCheck:false,esModuleInterop:true,jsx:ts.JsxEmit.React,module:ts.ModuleKind.ESNext,moduleResolution:ts.ModuleResolutionKind.Bundler,target:ts.ScriptTarget.ES2022,baseUrl:resolve(hostRoot,'web/app'),paths:{'@taichuy/taichuy-calendar':[declname]}};
  const program=ts.createProgram([filename,exampleName,declname],options,host);
  // Upstream host declarations may have unrelated diagnostics; include our fixture and public API only.
  const diagnostics=ts.getPreEmitDiagnostics(program).filter(d=>d.file&&(d.file.fileName===filename||d.file.fileName===exampleName||d.file.fileName===declname));
  assert.deepEqual(diagnostics.map(d=>ts.flattenDiagnosticMessageText(d.messageText,'\n')),[]);
});

test('plugin installs one complete code template with a stable identity alongside the six components', async () => {
  const { manifestText } = await import('../manifest.mjs');
  const bundle = JSON.parse(await readFile(resolve(root, 'migration/bundle-contract.json'), 'utf8'));
  const manifest = await manifestText(bundle.asset.sha256, bundle.businessModules);
  assert.equal((manifest.match(/^    ui_registration: component$/gm) ?? []).length, 6);
  assert.equal((manifest.match(/^    ui_registration: code_template$/gm) ?? []).length, 1);
  const block = manifest.split('  - contribution_code: CalendarComposition\n')[1];
  assert.ok(block, 'The composition has its own stable template contribution');
  const code = block.match(/^    code_template: (.+)$/m)?.[1];
  assert.equal(JSON.parse(code), source, 'Registered source must be the whole current editable block');
  assert.match(source, /你我悉数应知，此世必当载于青史，/);
  assert.match(source, /吾等将远迈先人的大功业/);
  assert.equal((manifest.match(/contribution_id: taichuy-calendar.CalendarComposition/g) ?? []).length, 2,
    'The template has both a managed declaration and declarative execution binding');
  assert.ok(!componentExports.includes('CalendarComposition'));
  assert.ok(!publicExports.includes('CalendarComposition'), 'An editable template is not a bundled component export');
});
