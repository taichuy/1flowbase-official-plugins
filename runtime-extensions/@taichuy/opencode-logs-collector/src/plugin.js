import { spawnSync } from "node:child_process";

// Installation values are JSON literals, never shell-interpolated commands.
const binary = __COLLECTOR_BINARY__;
const config = __COLLECTOR_CONFIG__;
const supported = new Set([
  "session.next.prompted", "session.next.step.started", "session.next.step.ended",
  "session.next.step.failed", "session.next.text.ended", "session.next.reasoning.ended",
  "session.next.tool.called", "session.next.tool.success", "session.next.tool.failed",
  "session.next.context.updated", "session.next.synthetic",
]);
export default async function AgentLogs() {
  function flush() {
    const result = spawnSync(binary, ["flush", "--config", config], { encoding: "utf8", stdio: ["ignore", "ignore", "pipe"] });
    if (result.error || result.status !== 0) console.error("1flowbase agent logs: delivery remains queued");
  }
  return {
    event({ event }) {
      const session = event.properties?.sessionID;
      if (supported.has(event.type) && typeof session === "string") {
        const result = spawnSync(binary, ["record", "--config", config, "--session", session],
          { input: JSON.stringify(event), encoding: "utf8", stdio: ["pipe", "ignore", "pipe"] });
        if (result.error || result.status !== 0) console.error("1flowbase agent logs: native event was not recorded");
      }
      // Idle does not imply model success. It only retries already durable facts.
      if (event.type === "session.idle") flush();
    },
    dispose() { flush(); },
  };
}
