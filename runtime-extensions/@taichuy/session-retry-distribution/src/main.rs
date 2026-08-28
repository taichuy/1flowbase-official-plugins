use std::io::{self, BufRead, Write};

use serde::Deserialize;
use serde_json::{json, Value};

const HOST_CALL_PROTOCOL: &str = "runtime_host_call/v1";

#[derive(Debug, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case", deny_unknown_fields)]
enum HostFrame {
    HostResult {
        protocol: String,
        call_id: String,
        #[serde(default)]
        result: Option<Value>,
        #[serde(default)]
        error: Option<HostError>,
    },
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
#[serde(deny_unknown_fields)]
struct HostError {
    kind: String,
    code: String,
    retryable: bool,
}

fn main() {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut stdout = io::stdout().lock();
    loop {
        let mut line = String::new();
        if stdin.read_line(&mut line).unwrap_or_default() == 0 {
            break;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => select(&request, &mut stdin, &mut stdout).unwrap_or_else(|_| {
                json!({"decision":"no_eligible_target","reason":"plugin_data_unavailable"})
            }),
            Err(_) => json!({"decision":"no_eligible_target","reason":"invalid_invocation"}),
        };
        writeln!(stdout, "{}", json!({"ok":true,"result":response})).ok();
        stdout.flush().ok();
    }
}

fn select(
    request: &Value,
    stdin: &mut impl BufRead,
    stdout: &mut impl Write,
) -> io::Result<Value> {
    if request.get("method").and_then(Value::as_str) != Some("select_distribution") {
        return Ok(json!({"decision":"no_eligible_target","reason":"unsupported_method"}));
    }
    let input = &request["input"];
    let conversation_id = input.get("conversation_id").and_then(Value::as_str);
    let Some(conversation_id) = conversation_id else {
        return Ok(json!({"decision":"no_eligible_target","reason":"conversation_required"}));
    };
    let routing_policy_id = input["routing_policy_id"].as_str().unwrap_or_default();
    let rule_version = input["rule_version"].as_str().unwrap_or_default();
    let identity = json!({
        "routing_policy_id":{"type":"string","value":routing_policy_id},
        "conversation_id":{"type":"string","value":conversation_id},
        "rule_version":{"type":"string","value":rule_version}
    });
    if let Some(target_id) = find_affinity(stdin, stdout, &identity)? {
        return Ok(decision_for_bound_target(input, &target_id));
    }
    let Some(target_id) = input["candidates"].as_array().and_then(|candidates| {
        candidates.iter().find(|candidate| candidate["ready"] == true)
            .and_then(|candidate| candidate["target_id"].as_str())
    }) else {
        return Ok(json!({"decision":"no_eligible_target","reason":"no_eligible_target"}));
    };
    host_call(stdin, stdout, "affinity-upsert", json!({
        "operations":[{"operation":"upsert","target":{"kind":"owned_collection","collection_code":"affinity"},
            "identity":identity,"values":{"target_id":{"type":"string","value":target_id}}}],
        "idempotency_key":format!("{}:{}:{}", routing_policy_id, conversation_id, rule_version)
    }))?;
    let winner = find_affinity(stdin, stdout, &identity)?.unwrap_or_else(|| target_id.to_string());
    Ok(decision_for_bound_target(input, &winner))
}

fn find_affinity(
    stdin: &mut impl BufRead,
    stdout: &mut impl Write,
    identity: &Value,
) -> io::Result<Option<String>> {
    let filters = identity.as_object().into_iter().flatten().map(|(field, value)| {
        json!({"field":field,"operator":"equal","value":value})
    }).collect::<Vec<_>>();
    let response = host_call(stdin, stdout, "affinity-find", json!({
        "operations":[{"operation":"find_one","target":{"kind":"owned_collection","collection_code":"affinity"},
            "fields":["target_id"],"filters":filters}]
    }))?;
    Ok(response.pointer("/results/0/row/values/target_id/value")
        .and_then(Value::as_str).map(str::to_string))
}

fn host_call(
    stdin: &mut impl BufRead,
    stdout: &mut impl Write,
    call_id: &str,
    request: Value,
) -> io::Result<Value> {
    writeln!(stdout, "{}", json!({"frame":"host_call","protocol":HOST_CALL_PROTOCOL,"call_id":call_id,
        "service":"plugin_data/v1","request":request}))?;
    stdout.flush()?;
    let mut line = String::new();
    stdin.read_line(&mut line)?;
    let frame: HostFrame = serde_json::from_str(line.trim()).map_err(io::Error::other)?;
    match frame {
        HostFrame::HostResult {
            protocol,
            call_id: response_call_id,
            result: Some(result),
            error: None,
        } if protocol == HOST_CALL_PROTOCOL && response_call_id == call_id => Ok(result),
        HostFrame::HostResult { .. } => Err(io::Error::other(
            "uncorrelated or failed runtime host result",
        )),
    }
}

fn decision_for_bound_target(input: &Value, target_id: &str) -> Value {
    if input["candidates"].as_array().is_some_and(|candidates| candidates.iter().any(|candidate| {
        candidate["ready"] == true && candidate["target_id"].as_str() == Some(target_id)
    })) {
        json!({"decision":"select","target_id":target_id})
    } else {
        json!({"decision":"no_eligible_target","reason":"affinity_target_unavailable"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn call(response: &str) -> io::Result<Value> {
        let mut input = Cursor::new(format!("{response}\n"));
        let mut output = Vec::new();
        host_call(&mut input, &mut output, "affinity-find", json!({"operations":[]}))
    }

    #[test]
    fn rejects_host_error() {
        assert!(call(r#"{"frame":"host_result","protocol":"runtime_host_call/v1","call_id":"affinity-find","error":{"kind":"storage_unavailable","code":"storage_unavailable","retryable":true}}"#).is_err());
    }

    #[test]
    fn rejects_mismatched_call_id() {
        assert!(call(r#"{"frame":"host_result","protocol":"runtime_host_call/v1","call_id":"other","result":{"results":[]}}"#).is_err());
    }

    #[test]
    fn rejects_missing_or_ambiguous_result() {
        assert!(call(r#"{"frame":"host_result","protocol":"runtime_host_call/v1","call_id":"affinity-find"}"#).is_err());
        assert!(call(r#"{"frame":"host_result","protocol":"runtime_host_call/v1","call_id":"affinity-find","result":{"results":[]},"error":{"kind":"storage_unavailable","code":"storage_unavailable","retryable":true}}"#).is_err());
    }
}
