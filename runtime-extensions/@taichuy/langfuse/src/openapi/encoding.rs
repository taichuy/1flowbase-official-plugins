use super::*;
use std::collections::{BTreeMap, BTreeSet};
use url::Url;

fn scalar(value: &Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Bool(_) | Value::Number(_) => Ok(value.to_string()),
        _ => Err(fail("langfuse_openapi_parameter_encoding")),
    }
}
fn supported(parameter: &Value, style: &str) -> Result<()> {
    if parameter
        .get("style")
        .and_then(Value::as_str)
        .unwrap_or(style)
        != style
        || parameter.get("allowReserved").and_then(Value::as_bool) == Some(true)
        || parameter.get("content").is_some()
    {
        return Err(fail("langfuse_openapi_parameter_encoding"));
    }
    Ok(())
}
pub(super) fn url(
    base: &Url,
    spec: &spec::Spec,
    operation: &spec::Operation,
    path: &serde_json::Map<String, Value>,
    query: &serde_json::Map<String, Value>,
) -> Result<Url> {
    let parameters = operation
        .parameters
        .iter()
        .map(|parameter| {
            (
                (
                    parameter["in"].as_str().unwrap(),
                    parameter["name"].as_str().unwrap(),
                ),
                parameter,
            )
        })
        .collect::<BTreeMap<_, _>>();
    if path
        .keys()
        .any(|key| !parameters.contains_key(&("path", key.as_str())))
        || query
            .keys()
            .any(|key| !parameters.contains_key(&("query", key.as_str())))
    {
        return Err(fail("langfuse_openapi_parameter_unknown"));
    }
    for ((location, name), parameter) in &parameters {
        if parameter.get("required").and_then(Value::as_bool) == Some(true)
            && match *location {
                "path" => !path.contains_key(*name),
                "query" => !query.contains_key(*name),
                _ => false,
            }
        {
            return Err(fail("langfuse_openapi_parameter_required"));
        }
    }
    let mut url = base.clone();
    let mut used = BTreeSet::new();
    {
        let mut encoded_path = base
            .path()
            .strip_suffix('/')
            .unwrap_or(base.path())
            .to_owned();
        for template in operation
            .path
            .strip_prefix('/')
            .ok_or_else(|| fail("langfuse_openapi_path"))?
            .split('/')
        {
            let mut segment = String::new();
            let mut remaining = template;
            while let Some(start) = remaining.find('{') {
                segment.push_str(&remaining[..start]);
                let end = remaining[start + 1..]
                    .find('}')
                    .ok_or_else(|| fail("langfuse_openapi_path"))?
                    + start
                    + 1;
                let name = &remaining[start + 1..end];
                let parameter = parameters
                    .get(&("path", name))
                    .ok_or_else(|| fail("langfuse_openapi_parameters"))?;
                supported(parameter, "simple")?;
                let schema = spec::resolve(&spec.document, &parameter["schema"])?;
                if matches!(
                    schema.get("type").and_then(Value::as_str),
                    Some("array" | "object")
                ) {
                    return Err(fail("langfuse_openapi_parameter_encoding"));
                }
                let value = scalar(
                    path.get(name)
                        .ok_or_else(|| fail("langfuse_openapi_parameter_required"))?,
                )?;
                if matches!(value.as_str(), "." | "..") {
                    return Err(fail("langfuse_openapi_path"));
                }
                segment.push_str(&value);
                used.insert(name.to_owned());
                remaining = &remaining[end + 1..];
            }
            if remaining.contains('}') {
                return Err(fail("langfuse_openapi_path"));
            }
            segment.push_str(remaining);
            if matches!(segment.as_str(), "." | "..") {
                return Err(fail("langfuse_openapi_path"));
            }
            // URL's segment setter discards TAB/CR/LF. Use its byte serializer
            // instead, adapting form spaces to path spaces, then set the already
            // escaped path. Literal '+' is serialized as %2B, so it is preserved.
            let encoded = url::form_urlencoded::byte_serialize(segment.as_bytes())
                .collect::<String>()
                .replace('+', "%20");
            encoded_path.push('/');
            encoded_path.push_str(&encoded);
        }
        url.set_path(&encoded_path);
    }
    if path.keys().any(|key| !used.contains(key)) {
        return Err(fail("langfuse_openapi_parameter_unknown"));
    }
    for (name, value) in query {
        let parameter = parameters[&("query", name.as_str())];
        supported(parameter, "form")?;
        match value {
            Value::Array(values) => {
                let values = values.iter().map(scalar).collect::<Result<Vec<_>>>()?;
                let explode = parameter
                    .get("explode")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                if explode {
                    for value in values {
                        url.query_pairs_mut().append_pair(name, &value);
                    }
                } else {
                    url.query_pairs_mut().append_pair(name, &values.join(","));
                }
            }
            value => {
                url.query_pairs_mut().append_pair(name, &scalar(value)?);
            }
        }
    }
    Ok(url)
}
