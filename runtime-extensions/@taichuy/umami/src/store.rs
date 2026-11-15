use crate::{remote::Context, Failure, Result};
use runtime_extension_sdk::{
    MultiplexHostService, PluginDataFilter, PluginDataFilterOperator, PluginDataOperation,
    PluginDataOperationResult, PluginDataOrder, PluginDataOrderDirection, PluginDataPage,
    PluginDataRequest, PluginDataResponse, PluginDataTarget, PluginDataValue,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const FIELDS: [&str; 6] = [
    "host_id",
    "name",
    "base_url",
    "username",
    "enabled",
    "credential_id",
];

#[derive(Clone)]
pub(crate) struct Host {
    pub host_id: String,
    pub name: String,
    pub base_url: String,
    pub username: String,
    pub enabled: bool,
    pub credential_id: String,
}
impl Host {
    pub fn summary(&self) -> Value {
        json!({"host_id":self.host_id,"name":self.name,"base_url":self.base_url,
            "username":self.username,"enabled":self.enabled,"has_password":!self.credential_id.is_empty()})
    }
    pub fn token_key(&self) -> String {
        format!("umami-token:{}:{}", self.host_id, self.credential_id)
    }
    fn values(&self) -> BTreeMap<String, PluginDataValue> {
        let mut values: BTreeMap<String, PluginDataValue> = [
            ("host_id", &self.host_id),
            ("name", &self.name),
            ("base_url", &self.base_url),
            ("username", &self.username),
            ("credential_id", &self.credential_id),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), PluginDataValue::String(v.clone())))
        .collect();
        values.insert("enabled".into(), PluginDataValue::Boolean(self.enabled));
        values
    }
    fn from_values(values: BTreeMap<String, PluginDataValue>) -> Result<Self> {
        let string = |key: &str| match values.get(key) {
            Some(PluginDataValue::String(v)) | Some(PluginDataValue::Uuid(v)) => Ok(v.clone()),
            _ => Err(Failure("umami_storage_error")),
        };
        Ok(Self {
            host_id: string("host_id")?,
            name: string("name")?,
            base_url: string("base_url")?,
            username: string("username")?,
            credential_id: string("credential_id")?,
            enabled: match values.get("enabled") {
                Some(PluginDataValue::Boolean(v)) => *v,
                _ => return Err(Failure("umami_storage_error")),
            },
        })
    }
}
fn target() -> PluginDataTarget {
    PluginDataTarget::OwnedCollection {
        collection_code: "umami_hosts".into(),
    }
}
fn filters(id: &str) -> Vec<PluginDataFilter> {
    vec![PluginDataFilter {
        field: "host_id".into(),
        operator: PluginDataFilterOperator::Equal,
        value: Some(PluginDataValue::String(id.into())),
    }]
}
pub(crate) async fn execute(
    ctx: &Context,
    operation: PluginDataOperation,
) -> Result<PluginDataOperationResult> {
    let request = PluginDataRequest {
        idempotency_key: None,
        operations: vec![operation],
    };
    request
        .validate()
        .map_err(|_| Failure("umami_storage_error"))?;
    let value = serde_json::to_value(request).map_err(|_| Failure("umami_storage_error"))?;
    let response = ctx
        .emitter
        .callback(MultiplexHostService::PluginDataV1, value)
        .await
        .map_err(|_| Failure("umami_storage_error"))?;
    let response: PluginDataResponse =
        serde_json::from_value(response.get("result").cloned().unwrap_or(Value::Null))
            .map_err(|_| Failure("umami_storage_error"))?;
    response
        .results
        .into_iter()
        .next()
        .ok_or(Failure("umami_storage_error"))
}
pub(crate) async fn get(ctx: &Context, id: &str) -> Result<Host> {
    match execute(
        ctx,
        PluginDataOperation::FindOne {
            target: target(),
            fields: FIELDS[..6].iter().map(|s| s.to_string()).collect(),
            filters: filters(id),
        },
    )
    .await?
    {
        PluginDataOperationResult::OptionalRow { row: Some(row) } => Host::from_values(row.values),
        PluginDataOperationResult::OptionalRow { row: None } => Err(Failure("host_not_found")),
        _ => Err(Failure("umami_storage_error")),
    }
}
pub(crate) async fn all(ctx: &Context) -> Result<Vec<Host>> {
    let mut result = Vec::new();
    loop {
        match execute(
            ctx,
            PluginDataOperation::Find {
                target: target(),
                fields: FIELDS[..6].iter().map(|s| s.to_string()).collect(),
                filters: vec![],
                order: vec![PluginDataOrder {
                    field: "host_id".into(),
                    direction: PluginDataOrderDirection::Ascending,
                }],
                page: PluginDataPage {
                    limit: 100,
                    offset: result.len() as u64,
                },
            },
        )
        .await?
        {
            PluginDataOperationResult::Rows { rows } => {
                let count = rows.len();
                for row in rows {
                    result.push(Host::from_values(row.values)?);
                }
                if count < 100 {
                    return Ok(result);
                }
            }
            _ => return Err(Failure("umami_storage_error")),
        }
    }
}
pub(crate) async fn save(ctx: &Context, host: &Host, update: bool) -> Result<()> {
    let operation = if update {
        PluginDataOperation::Update {
            target: target(),
            filters: filters(&host.host_id),
            values: host.values(),
        }
    } else {
        PluginDataOperation::Insert {
            target: target(),
            values: host.values(),
        }
    };
    match execute(ctx, operation).await? {
        PluginDataOperationResult::Mutation { affected: 1 } => Ok(()),
        _ => Err(Failure("umami_storage_error")),
    }
}
pub(crate) async fn delete(ctx: &Context, id: &str) -> Result<()> {
    match execute(
        ctx,
        PluginDataOperation::Delete {
            target: target(),
            filters: filters(id),
        },
    )
    .await?
    {
        PluginDataOperationResult::Mutation { affected: 1 } => Ok(()),
        _ => Err(Failure("umami_storage_error")),
    }
}
