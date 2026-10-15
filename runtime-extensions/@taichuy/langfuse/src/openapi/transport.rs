use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::{HttpEgressClientError, HttpEgressRequest};
use std::collections::BTreeMap;

pub(super) struct Transport {
    client: HttpEgressClient,
    pub base: url::Url,
    authorization: String,
}
impl Transport {
    pub async fn new(host: &Host, instance_id: &str) -> Result<Self> {
        let connection = config::connection(host, instance_id).await?;
        Ok(Self {
            client: HttpEgressClient::new(host.emitter.clone()),
            base: connection.base,
            authorization: connection.authorization,
        })
    }
    pub async fn send(&self, method: &str, url: url::Url, body: Option<Value>) -> Result<Value> {
        let mut headers = BTreeMap::from([
            ("authorization".into(), self.authorization.clone()),
            ("accept".into(), "application/json".into()),
        ]);
        let body_base64 = match body {
            Some(body) => {
                headers.insert("content-type".into(), "application/json".into());
                Some(
                    STANDARD
                        .encode(serde_json::to_vec(&body).map_err(|_| fail("langfuse_request"))?),
                )
            }
            None => None,
        };
        let response = self
            .client
            .execute(HttpEgressRequest {
                method: method.into(),
                url: url.to_string(),
                headers,
                body_base64,
            })
            .await
            .map_err(|error| match error {
                HttpEgressClientError::Host(error) => Failure {
                    code: error.code,
                    status: error.status,
                    details: error.details,
                },
                HttpEgressClientError::Transport(
                    runtime_extension_sdk::MultiplexError::FrameTooLarge,
                ) => fail("host_http_frame_too_large"),
                _ => fail("host_http_transport"),
            })?;
        let bytes = STANDARD
            .decode(&response.body_base64)
            .map_err(|_| fail("host_http_response"))?;
        let mut result = json!({"status":response.status,"headers":response.headers,"body_base64":response.body_base64});
        if let Ok(body) = serde_json::from_slice::<Value>(&bytes) {
            result["body"] = body;
        }
        Ok(result)
    }
    pub async fn document(&self) -> Result<Value> {
        let response = self
            .send(
                "GET",
                self.base
                    .join("generated/api/openapi.yml")
                    .map_err(|_| fail("langfuse_base_url"))?,
                None,
            )
            .await?;
        let status = response["status"]
            .as_u64()
            .ok_or_else(|| fail("host_http_response"))? as u16;
        if !(200..300).contains(&status) {
            return Err(Failure {
                code: "langfuse_openapi_http".into(),
                status: Some(status),
                details: Some(json!({"response":response})),
            });
        }
        let bytes = STANDARD
            .decode(
                response["body_base64"]
                    .as_str()
                    .ok_or_else(|| fail("host_http_response"))?,
            )
            .map_err(|_| fail("host_http_response"))?;
        serde_yaml::from_slice(&bytes).map_err(|_| fail("langfuse_openapi_document"))
    }
}
