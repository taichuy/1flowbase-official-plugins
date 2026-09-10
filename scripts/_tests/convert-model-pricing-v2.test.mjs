import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { convertPricingSource, migratePricingRepository } from '../convert-model-pricing-v2.mjs';
import { updateModelPricingCatalog } from '../model-pricing-catalog.mjs';
const legacy = JSON.parse(fs.readFileSync(new URL('./fixtures/model-pricing/legacy-catalog.json',import.meta.url)));
const meters = ['input','output','cache_hit','cache_write'];
const groups = new Map();
for (const row of legacy.rules) {
  const {provider_code,upstream_model_id,currency_code,source_kind,source_catalog_id,source_checksum,source_version,...rule} = row;
  const key = `${provider_code}/${upstream_model_id}`;
  if (!groups.has(key)) groups.set(key,{schema_version:'1flowbase.model-pricing-source/v1',provider_code,upstream_model_id,currency_code,rules:[]});
  groups.get(key).rules.push(rule);
}
const threshold = (t,n) => !t || (t.operator === 'gt' ? n > t.value : n >= t.value);
const matches = (w,time,input,ttl) => {
  const local = time.slice(11,19);
  return (!w.effective_from || time >= w.effective_from) && (!w.effective_to || time < w.effective_to) &&
    (!w.local_time_start || (w.local_time_start < w.local_time_end ? local >= w.local_time_start && local < w.local_time_end : local >= w.local_time_start || local < w.local_time_end)) &&
    threshold(w.input_tokens,input) && (!w.cache_write_ttl_seconds || ttl === w.cache_write_ttl_seconds);
};
function oldRates(source,time,input,ttl) {
  const r = source.rules.filter(r => matches(r,time,input,ttl)).sort((a,b)=>b.priority-a.priority || b.effective_from.localeCompare(a.effective_from))[0];
  if (!r) return null;
  let rates = Object.fromEntries(meters.map(m => [m,[r[`${m === 'cache_write' ? 'input' : m}_token_unit_size`],r[`${m === 'cache_write' ? 'input' : m}_token_unit_price`]]]));
  if (r.rating_policy_enabled) {
    const p = r.rating_policy;
    if (p.type === 'input_token_tiers') {
      for (const tier of p.tiers) if(threshold(tier.when,input)) rates = Object.fromEntries(meters.map(m=>{ const v=tier.rates[m==='cache_write'?'input':m];return [m,[v.unit_size,v.unit_price]]; }));
    } else {
      let selected = p.rates;
      for(const tier of p.input_token_tiers??[]) if(threshold(tier.when,input)) selected=tier.rates;
      rates=Object.fromEntries(meters.map(m=>[m,[p.unit_size,m==='cache_write' ? selected[m].unit_price??selected[m].by_ttl_seconds[ttl] : selected[m]]]));
    }
  }
  return rates;
}
function newRates(source,time,input,ttl) {
  if (!matches(source,time,input,ttl)) return null;
  const result = {...source};
  for (const r of source.rules) if(matches(r.when,time,input,ttl)) Object.assign(result,r.overrides);
  return Object.fromEntries(meters.map(m=>[m,[result[`${m}_token_unit_size`],result[`${m}_token_unit_price`]]]));
}
test('real legacy sources preserve four prices across tier and window boundaries',()=>{
  for(const source of groups.values()) {
    const next=convertPricingSource(source);
    assert.deepEqual(convertPricingSource(next),next);
    for(const date of ['2026-09-10','2026-12-31','2027-01-01'])
      for(const time of ['00:00:00','00:59:59','01:00:00','03:59:59','04:00:00','05:59:59','06:00:00','09:59:59','10:00:00','23:59:59'])
        for(const input of [0,128000,128001,200000,200001,256000,256001,272000,272001,1000000])
          for(const ttl of [300,3600]) {
            const instant=`${date}T${time}Z`;
            assert.deepEqual(newRates(next,instant,input,ttl),oldRates(source,instant,input,ttl),`${source.upstream_model_id} ${instant} ${input} ${ttl}`);
          }
  }
});
test('conversion rejects unknown structures and noncontiguous windows',()=>{
  const source=structuredClone(groups.get('deepseek/deepseek-v4-pro'));
  source.rules[0].local_time_end='00:59:00';
  assert.throws(()=>convertPricingSource(source),/unsupported/);
  const single=structuredClone(groups.get('zero/any'));
  single.rules[0].unknown=1;
  assert.throws(()=>convertPricingSource(single),/unsupported/);
});
test('repository migration is idempotent and never bypasses v2 immutability',()=>{
  const repo=fs.mkdtempSync(path.join(os.tmpdir(),'pricing-converter-'));
  const root=path.join(repo,'model-pricing');
  fs.mkdirSync(root,{recursive:true});
  for(const source of groups.values()) {
    const dir=path.join(root,`@${source.provider_code}`,source.upstream_model_id.replaceAll('/','_'));
    fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(path.join(dir,'pricing.json'),JSON.stringify(source));
  }
  fs.writeFileSync(path.join(root,'catalog-source.json'),JSON.stringify({schema_version:'1flowbase.model-pricing-source/v1',catalog_version:'legacy',currency_code:'USD'}));
  fs.mkdirSync(path.join(root,'catalog/v1'),{recursive:true});
  fs.mkdirSync(path.join(root,'_maintenance'),{recursive:true});
  fs.writeFileSync(path.join(root,'catalog/v1/catalog.json'),JSON.stringify(legacy));
  const state = JSON.parse(fs.readFileSync(new URL('./fixtures/model-pricing/legacy-state.json',import.meta.url)));
  const statePath = path.join(root,'_maintenance/catalog-state.json');
  const corrupted = structuredClone(state);corrupted.rules[legacy.rules[0].id].source_checksum = 'sha256:bad';
  fs.writeFileSync(statePath,JSON.stringify(corrupted));
  assert.throws(()=>migratePricingRepository(repo),/unsupported/);
  // Failed preflight leaves source bytes unmigrated.
  assert.equal(JSON.parse(fs.readFileSync(path.join(root,'@zero/any/pricing.json'))).schema_version,'1flowbase.model-pricing-source/v1');
  fs.writeFileSync(statePath,JSON.stringify(state));
  assert.ok(migratePricingRepository(repo)>0);
  updateModelPricingCatalog({repoRoot:repo});
  assert.equal(migratePricingRepository(repo),0);
  const file=path.join(root,'@zero/any/pricing.json');
  const zero=JSON.parse(fs.readFileSync(file));zero.input_token_unit_price='1';fs.writeFileSync(file,JSON.stringify(zero));
  assert.equal(migratePricingRepository(repo),0);
  assert.throws(()=>updateModelPricingCatalog({repoRoot:repo}),/immutable/);
  fs.rmSync(repo,{recursive:true,force:true});
});

