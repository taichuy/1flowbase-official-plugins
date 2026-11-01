//! One stdio request per process. Host owns registration/auth/policy/deadline/process reap.
use runtime_extension_sdk::{
    javascript::{load_packaged_script, run_script, Limits, PackagedScript},
    CapabilityExecutionBudget, CapabilityStdioError, CapabilityStdioMethod, CapabilityStdioRequest,
    CapabilityStdioResponse, CAPABILITY_STDIO_FRAME_BYTES,
};
use serde_json::{json, Value};
use std::{
    io::{self, Read, Write},
    time::Duration,
};
const DESCRIPTOR: &str = include_str!("../../_assets/runtime/descriptor.json");
fn transport_error(code: &str) -> CapabilityStdioResponse {
    CapabilityStdioResponse {
        ok: false,
        result: Value::Null,
        error: Some(CapabilityStdioError {
            message: code.to_owned(),
            provider_summary: None,
        }),
    }
}
fn execute(bytes: &[u8]) -> CapabilityStdioResponse {
    let request: CapabilityStdioRequest = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(_) => return transport_error("invalid_request"),
    };
    if request.method != CapabilityStdioMethod::Execute {
        return transport_error("unsupported_method");
    }
    let Some(operation) = request.input.get("handler").and_then(Value::as_str) else {
        return transport_error("invalid_request");
    };
    let Some(body) = request.input.pointer("/input_payload/body") else {
        return transport_error("invalid_request");
    };
    let budget: CapabilityExecutionBudget = match request
        .input
        .get("execution_budget")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
    {
        Some(v) => v,
        None => return transport_error("execution_budget_missing"),
    };
    let descriptor: PackagedScript = match serde_json::from_str(DESCRIPTOR) {
        Ok(v) => v,
        Err(_) => return transport_error("artifact_invalid"),
    };
    let source = match load_packaged_script(env!("CARGO_PKG_VERSION"), &descriptor) {
        Ok(v) => v,
        Err(e) => return transport_error(e.code()),
    };
    let Ok(memory_limit_bytes) = usize::try_from(budget.memory_bytes.unwrap_or(0)) else {
        return transport_error("invalid_limits");
    };
    let Ok(max_output_bytes) = usize::try_from(budget.max_output_bytes) else {
        return transport_error("invalid_limits");
    };
    // Reserve the exact canonical envelope overhead, not an estimated payload quota.
    let envelope_bytes = serde_json::to_vec(&CapabilityStdioResponse {
        ok: true,
        result: Value::Null,
        error: None,
    })
    .expect("fixed envelope serializes")
    .len()
        - 4;
    let Some(max_output_bytes) = max_output_bytes.checked_sub(envelope_bytes + 1) else {
        return transport_error("invalid_limits");
    };
    let limits = Limits {
        timeout: Duration::from_millis(budget.timeout_ms),
        memory_limit_bytes,
        stack_size_bytes: 0,
        max_output_bytes,
    };
    match run_script(
        &source,
        &json!({"operation": operation, "body": body}),
        limits,
    ) {
        Ok(output) => CapabilityStdioResponse {
            ok: true,
            result: output.output_payload,
            error: None,
        },
        Err(error) => transport_error(error.code()),
    }
}
fn main() -> io::Result<()> {
    let mut bytes = Vec::new();
    // Match the existing Host unary frame contract; not a private business/input quota.
    let response = match io::stdin()
        .take(CAPABILITY_STDIO_FRAME_BYTES + 1)
        .read_to_end(&mut bytes)
    {
        Ok(_) if bytes.len() <= CAPABILITY_STDIO_FRAME_BYTES as usize => execute(&bytes),
        Ok(_) => transport_error("input_frame_exceeded"),
        Err(_) => transport_error("input_read_failed"),
    };
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    serde_json::to_writer(&mut writer, &response)?;
    writeln!(writer)
}
