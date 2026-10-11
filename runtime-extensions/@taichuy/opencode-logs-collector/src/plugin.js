import { spawnSync } from "node:child_process";

// Installation values are JSON literals, never shell-interpolated commands.
const binary = __COLLECTOR_BINARY__;
const config = __COLLECTOR_CONFIG__;
const supported = new Set([
  "message.updated", "message.part.updated",
  "session.next.prompted", "session.next.step.started", "session.next.step.ended",
  "session.next.step.failed", "session.next.text.ended", "session.next.reasoning.ended",
  "session.next.tool.called", "session.next.tool.success", "session.next.tool.failed",
  "session.next.context.updated", "session.next.synthetic",
]);
export default async function AgentLogs() {
  let systemSequence = 0;
  function record(session, event) {
    const result = spawnSync(binary, ["record", "--config", config, "--session", session],
      { input: JSON.stringify(event), encoding: "utf8", stdio: ["pipe", "ignore", "pipe"] });
    if (result.error || result.status !== 0) console.error("1flowbase agent logs: native event was not recorded");
  }
  function flush() {
    const result = spawnSync(binary, ["flush", "--config", config], { encoding: "utf8", stdio: ["ignore", "ignore", "pipe"] });
    if (result.error || result.status !== 0) console.error("1flowbase agent logs: delivery remains queued");
  }
  return {
    event({ event }) {
      const session = event.properties?.sessionID ?? event.properties?.info?.sessionID ?? event.properties?.part?.sessionID;
      if (supported.has(event.type) && typeof session === "string") {
        record(session, event);
      }
      // Idle does not imply model success. It only retries already durable facts.
      if (event.type === "session.idle") flush();
    },
    // Official request preparation hook: observe the array, never alter it.
    // Other plugins may transform it later; this is a hook snapshot, not a wire claim.
    async "experimental.chat.system.transform"(input, output) {
      if (typeof input.sessionID !== "string") return;
      const timestamp = Date.now();
      record(input.sessionID, {
        id: `system_${timestamp}_${++systemSequence}`,
        type: "collector.system.transform",
        properties: {
          sessionID: input.sessionID, timestamp, system: [...output.system],
          model: { id: input.model?.id, providerID: input.model?.providerID },
        },
      });
    },
    dispose() { flush(); },
  };
}