test('canonical temporal admission rejects QA counterexamples and keeps legal neighbours', () => {
  const base = convertPricingSource(groups.get('zero/any'));
  const condition = when => ({rules:[{when,overrides:{input_token_unit_price:'1'}}]});
  const window = (start, end) => ({timezone:'UTC',local_time_start:start,local_time_end:end});
  const pairs = [
    ['timezone alone', condition({timezone:'UTC'}), condition({timezone:'UTC',input_tokens:{operator:'gte',value:0}})],
    ['weekday timezone', condition({weekday_mask:1}), condition({weekday_mask:1,timezone:'UTC'})],
    ['equal conditional endpoints', condition(window('01:00:00','01:00:00')), condition(window('01:00:00','01:00:01'))],
    ['conditional calendar date', condition({effective_from:'2026-02-30T00:00:00Z'}), condition({effective_from:'2026-02-28T00:00:00Z'})],
    ['equal default endpoints', window('01:00:00','01:00:00'), window('01:00:00','01:00:01')],
    ['default cross-midnight weekday', {...window('23:00:00','01:00:00'),weekday_mask:1}, {...window('23:00:00','01:00:00'),weekday_mask:127}],
    ['default calendar date', {effective_from:'2026-02-30T00:00:00Z'}, {effective_from:'2026-02-28T00:00:00Z'}],
    ['priority i32', {priority:2147483648}, {priority:2147483647}],
  ];
  for (const [name, invalid, valid] of pairs) {
    assert.throws(() => convertPricingSource({...base,...invalid}), /invalid v2/, name);
    assert.doesNotThrow(() => convertPricingSource({...base,...valid}), name);
  }
  // Conditional windows may cross midnight even with a restricted weekday mask.
  assert.doesNotThrow(() => convertPricingSource({...base,...condition({...window('23:59:59','00:00:00'),weekday_mask:1})}));
  for (const [invalid, valid] of [
    ['2026-02-29T00:00:00Z','2028-02-29T00:00:00Z'],
    ['2100-02-29T00:00:00Z','2000-02-29T00:00:00Z'],
    ['2026-04-31T00:00:00Z','2026-04-30T00:00:00Z'],
    ['2026-00-01T00:00:00Z','2026-01-01T00:00:00Z'],
    ['2026-13-01T00:00:00Z','2026-12-01T00:00:00Z'],
    ['2026-01-00T00:00:00Z','2026-01-01T00:00:00Z'],
    ['2026-01-01T24:00:00Z','2026-01-01T23:59:59Z'],
    ['2026-01-01T00:60:00Z','2026-01-01T00:59:59Z'],
    ['2026-01-01T00:00:61Z','2026-01-01T00:00:59Z'],
    ['2026-01-01T00:00:00+24:00','2026-01-01T00:00:00+23:59'],
    ['2026-01-01T00:00:00-01:60','2026-01-01T00:00:00-01:59'],
    ['2026-01-01T00:00:60Z','2026-01-31T23:59:60Z'],
    ['2026-01-01T00:00:00.Z','2026-01-01t00:00:00.000000001z'],
  ]) {
    for (const patch of [value => ({effective_from:value}), value => condition({effective_from:value})]) {
      assert.throws(() => convertPricingSource({...base,...patch(invalid)}), /invalid v2/, invalid);
      assert.doesNotThrow(() => convertPricingSource({...base,...patch(valid)}), valid);
    }
  }
  for (const weekday_mask of [0,128,1.5,Number.MAX_SAFE_INTEGER + 1]) {
    assert.throws(() => convertPricingSource({...base,weekday_mask}), /invalid v2/);
    assert.throws(() => convertPricingSource({...base,...condition({timezone:'UTC',weekday_mask})}), /invalid v2/);
  }
  for (const weekday_mask of [1,127]) assert.doesNotThrow(() => convertPricingSource({...base,weekday_mask,...condition({timezone:'UTC',weekday_mask})}));
  // Ordering uses offset-aware nanoseconds, not Date's truncated millisecond representation.
  const from = '2026-01-01T00:00:00.000000001Z';
  const to = '2026-01-01T01:00:00.000000002+01:00';
  for (const patch of [v=>v, condition]) {
    assert.doesNotThrow(() => convertPricingSource({...base,...patch({effective_from:from,effective_to:to})}));
    assert.throws(() => convertPricingSource({...base,...patch({effective_from:to,effective_to:from})}), /invalid v2/);
    assert.throws(() => convertPricingSource({...base,...patch({effective_from:from,effective_to:from})}), /invalid v2/);
  }
});
