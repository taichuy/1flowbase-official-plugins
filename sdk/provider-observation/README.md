# Provider observation SDK

Repository-local path dependency; providers keep independent Cargo targets and packages.
`provider_adapter!()` maps the shared observation and negotiated outer request envelope to
provider-local wire types. Transport hooks observe serialized request bodies before sending,
HTTP response bytes before decoding, SSE chunks, and WebSocket messages. Headers, URLs and
authentication exchanges are excluded. `request_prepared` is not proof of successful send.

Each invocation owns a 128-message / 1 MiB encoded observation buffer. `record` is synchronous
and never waits for capacity. Business callbacks bypass this buffer and write through the
existing single stdio sink. Capture polls business work before draining one observation;
there is no second stdout writer or unbounded output channel. Shared stdout still imposes
physical pipe backpressure; the host must keep draining it independently of log persistence.

Overflow drops observation records only and emits an out-of-band `capture_integrity` body
`{"dropped_count": N, "reason": "observation_capacity_exceeded"}` before returning. It suppresses
`stream_end`. Transport failure also omits `stream_end`. Cancellation/process exit may prevent
any final marker: the host owns invocation lifecycle and must classify missing terminal evidence
as incomplete, never complete. The host must also preserve integrity markers outside a full
observation queue. Raw non-UTF8 bytes are base64 encoded without lossy text conversion.

Validation after assembly:
- `cargo test --manifest-path sdk/provider-observation/Cargo.toml`
- Each provider: `cargo test --manifest-path runtime-extensions/@taichuy/<provider>/Cargo.toml protocol_observation`
- OpenAI: `cargo test --manifest-path runtime-extensions/@taichuy/openai/Cargo.toml native_tool_roundtrip`
- `node --test scripts/_tests/detect-version-releases.test.mjs`

Local production binary: `cargo build --release --manifest-path runtime-extensions/@taichuy/<provider>/Cargo.toml`.
Release workflow builds from the repository checkout so this SDK is available. SDK changes
require a manifest version bump for all seven consumers before release detection succeeds.
