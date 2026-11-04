# Embedded computation contract — 0.1.15

## Specification

Let theta fix the release version, five locked algorithm/data packages, existing dayjs clock adapter, JS bundle bytes/hash, and all explicitly supplied civil date/clock/offset/as_of parameters. For the eight named registered operations, F_theta(x) is synchronous Result<JsonObject, ExecutionError>. Successful execution yields a business envelope {ok:true,data} or {ok:false,error:{code,message}}. Execution/protocol/artifact failures use the canonical CapabilityStdioResponse outer error, never an inner success-shaped substitute.

JSON equality compares exact key sets, values, missing vs null, array order/length, dates/strings/enums/counts. Public true-solar times and corrections are rounded to integer seconds and compared exactly. No global floating epsilon is used. This verifies engine equivalence of the business wrappers, not the astronomical or traditional-calendar theories for every possible input.

Civil-to-instant conversion is independently specified as UTC_millis = Gregorian_civil_as_UTC_millis - offset_minutes*60000. True-solar correction_seconds = returned_civil_as_UTC_seconds - supplied_civil_as_UTC_seconds. Host TZ must not affect either. The original UI algorithms are unchanged; backend adapts their Dayjs civil/instant inputs explicitly. dayjs.utc(...).utcOffset(offset,true) is not used: dayjs1.11.19 preserves a stale UTC flag on that path. dayjs1.11.19 applies its hours heuristic even after parsing HH:mm: minute offsets with absolute value <=16 are therefore supplied as fractional hours; larger values use numeric minutes. Independent +/-1 and +/-15 minute fixtures guard this adapter.

No invented common year domain: civil validation checks actual Gregorian date validity and JS Date representation; upstream constraints apply per operation. Primary source evidence: iztro2.5.3 lib/calendar/convertor.js solar2lunar explicitly requires 1900-01-31 through2100-12-31; tyme4ts1.3.9 dist/lib/index.mjs SolarYear checks -1<=year<=9999; lunar-typescript1.8.6 dist/lib/Solar.mjs constructor validates calendar fields/reform gap, not1901–2099. Finite fixtures1900/2000/2100 are boundary evidence only.

## Implementation mapping

- scripts/runtime-contract.mjs: D operation names, input schemas, business errors; scripts/runtime-schemas.mjs: explicit public JSON projections, not arbitrary npm exports.
- src/runtime/backend: shared original algorithms + explicit civil/instant and plain-data DTO projection. Class/function/cyclic iztro/tyme objects are not implicitly stringified.
- scripts/build-backend.mjs: existing esbuild closes all dependencies; no runtime external imports/require/process; full dependency licenses included. Builds a descriptor {version,path,sha256,bytes} compiled into the Rust binary.
- src/runtime/main.rs: canonical capability request/response, one terminal per process, host-selected execution budget; no private HTTP server. No guessed business capacity limits; stdin matches the existing canonical Host unary-frame bound. Host limits/deadline are passed as trusted transport metadata, not request body preferences. When Host memory/stack policy is unspecified, the engine's existing default is preserved.
- sync-js-runtime: each call a new Runtime/Context; strict plugin JSON rejects undefined/nonfinite/Promise/cycles/native objects; legacy Code codec remains separately fixed to historical stringify semantics. No strict-to-legacy fallback, async pump, pooling or bytecode cache.
- load_packaged_script: current executable's immutable version/bin root; exact compiled version, expected byte length and SHA over the same read; regular-file and canonical containment checks. No cwd or current/latest fallback. No arbitrary16MiB asset capacity guess.
- manifest managed-service operations use contribution point1flowbase.managed-service.operation / service.execute; existing UI payloads remain explicit declarative-only frontend contributions and workspace assignment+durable grants remain mandatory.
- existing Host owns request dispatch, authorization, deadline and process kill/reap. Kernel JS interrupt budget excludes arbitrary external slow filesystem I/O; cleanup progress assumes an alive Tokio owner/working OS. No claim of unconditional runtime-shutdown or filesystem latency bound.

## Verifiers and release

Runtime fixture tests provide >=24 normal/boundary/error observations plus independent Gregorian/UTC and true-solar self-consistency checks, TZ invariance, actual Rust outer envelopes, version-root relocation/missing/tamper. Host tests cover legacy codec compatibility and cancellation stop/JS exit/join observations, system-service frontend authority and explicit assignment. Final runtime evidence must observe D=R from the real installed host registry and authorized/unauthorized API calls; declarations alone are not proof.

Published runtime validation uses an isolated filesystem with only the package and necessary dynamic libraries, no Node/npm/pnpm/node_modules, and exec tracing. The exact SDK git pin also selects the cloud packaging host checkout. Host minimum0.6.2 prevents normal catalog installation on older0.6.1; no override should be used to claim old-host compatibility. Release validation includes signed platform packages, artifact metadata SHA and official registry/catalog, not merely extension-catalog checks.
