# Runtime acceptance corpus (T1 fixtures, not final QA)

Only Node built-ins and the current public DTO schema are used. Nothing installs,
rebuilds, starts a service, downloads, publishes or changes the UI. The corpus is
hand-authored in `corpus.mjs`; its expected values must not be regenerated from
`backend.js` or from QuickJS observations.

## Two independent gates

1. Known dates and independent arithmetic reject **equal-but-wrong** results:
   CNY 2024-02-10, lunar leap month 2023-03-22, Gregorian 1900/2000/2100,
   exact fixed-offset UTC (including ±1/±15 minutes and UTC date crossings), and
   exact true-solar `output civilUTC - input civilUTC` in integer seconds.
   Charts additionally check independently known CNY year/month facts, civil/UTC
   birth projection, tree structure and public DTO schema. This is not a second
   astronomical solver or a proof of every chart interpretation.
2. Every installed-binary corpus result must then equal the **entire** standalone
   Node wrapper result, exactly, including every number, second, array, string,
   null and error. There is no floating epsilon or lossy JSON sanitizer. Outputs
   must first be finite plain data trees (trusted cross-realm prototypes accepted;
   cycles, proxies, methods, symbols, accessors, sparse arrays and classes denied).

The schema constrains shape; it never supplies expected business values. The
harness has negative controls for malformed trees, equal-but-wrong UTC and a
one-second solar correction error. Fresh Node VMs are the normal lane; one reused
VM checks state sequences. Node subprocesses run the whole corpus under UTC,
Asia/Shanghai and America/New_York. The binary lane repeats all corpus cases and
runs eight-operation/offset/civil-crossing probes under the same timezones.

`iztro` 2.5.3 `lib/calendar/convertor.js` explicitly accepts
1900-01-31..2100-12-31 inclusive. Only Ziwei uses this bound. Successful fixtures
before/after it in other operations guard against a guessed global 1901..2099
range. Boundary success is required for every operation, not only invalid input.

## Commands (run from the plugin root)

Cheap Node-only development feedback, after the dataset is ready:

```sh
node --test scripts/_tests/runtime-harness.test.mjs scripts/_tests/runtime-reference.test.mjs
```

Binary acceptance is for Root's centralized frozen assembly, not per-packet QA:

```sh
RUNTIME_PACKAGE_ROOT=/absolute/install/0.1.18 \
TEST_EVIDENCE_DIR=/absolute/workspace/tmp/test-governance/calendar-runtime \
node --test scripts/_tests/runtime-binary.test.mjs
```

`RUNTIME_PACKAGE_ROOT` must contain `bin/taichuy-calendar` (or `.exe`) and the
matching `_assets/runtime/{backend.js,descriptor.json}` from the same build; its
basename must equal the descriptor version (`0.1.18` for this candidate). No
fallback to a locally built binary is allowed. If the variable is absent, **all
binary-only tests visibly SKIP; QuickJS/binary acceptance is NOT established**.
An explicitly configured but incomplete/mismatched package is a failure, not skip.

The tests copy only the executable and runtime assets into temporary version
roots, preserve the version basename on relocation, run from unrelated/decoy cwd
with no Node on PATH, and remove the copies in `after`. They never mutate the
provided installation. Wrong version/missing/tampered/escaped assets must fail at
the transport layer. Editing an on-disk descriptor cannot authorize changed code:
the binary's descriptor/hash/byte length are compiled in. Same-size tamper isolates
hash integrity from the byte-length guard.

The host envelope is exactly:

```json
{"method":"execute","input":{"handler":"birth.normalize","input_payload":{"path":{},"query":{},"body":{}},"execution_budget":{"timeout_ms":30000,"memory_bytes":null,"max_output_bytes":1048576}}}
```

Business success/error is inside outer `{ok:true,result:...,error:null}`; transport
errors are outer `{ok:false,result:null,error:{message:...,provider_summary:null}}`.
Input tests use the shared canonical 1 MiB unary boundary, not a plugin-private
64 KiB bound. Every call/process/output is bounded. Optional JSON observations
include asset SHA-256 and, in the binary lane, executable SHA-256/descriptor; keep
`TEST_EVIDENCE_DIR` under `tmp/test-governance/`. Passing reference tests alone does
not establish the binary lane or final product QA. Rebuild/copy the candidate when
Root changes backend source; stale assets are not evidence for the new source.
