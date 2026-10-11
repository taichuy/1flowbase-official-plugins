import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

async function plugin() {
  const calls = [];
  const source = (await readFile(new URL("../plugin.js", import.meta.url), "utf8"))
    .replace('import { spawnSync } from "node:child_process";', 'const spawnSync = mockSpawn;')
    .replace("__COLLECTOR_BINARY__", JSON.stringify("/fixture/collector binary"))
    .replace("__COLLECTOR_CONFIG__", JSON.stringify("/fixture/config.json"))
    .replace("export default async function AgentLogs()", "async function AgentLogs()");
  const sandbox = vm.createContext({
    mockSpawn(binary, args, options) { calls.push({binary,args:Array.from(args),options}); return {status:0}; },
    console,
  });
  vm.runInContext(source + "\nthis.createPlugin = AgentLogs;", sandbox);
  return { hooks: await sandbox.createPlugin(), calls };
}
test("default native V1 inner session shapes synchronously reach record", async () => {
  const {hooks,calls} = await plugin();
  for (const event of [
    {type:"message.updated",properties:{info:{sessionID:"ses_fixture",id:"msg_fixture",role:"user"}}},
    {type:"message.part.updated",properties:{part:{sessionID:"ses_fixture",id:"prt_fixture",messageID:"msg_fixture",type:"text",text:"fixture"}}},
  ]) {
    hooks.event({event});
    assert.equal(calls.length, event.type === "message.updated" ? 1 : 2);
    assert.deepEqual(calls.at(-1).args,["record","--config","/fixture/config.json","--session","ses_fixture"]);
    assert.deepEqual(JSON.parse(calls.at(-1).options.input),event);
    assert.equal(calls.at(-1).options.shell,undefined);
  }
  hooks.event({event:{type:"message.part.delta",properties:{sessionID:"ses_fixture",delta:"partial"}}});
  assert.equal(calls.length,2);
  hooks.event({event:{type:"session.idle",properties:{sessionID:"ses_fixture"}}});
  assert.deepEqual(calls.at(-1).args,["flush","--config","/fixture/config.json"]);
  hooks.dispose();
  assert.equal(calls.length,4);
});
test("V2 canonical event also reaches record without asynchronous delivery gap", async () => {
  const {hooks,calls} = await plugin();
  hooks.event({event:{type:"session.next.text.ended",properties:{sessionID:"ses_fixture",text:"done"}}});
  assert.equal(calls.length,1);
  assert.equal(calls[0].args.at(-1),"ses_fixture");
});

test("official native system hook records a snapshot without changing the array", async () => {
  const { hooks, calls } = await plugin();
  const output = { system: ["provider instructions", "project instructions"] };
  const before = JSON.stringify(output);
  await hooks["experimental.chat.system.transform"]({
    sessionID: "ses_fixture", model: { id: "native-model", providerID: "gateway" },
  }, output);
  assert.equal(JSON.stringify(output), before);
  const event = JSON.parse(calls[0].options.input);
  assert.equal(event.type, "collector.system.transform");
  assert.deepEqual(event.properties.system, output.system);
  assert.equal(event.properties.sessionID, "ses_fixture");
  assert.equal(event.properties.model.id, "native-model");
  assert.equal(typeof event.properties.timestamp, "number");
  assert.equal(calls[0].args.at(-1), "ses_fixture");
  await hooks["experimental.chat.system.transform"]({ model: {} }, output);
  assert.equal(calls.length, 1, "non-session small-model requests have no source session");
});
