use super::*;
mod encoding;
mod spec;
mod transport;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Discover {
    instance_id: String,
    search: Option<String>,
    tag: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Get {
    instance_id: String,
    #[serde(rename = "operationId")]
    operation_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Execute {
    instance_id: String,
    #[serde(rename = "operationId")]
    operation_id: String,
    #[serde(default)]
    path: serde_json::Map<String, Value>,
    #[serde(default)]
    query: serde_json::Map<String, Value>,
    #[serde(default, deserialize_with = "present_body")]
    body: Option<Value>,
}
fn present_body<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}
fn input<T: serde::de::DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body).map_err(|_| fail("langfuse_openapi_input"))
}
async fn load(host: &Host, id: &str) -> Result<(transport::Transport, spec::Spec)> {
    let transport = transport::Transport::new(host, id).await?;
    let spec = spec::Spec::parse(transport.document().await?)?;
    Ok((transport, spec))
}
pub(crate) async fn execute(host: &Host, handler: &str, body: Value) -> Result<Value> {
    match handler {
        "openapi_discover" => {
            let request: Discover = input(body)?;
            let (_, spec) = load(host, &request.instance_id).await?;
            let search = request.search.map(|search| search.to_lowercase());
            let mut operations = Vec::new();
            for operation in spec.operations.values() {
                if request.tag.as_ref().is_some_and(|tag| {
                    !operation
                        .operation
                        .get("tags")
                        .and_then(Value::as_array)
                        .is_some_and(|tags| tags.iter().any(|value| value.as_str() == Some(tag)))
                }) {
                    continue;
                }
                let mut dto = json!({"operationId":operation.id,"method":operation.method,"path":operation.path});
                for key in ["summary", "description", "tags"] {
                    if let Some(value) = operation.operation.get(key) {
                        dto[key] = value.clone();
                    }
                }
                if search
                    .as_ref()
                    .is_some_and(|search| !dto.to_string().to_lowercase().contains(search))
                {
                    continue;
                }
                operations.push(dto);
            }
            Ok(json!({"success":true,"instance_id":request.instance_id,"operations":operations}))
        }
        "openapi_get" => {
            let request: Get = input(body)?;
            let (_, spec) = load(host, &request.instance_id).await?;
            match request.operation_id {
                None => Ok(
                    json!({"success":true,"instance_id":request.instance_id,"document":spec.document}),
                ),
                Some(id) => {
                    let operation = spec.get(&id)?;
                    Ok(
                        json!({"success":true,"instance_id":request.instance_id,"operationId":id,"method":operation.method,"path":operation.path,"operation":operation.operation,"parameters":operation.parameters,"components":spec.components(operation)?}),
                    )
                }
            }
        }
        "openapi_execute" => {
            let request: Execute = input(body)?;
            let (transport, spec) = load(host, &request.instance_id).await?;
            let operation = spec.get(&request.operation_id)?;
            let url = encoding::url(
                &transport.base,
                &spec,
                operation,
                &request.path,
                &request.query,
            )?;
            let response = transport.send(&operation.method, url, request.body).await?;
            Ok(
                json!({"success":true,"instance_id":request.instance_id,"operationId":request.operation_id,"response":response}),
            )
        }
        _ => Err(fail("langfuse_operation")),
    }
}
