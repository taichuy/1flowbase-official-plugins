use super::*;
use serde::Deserialize;
use std::collections::BTreeSet;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    application_id: String,
    started_at_from: Option<String>,
    started_at_to: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Export {
    instance_id: Option<String>,
    application_id: String,
    record_ids: Option<Vec<String>>,
    log_conversation_ids: Option<Vec<String>>,
    started_at_from: Option<String>,
    started_at_to: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Import {
    instance_id: Option<String>,
    application_id: String,
    trace_ids: Option<Vec<String>>,
    started_at_from: Option<String>,
    started_at_to: Option<String>,
}
#[derive(Default, Serialize)]
struct ExportReceipt {
    exported_records: usize,
    exported_spans: usize,
    trace_ids: BTreeSet<String>,
}
#[derive(Default, Serialize)]
struct ImportReceipt {
    imported_traces: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    partial_committed_records: Option<usize>,
    accepted_events: usize,
    duplicate_events: usize,
    record_ids: BTreeSet<String>,
}
fn input<T: serde::de::DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body).map_err(|_| fail("langfuse_input"))
}
fn unique(ids: Vec<String>) -> Result<Vec<String>> {
    if ids.is_empty() || ids.iter().any(|id| id.trim().is_empty()) {
        return Err(fail("langfuse_selector"));
    }
    Ok(ids
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}
pub(crate) fn date_range(
    from: Option<&str>,
    to: Option<&str>,
) -> Result<(OffsetDateTime, OffsetDateTime)> {
    let from = from
        .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
        .ok_or_else(|| fail("langfuse_date_range"))?;
    let to = to
        .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
        .ok_or_else(|| fail("langfuse_date_range"))?;
    if from >= to {
        return Err(fail("langfuse_date_range"));
    }
    Ok((from, to))
}
fn partial<T: Serialize>(mut error: Failure, receipt: &T) -> Failure {
    error.details = Some(json!({"progress":receipt,"cause_details":error.details}));
    error
}
pub(crate) async fn execute(host: &Host, handler: &str, payload: Value) -> Result<Value> {
    let body = payload.get("body").cloned().unwrap_or_else(|| json!({}));
    let id = || {
        payload
            .pointer("/path/instance_id")
            .and_then(Value::as_str)
            .ok_or_else(|| fail("langfuse_instance_id"))
    };
    match handler {
        "openapi_discover" | "openapi_get" | "openapi_execute" => {
            openapi::execute(host, handler, body).await
        }
        "instances_list" => {
            config::list(
                host,
                payload.get("query").cloned().unwrap_or_else(|| json!({})),
            )
            .await
        }
        "instances_create" => config::create(host, body).await,
        "instances_update" => config::update(host, id()?, body).await,
        "instances_delete" => config::delete(host, id()?).await,
        "instances_test" => remote::Remote::new(host, id()?).await?.test().await,
        "config_get" => config::read(host).await,
        "config_put" => config::save(host, body).await,
        "test" => {
            remote::Remote::new(host, config::DEFAULT)
                .await?
                .test()
                .await
        }
        "applications" => match host
            .logs(ApplicationLogsRequest::ListApplications {
                application_types: vec!["agent_flow".into(), "agent_logs".into()],
            })
            .await?
        {
            ApplicationLogsResponse::Applications { items } => {
                Ok(json!({"success":true,"items":items}))
            }
            _ => Err(fail("host_application_logs_response")),
        },
        "records_query" => {
            let query: Query = input(body)?;
            match host
                .logs(ApplicationLogsRequest::ListRecords {
                    application_id: query.application_id,
                    started_at_from: query.started_at_from,
                    started_at_to: query.started_at_to,
                    cursor: query.cursor,
                    limit: query.limit,
                })
                .await?
            {
                ApplicationLogsResponse::Records { items, next_cursor } => {
                    Ok(json!({"success":true,"items":items,"next_cursor":next_cursor}))
                }
                _ => Err(fail("host_application_logs_response")),
            }
        }
        "export" => export(host, input(body)?).await,
        "import" => import(host, input(body)?).await,
        _ => Err(fail("langfuse_operation")),
    }
}
async fn export(host: &Host, request: Export) -> Result<Value> {
    let mut selectors = Vec::new();
    match (
        request.record_ids,
        request.log_conversation_ids,
        request.started_at_from,
        request.started_at_to,
    ) {
        (Some(records), None, None, None) => {
            for record in unique(records)? {
                selectors.push((Some(vec![record]), None));
            }
        }
        (None, Some(conversations), None, None) => {
            for conversation in unique(conversations)? {
                selectors.push((None, Some(vec![conversation])));
            }
        }
        (None, None, Some(from), Some(to)) => {
            date_range(Some(&from), Some(&to))?;
            let mut cursor = None;
            let mut seen = BTreeSet::new();
            let mut records = BTreeSet::new();
            loop {
                match host
                    .logs(ApplicationLogsRequest::ListRecords {
                        application_id: request.application_id.clone(),
                        started_at_from: Some(from.clone()),
                        started_at_to: Some(to.clone()),
                        cursor,
                        limit: None,
                    })
                    .await?
                {
                    ApplicationLogsResponse::Records { items, next_cursor } => {
                        if items.is_empty() && next_cursor.is_some() {
                            return Err(fail("host_application_logs_cursor"));
                        }
                        for record in items {
                            records.insert(record.record_id);
                        }
                        match next_cursor {
                            None => break,
                            Some(next) if seen.insert(next.clone()) => cursor = Some(next),
                            _ => return Err(fail("host_application_logs_cursor")),
                        }
                    }
                    _ => return Err(fail("host_application_logs_response")),
                }
            }
            for record in records {
                selectors.push((Some(vec![record]), None));
            }
        }
        _ => return Err(fail("langfuse_selector")),
    }
    let remote = remote::Remote::new(
        host,
        request.instance_id.as_deref().unwrap_or(config::DEFAULT),
    )
    .await?;
    let mut receipt = ExportReceipt::default();
    let mut exported_records = BTreeSet::new();
    for (record_ids, log_conversation_ids) in selectors {
        let step = async {
            match host
                .logs(ApplicationLogsRequest::ExportOtlp {
                    application_id: request.application_id.clone(),
                    record_ids,
                    log_conversation_ids,
                })
                .await?
            {
                ApplicationLogsResponse::Exported {
                    otlp,
                    record_ids,
                    trace_ids,
                    exported_spans,
                } => {
                    remote.export(mapping::to_langfuse(otlp)?).await?;
                    exported_records.extend(record_ids);
                    receipt.exported_records = exported_records.len();
                    receipt.exported_spans += exported_spans;
                    receipt.trace_ids.extend(trace_ids);
                    Ok(())
                }
                _ => Err(fail("host_application_logs_response")),
            }
        }
        .await;
        if let Err(error) = step {
            return Err(partial(error, &receipt));
        }
    }
    Ok(json!({"success":true,"receipt":receipt}))
}
async fn import(host: &Host, request: Import) -> Result<Value> {
    // Recheck target kind in the Host before making any vendor request. ImportOtlp
    // independently checks actor/workspace/operation/reception authorization.
    let applications = host
        .logs(ApplicationLogsRequest::ListApplications {
            application_types: vec!["agent_logs".into()],
        })
        .await?;
    match applications {
        ApplicationLogsResponse::Applications { items }
            if items.iter().any(|app| {
                app.application_id == request.application_id && app.application_type == "agent_logs"
            }) =>
        {
            ()
        }
        _ => return Err(fail("langfuse_target_agent_logs_required")),
    }
    let remote = remote::Remote::new(
        host,
        request.instance_id.as_deref().unwrap_or(config::DEFAULT),
    )
    .await?;
    let traces = match (
        request.trace_ids,
        request.started_at_from,
        request.started_at_to,
    ) {
        (Some(ids), None, None) => unique(ids)?,
        (None, Some(from), Some(to)) => remote.traces(&from, &to).await?,
        _ => return Err(fail("langfuse_selector")),
    };
    let mut receipt = ImportReceipt::default();
    for trace in traces {
        let step = async {
            let observations = remote.observations(&trace).await?;
            let otlp = mapping::from_observations(&observations)?;
            match host
                .logs(ApplicationLogsRequest::ImportOtlp {
                    application_id: request.application_id.clone(),
                    otlp,
                })
                .await?
            {
                ApplicationLogsResponse::Imported {
                    accepted_events,
                    duplicate_events,
                    record_ids,
                } => {
                    receipt.accepted_events += accepted_events;
                    receipt.duplicate_events += duplicate_events;
                    receipt.record_ids.extend(record_ids);
                    receipt.imported_traces += 1;
                    Ok(())
                }
                _ => Err(fail("host_application_logs_response")),
            }
        }
        .await;
        if let Err(error) = step {
            if error.code == "agent_logs_partial_import" {
                if let Some(details) = &error.details {
                    receipt.accepted_events += details
                        .get("accepted_events")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;
                    receipt.duplicate_events += details
                        .get("duplicate_events")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;
                    receipt.partial_committed_records = details
                        .get("committed_records")
                        .and_then(Value::as_u64)
                        .map(|count| count as usize);
                }
            }
            return Err(partial(error, &receipt));
        }
    }
    Ok(json!({"success":true,"receipt":receipt}))
}
