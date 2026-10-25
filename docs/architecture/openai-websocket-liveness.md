# OpenAI WebSocket establishment and liveness boundaries

## One deadline for establishing a connection

`runtime-extensions/@taichuy/openai/src/lib.rs` owns connection establishment. The direct path covers DNS, TCP, optional TLS and WebSocket Upgrade. The proxy path also covers the HTTP CONNECT exchange. `SocketOwner` exists only after these operations succeed, so its control-frame handling cannot detect a stalled handshake.

`bounded_websocket_establishment` bounds the whole establishment future with one monotonic deadline. It reuses the existing ten-second transport write allowance from `websocket_io::WRITE_TIMEOUT` and takes the earlier remaining absolute recovery deadline when present:

```text
D = monotonic_start + min(existing_transport_allowance, remaining_request_budget)
```

The budget is shared across all stages and header reads. Progress into TLS or Upgrade does not grant another allowance. An already-expired request does not poll the network future. A successful raw connection is accepted strictly before D; a success at D is dropped before constructing a socket owner. The caller rechecks the request deadline before activating the close ledger or inserting physical/logical session mappings. The existing deadline check before `response.create` remains in place.

The allowance limits establishing a transport, independently of context size or established inference duration. A healthy but unusually slow handshake can exceed it. Cooperative async scheduling is required for the timer to run. Dropping the pending future releases sockets and proxy tunnels that it owns; an OS DNS resolver task already executing on a blocking worker may finish later.

## Errors, recovery and ownership

A local establishment timeout uses the existing typed transport error path with `kind=websocket_connect_timeout` and `reason_category=transport_disconnected`. An already-expired request retains the existing `attempt_admission / deadline_exceeded` classification. Genuine authorization, protocol, proxy and I/O errors produced by the underlying connection future retain their original diagnosis.

The existing recovery FSM owns retry eligibility, attempt accounting, first-failure evidence and context rebuild. The timeout helper does not create a retry, reset an attempt budget, replay committed output or claim that a provider completed business work. No application `response.create` is sent by connection establishment. A rejected setup produces no registered session or close-ledger activation.

The host's connection-scope closure and orphaned invocation admission remain separate from business cancellation. A local transport failure does not manufacture a provider or subscriber acknowledgement.

## Established connections and large messages

The existing physical socket owner, event-count and byte backpressure, Ping/Pong handling, oversized-message support and terminal FIFO behavior remain in use. This change adds no complete-message receive timer: complete-message delivery does not expose partial TCP progress and cannot alone distinguish a slowly received large frame from receive silence.

Existing stream policies remain separate. In particular, `STREAM_IDLE_TIMEOUT` still bounds waiting for the next application-facing event at five minutes; Pong frames are consumed by the socket owner and do not reset that application-facing timer. The establishment change therefore makes no claim that the entire provider invocation can run indefinitely without application output.

## Verification and quality layers

- Level 1: formatting, version consistency and focused `websocket_connect_tests`. Paused-clock tests check one cumulative budget, earlier request deadline, expired zero-poll behavior, exact success-boundary cleanup, original error preservation and accepted-resource survival beyond the setup budget.
- Level 2: real loopback fixtures cover a silent direct Upgrade, a silent TLS handshake, a silent proxy CONNECT and a successful CONNECT followed by a silent Upgrade. Each observes actual request bytes before pausing time, then verifies EOF/reset after timeout. Existing healthy proxy, compression, large-message, recovery, continuation and diagnostics regressions remain active.
- Level 3: existing provider package/conformance and release workflows establish artifact/host source identity. Installing a candidate into a running gateway is a separate operational action.

A finite-state model checks the shared-deadline contract separately from the Rust fixtures. Its mathematical result is an abstraction proof; real loopback tests supply evidence for the library and resource-ownership boundary. Historical errors that identify connection setup do not reveal whether DNS, TCP, proxy, TLS or Upgrade consumed the historical wait without per-stage timing evidence.
