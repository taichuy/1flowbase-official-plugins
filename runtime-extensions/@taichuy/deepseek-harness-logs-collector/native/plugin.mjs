// Native Cordis lifecycle integration; no persistence/history reader.
import { spawnSync } from 'node:child_process';
export const name = '1flowbase-deepseek-harness-logs';
export function apply(ctx, config) {
  if (!config?.binary || !config?.collectorConfig) throw new Error('Collector paths are required');
  const invoke = (args, input) => {
    const result = spawnSync(config.binary, args, {
      input, encoding: 'utf8', stdio: ['pipe', 'ignore', 'pipe'],
      windowsHide: true,
    });
    if (result.error || result.status !== 0) {
      // Never echo stdin, config or child stderr: these may contain private facts.
      console.error('[1flowbase collector] journal or delivery failed; run collector flush to retry');
    }
  };
  ctx.on('session/event', (session, event) => {
    // session/event is fire-and-forget. The journal must be synced before return.
    // Terminal records also drain because turn/end itself does not trigger session/flush.
    invoke(['record', '--config', config.collectorConfig, '--session', String(session.id),
      ...(event.type === 'turn/end' ? ['--flush'] : [])],
      JSON.stringify({ type: 'dsh_event', session: session.header, event }));
  });
  ctx.on('session/flush', () => {
    invoke(['flush', '--config', config.collectorConfig]);
  });
}
