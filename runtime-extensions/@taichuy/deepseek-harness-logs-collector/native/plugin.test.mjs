import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { apply } from './plugin.mjs';
test('callbacks finish journal subprocess before returning; terminal and flush drain', { skip: process.platform === 'win32' }, () => {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-collector-native-'));
  try {
    const binary = join(dir, 'collector');
    const output = join(dir, 'invocations.jsonl');
    writeFileSync(binary, '#!/usr/bin/env node\n' +
      "const fs=require('node:fs');fs.appendFileSync(" + JSON.stringify(output) +
      ",JSON.stringify({args:process.argv.slice(2),input:fs.readFileSync(0,'utf8')})+'\\n');\n", {mode:0o700});
    const callbacks = new Map();
    apply({on:(name,cb)=>callbacks.set(name,cb)}, {binary,collectorConfig:join(dir,'private-config')});
    const session = {id:'fixture',header:{id:'fixture',version:4}};
    callbacks.get('session/event')(session,{type:'user/message',seq:0,time:1,data:{content:[]}});
    let lines=readFileSync(output,'utf8').trim().split('\n').map(JSON.parse);
    assert.equal(lines.length,1);
    assert.ok(!lines[0].args.includes('--flush'));
    assert.equal(JSON.parse(lines[0].input).session.id,'fixture');
    callbacks.get('session/event')(session,{type:'turn/end',seq:1,time:2,data:{turn:1,reason:{kind:'completed'}}});
    callbacks.get('session/flush')(session);
    lines=readFileSync(output,'utf8').trim().split('\n').map(JSON.parse);
    assert.ok(lines[1].args.includes('--flush'));
    assert.equal(lines[2].args[0],'flush');
  } finally { rmSync(dir,{recursive:true,force:true}); }
});
