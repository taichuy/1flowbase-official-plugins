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
  assert.match(source, /onAlmanac=\{goAlmanac\}/);
  assert.ok(publicExports.includes('CalendarProvider'));
  assert.ok(!componentExports.includes('CalendarProvider'));
});
test('portable template ships the same editable source with remappable identities', async () => {
  const p = await readPackage(resolve(repo, 'applications-demo/@taichuy/calendar-composition'));
  assert.equal(p.pages.length, 1); assert.equal(p.pages[0].tabs.length, 1);
  const b = p.pages[0].tabs[0].blocks[0];
  assert.equal(b.source_code, source);
  assert.equal(b.runtime_descriptor.contribution.code, 'Calendar');
  assert.equal(p.plugins[0].plugin_version, '0.1.6');
  assert.notEqual(p.pages[0].id, '01a1949d-0dee-7099-bd6c-b7c94e926c61');
  assert.deepEqual(await buildArchive(resolve(repo, 'applications-demo/@taichuy/calendar-composition')), await readFile(resolve(root, '_assets/templates/calendar-composition.zip')));
});
test('public declarations match actual source types and allow composing hooks and primitives', async () => {
  const req=createRequire(resolve(hostRoot,'web/package.json')); const ts=req('typescript');
  const declarations=await readFile(resolve(root,'types/index.d.ts'),'utf8');
  const fixture=`import React from 'react'; import dayjs from 'dayjs';
import { CalendarProvider, Calendar, BirthInfo, useCalendarDate, useBirthInfo, LunarInfoCard, ZiWeiCalculator, BaziCalculator, LocationSelector } from '@taichuy/taichuy-calendar';
function Custom() { const calendar=useCalendarDate(); const birth=useBirthInfo(); birth.setBirthDate(dayjs()); birth.setGender('female'); calendar.setSelectedDate(dayjs()); return <><Calendar onAlmanac={()=>{}}/><BirthInfo/><LunarInfoCard date={calendar.selectedDate}/><ZiWeiCalculator date={birth.birthDate} time={birth.time} gender={birth.gender} locationInfo={birth.locationInfo}/><BaziCalculator date={birth.birthDate} time={birth.time} gender={birth.gender}/><LocationSelector onChange={(codes,lng,lat)=>{ codes.join(); lng?.toFixed(); lat?.toFixed(); }}/></>; }
export default function Block() { return <CalendarProvider><Custom/></CalendarProvider>; }`;
  const filename=resolve(hostRoot,'web/app/calendar-composition-type.fixture.tsx');
  const declname=resolve(hostRoot,'web/app/node_modules/@taichuy/taichuy-calendar/index.d.ts');
  const host=ts.createCompilerHost({}); const read=host.readFile.bind(host),exists=host.fileExists.bind(host);
  host.readFile=p=>p===filename?fixture:p===declname?declarations:read(p);
  host.fileExists=p=>p===filename||p===declname||exists(p);
  const options={noEmit:true,strict:true,skipLibCheck:false,esModuleInterop:true,jsx:ts.JsxEmit.React,module:ts.ModuleKind.ESNext,moduleResolution:ts.ModuleResolutionKind.Bundler,target:ts.ScriptTarget.ES2022,baseUrl:resolve(hostRoot,'web/app'),paths:{'@taichuy/taichuy-calendar':[declname]}};
  const program=ts.createProgram([filename,declname],options,host);
  // Upstream host declarations may have unrelated diagnostics; include our fixture and public API only.
  const diagnostics=ts.getPreEmitDiagnostics(program).filter(d=>d.file&&(d.file.fileName===filename||d.file.fileName===declname));
  assert.deepEqual(diagnostics.map(d=>ts.flattenDiagnosticMessageText(d.messageText,'\n')),[]);
});
