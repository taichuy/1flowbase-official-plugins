# OpenAI WebSocket receive liveness

## Ownership and failure boundary

`runtime-extensions/@taichuy/openai/src/websocket_io.rs` has one task per physical WebSocket. That task owns reads, writes, control frames and physical close. Provider invocations consume its ordered event mailbox; a mailbox or physical socket does not own the host's logical invocation, continuation lease or business cancellation.

A successful write establishes that the local sink accepted the frame. It does not establish that the receive direction is making progress. The existing 20-second Ping cadence now also drives a receive-liveness probe:

1. A successfully flushed Ping arms a deadline one keepalive interval after write completion, provided socket reads are enabled and no probe is outstanding.
2. Any real inbound frame clears the outstanding probe. Application data, Ping and Pong are all receive progress. This is a transport-progress check, not matching-Pong authentication.
3. If the full observation window expires without inbound progress, the owner records `read_liveness_timeout`, `transport_disconnected` and `io_error_kind=timed_out`, then leaves its ordinary transport failure path.
4. The existing recovery state machine decides whether reconnect, context rebuild or termination is safe. The liveness detector does not grant retry eligibility or manufacture a provider/subscriber acknowledgement.

No context-length, message-size or inference-duration ceiling is added. An upstream that returns timely Pong frames can continue reasoning without application output. The detector assumes timely control-frame processing: a peer delayed beyond the observation window is indistinguishable from a failed receive path at this layer.

## Backpressure and scheduling

The bounded event mailbox and byte semaphore are delivery scheduling windows. Oversized messages continue to acquire the whole byte window rather than being rejected. When a frame is pending admission, the actor deliberately stops reading from the socket. Receive-liveness inference is cleared and suspended for that entire interval, because an unread Pong cannot distinguish local backpressure from peer failure.

After admission resumes, a subsequent successful Ping starts a fresh full window. A delayed interval tick never counts as an already-completed probe window. Explicit close and command processing retain their existing ownership and priority.

Keepalive writes remain ahead of continuously ready data to avoid starving control writes. Inbound reads precede the separate expiry branch, so a frame and deadline ready together are resolved by reading the frame. The probe uses an explicit Tokio deadline; later keepalive ticks neither shorten nor extend it. In particular, a slow successful Ping gets a complete interval after its flush, not the remainder of the original interval phase.

Under fair scheduling, finite higher-priority work and successful writes, a peer silent from connection establishment is detected after the first Ping opportunity plus one complete observation window. With immediate writes this is approximately two 20-second intervals. The existing 10-second write timeout bounds each individual write. Scheduling, command processing, a write at the expiry boundary and intentional consumer backpressure mean this is not an unconditional 40-second wall-clock service guarantee.

## Terminal ordering

The new failure does not invalidate already-buffered events. A previously received `response.completed` is delivered before the later transport error. Capacity, explicit owner cancellation and other existing invalidating failures retain their distinct behavior. A local socket failure is not proof that the provider stopped executing, and does not change host orphan/admission serialization or business cancellation.

## Verification boundaries

The existing owner test module contains paused-clock scenarios for receive silence despite successful writes, timely Pong during extended inference, inbound/deadline ties, slow Ping flush, count and byte backpressure plus resumption, application progress, completion-before-error and the existing write timeout. Tests use Tokio's existing dev-only `test-util` feature; production dependency features and package dependencies are unchanged.

The tests also retain real loopback WebSocket checks for control-frame handling, large requests/responses, event order, cancellation and independent socket ownership. Recovery and diagnostics tests remain the authority for retry eligibility and typed error preservation.

Quality layers for this change:

- Level 1: Rust formatting, metadata alignment and the focused `websocket_io` owner tests. Paused time replaces real network waiting for deadline boundaries.
- Level 2: related transport, recovery, continuation and diagnostics tests, plus a controlled negative using the prior owner with the new receive-silence fixture.
- Level 3: the existing provider packaging/conformance and release workflows, with exact host/plugin source identity and isolated provider fixtures. Installing a package into an active gateway is a separate operational action.

Finite-state model checks can establish the stated safety and conditional liveness properties of an abstraction. Rust tests must separately exercise the timer, stream and actor refinement. Neither source analysis nor a successful isolated fixture reconstructs missing historical per-attempt Ping/Pong telemetry.
