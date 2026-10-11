import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { planCollectorRelease } from '../release-plan.mjs';
const repository='owner/plugins';
function fixture(t){
 const repoRoot=fs.mkdtempSync(path.join(os.tmpdir(),'collector-plan-'));t.after(()=>fs.rmSync(repoRoot,{recursive:true,force:true}));
 const git=(...args)=>execFileSync('git',args,{cwd:repoRoot,encoding:'utf8'}).trim();
 git('init','-q');git('config','user.email','fixture@example.invalid');git('config','user.name','Fixture');
 const collector={collector_code:'codex-logs-collector',version:'0.4.0',plugin_dir:'runtime-extensions/@owner/codex-logs-collector',release_tag:'codex-logs-collector-v0.4.0'};
 const write=(name,value)=>{const p=path.join(repoRoot,name);fs.mkdirSync(path.dirname(p),{recursive:true});fs.writeFileSync(p,value);};
 const commit=()=>{git('add','.');git('commit','-qm','fixture');return git('rev-parse','HEAD');};
 write(collector.plugin_dir+'/src/lib.rs','pub fn original() {}');write('sdk/agent-logs-collector/src/lib.rs','pub fn shared() {}');
 const published=commit();const asset='codex-logs-collector-0.4.0-distribution.tar.gz';
 const entry={version:collector.version,checksum:'sha256:fixture',signature:{algorithm:'ed25519',key_id:'fixture',signature:'fixture'},download_locator:{kind:'release_asset',locator:`https://github.com/${repository}/releases/download/${collector.release_tag}/${asset}`}};
 write(collector.plugin_dir+'/catalog-entry.json',JSON.stringify(entry));
 let sourceSha=commit();
 const lookup=()=>({targetCommitish:published,assets:[{name:asset}]});
 const plan=(extra={})=>planCollectorRelease({repoRoot,collector,repository,sourceSha,lookup,...extra});
 return {collector,write,commit,plan,lookup,published,setHead:()=>sourceSha=commit()};
}
test('published unchanged collector reuses its signed catalog distribution across newer commits',t=>{
 const f=fixture(t);f.write('runtime-extensions/@owner/other/src/lib.rs','changed');f.setHead();
 const p=f.plan();assert.equal(p.action,'reuse');assert.equal(p.sourceSha,f.published);assert.equal(p.entry.version,'0.4.0');
});
test('new version without a release enters native build matrix',t=>{const f=fixture(t);assert.equal(f.plan({lookup:()=>undefined}).action,'build');});
for(const changed of ['runtime-extensions/@owner/codex-logs-collector/src/lib.rs','sdk/agent-logs-collector/src/lib.rs','installers/client-collectors/install.sh','scripts/collectors/package.mjs','scripts/collectors/distribution.mjs']){
 test('published version refuses changed payload input '+changed,t=>{const f=fixture(t);f.write(changed,'changed');f.setHead();assert.throws(()=>f.plan(),/bump.*version/i);});
}
test('missing published catalog retains rebuild-and-verify retry path',t=>{const f=fixture(t);f.write(f.collector.plugin_dir+'/catalog-entry.json',JSON.stringify({version:'0.3.0'}));f.setHead();const p=f.plan();assert.equal(p.action,'build');assert.equal(p.sourceSha,f.published);assert.ok(p.existing);});
test('existing asset identity and published commit are checked before reuse',t=>{const f=fixture(t);assert.throws(()=>f.plan({lookup:()=>({...f.lookup(),assets:[{name:'wrong'}]})}),/asset/i);assert.throws(()=>f.plan({lookup:()=>({...f.lookup(),targetCommitish:'main'})}),/commit/i);});
test('matching-version catalog cannot redirect reuse to another artifact',t=>{const f=fixture(t);f.write(f.collector.plugin_dir+'/catalog-entry.json',JSON.stringify({version:'0.4.0',download_locator:{kind:'release_asset',locator:'https://elsewhere.invalid/asset'}}));f.setHead();assert.throws(()=>f.plan(),/catalog/i);});
