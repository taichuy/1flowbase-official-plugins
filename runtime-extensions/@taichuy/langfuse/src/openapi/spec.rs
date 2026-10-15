use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub(super) struct Operation {
    pub id: String,
    pub method: String,
    pub path: String,
    pub operation: Value,
    pub parameters: Vec<Value>,
}
pub(super) struct Spec {
    pub document: Value,
    pub operations: BTreeMap<String, Operation>,
}
fn pointer(reference: &str) -> Result<String> {
    let fragment = reference
        .strip_prefix('#')
        .ok_or_else(|| fail("langfuse_openapi_ref"))?;
    let mut decoded = Vec::new();
    let mut bytes = fragment.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let hex = |byte: u8| (byte as char).to_digit(16).map(|value| value as u8);
            let high = bytes
                .next()
                .and_then(hex)
                .ok_or_else(|| fail("langfuse_openapi_ref"))?;
            let low = bytes
                .next()
                .and_then(hex)
                .ok_or_else(|| fail("langfuse_openapi_ref"))?;
            decoded.push((high << 4) | low);
        } else {
            decoded.push(byte);
        }
    }
    let pointer = String::from_utf8(decoded).map_err(|_| fail("langfuse_openapi_ref"))?;
    if !pointer.starts_with('/') {
        return Err(fail("langfuse_openapi_ref"));
    }
    Ok(pointer)
}
fn pointer_tokens(reference: &str) -> Result<Vec<String>> {
    pointer(reference)?[1..]
        .split('/')
        .map(|token| {
            let mut result = String::new();
            let mut chars = token.chars();
            while let Some(c) = chars.next() {
                match c {
                    '~' => match chars.next() {
                        Some('0') => result.push('~'),
                        Some('1') => result.push('/'),
                        _ => return Err(fail("langfuse_openapi_ref")),
                    },
                    c => result.push(c),
                }
            }
            Ok(result)
        })
        .collect()
}
fn reference<'a>(document: &'a Value, value: &str) -> Result<&'a Value> {
    pointer_tokens(value)?;
    document
        .pointer(&pointer(value)?)
        .ok_or_else(|| fail("langfuse_openapi_ref"))
}
pub(super) fn resolve(document: &Value, value: &Value) -> Result<Value> {
    let mut value = value;
    let mut visited = BTreeSet::new();
    while let Some(reference_value) = value.get("$ref") {
        let name = reference_value
            .as_str()
            .ok_or_else(|| fail("langfuse_openapi_ref"))?;
        if !visited.insert(name) {
            return Err(fail("langfuse_openapi_ref_cycle"));
        }
        value = reference(document, name)?;
    }
    Ok(value.clone())
}
fn refs(value: &Value, output: &mut BTreeSet<String>) -> Result<()> {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if key == "$ref" {
                    output.insert(
                        value
                            .as_str()
                            .ok_or_else(|| fail("langfuse_openapi_ref"))?
                            .into(),
                    );
                } else {
                    refs(value, output)?;
                }
            }
        }
        Value::Array(array) => {
            for value in array {
                refs(value, output)?;
            }
        }
        _ => (),
    }
    Ok(())
}
fn parameters(document: &Value, path: &Value, operation: &Value) -> Result<Vec<Value>> {
    let mut result = BTreeMap::new();
    for source in [path, operation] {
        let Some(parameters) = source.get("parameters") else {
            continue;
        };
        let parameters = parameters
            .as_array()
            .ok_or_else(|| fail("langfuse_openapi_parameters"))?;
        let mut own = BTreeSet::new();
        for parameter in parameters {
            let parameter = resolve(document, parameter)?;
            let name = parameter
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| fail("langfuse_openapi_parameters"))?;
            let location = parameter
                .get("in")
                .and_then(Value::as_str)
                .ok_or_else(|| fail("langfuse_openapi_parameters"))?;
            if !matches!(location, "path" | "query" | "header" | "cookie") {
                return Err(fail("langfuse_openapi_parameters"));
            }
            let key = (location.to_owned(), name.to_owned());
            if !own.insert(key.clone()) {
                return Err(fail("langfuse_openapi_parameters"));
            }
            result.insert(key, parameter);
        }
    }
    Ok(result.into_values().collect())
}
impl Spec {
    pub fn parse(document: Value) -> Result<Self> {
        if !document
            .get("openapi")
            .and_then(Value::as_str)
            .is_some_and(|version| version.starts_with("3."))
        {
            return Err(fail("langfuse_openapi_document"));
        }
        let mut references = BTreeSet::new();
        refs(&document, &mut references)?;
        // Validate targets without flattening. Cyclic schema references are valid.
        for name in references {
            reference(&document, &name)?;
        }
        let paths = document
            .get("paths")
            .and_then(Value::as_object)
            .ok_or_else(|| fail("langfuse_openapi_document"))?;
        let mut operations = BTreeMap::new();
        for (path, item) in paths {
            if path.starts_with("x-") {
                continue;
            }
            if !path.starts_with('/') || path.starts_with("//") || path.contains(['?', '#', '\\']) {
                return Err(fail("langfuse_openapi_path"));
            }
            let item = resolve(&document, item)?;
            let object = item
                .as_object()
                .ok_or_else(|| fail("langfuse_openapi_document"))?;
            for method in [
                "get", "put", "post", "delete", "options", "head", "patch", "trace",
            ] {
                let Some(operation) = object.get(method) else {
                    continue;
                };
                let operation = resolve(&document, operation)?;
                let id = operation
                    .get("operationId")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| fail("langfuse_openapi_operation_id"))?
                    .to_owned();
                let operation = Operation {
                    id: id.clone(),
                    method: method.to_uppercase(),
                    path: path.clone(),
                    parameters: parameters(&document, &item, &operation)?,
                    operation,
                };
                if operations.insert(id, operation).is_some() {
                    return Err(fail("langfuse_openapi_duplicate_operation_id"));
                }
            }
        }
        Ok(Self {
            document,
            operations,
        })
    }
    pub fn get(&self, id: &str) -> Result<&Operation> {
        self.operations
            .get(id)
            .ok_or_else(|| fail("langfuse_openapi_operation_not_found"))
    }
    pub fn components(&self, operation: &Operation) -> Result<Value> {
        let mut pending = BTreeSet::new();
        refs(&operation.operation, &mut pending)?;
        refs(&json!(operation.parameters), &mut pending)?;
        let mut visited = BTreeSet::new();
        let mut components = serde_json::Map::new();
        while let Some(name) = pending.pop_first() {
            if !visited.insert(name.clone()) {
                continue;
            }
            let target = reference(&self.document, &name)?;
            refs(target, &mut pending)?;
            let tokens = pointer_tokens(&name)?;
            if tokens.len() < 3 || tokens[0] != "components" {
                return Err(fail("langfuse_openapi_ref_scope"));
            }
            let group = &tokens[1];
            let key = &tokens[2];
            let component = self
                .document
                .get("components")
                .and_then(|value| value.get(group))
                .and_then(|value| value.get(key))
                .ok_or_else(|| fail("langfuse_openapi_ref"))?;
            refs(component, &mut pending)?;
            let group = components.entry(group.clone()).or_insert_with(|| json!({}));
            group[key] = component.clone();
        }
        Ok(Value::Object(components))
    }
}
