//! iLink media uses AES-128-ECB + PKCS#7, with a provider-issued opaque CDN query.
//! ECB is the provider protocol (not authenticated encryption); HTTPS authenticates transport.
use super::*;
use aws_lc_rs::cipher::{DecryptingKey, DecryptionContext, UnboundCipherKey, AES_128};
use runtime_extension_sdk::{
    NotificationAttachmentDownloadOutcome, NotificationAttachmentReference,
    NotificationReceivedAttachment,
};
const CDN_BASE: &str = "https://novac2c.cdn.weixin.qq.com";
/// Bounded collection for a CDN edge which returns the whole HTTP 200 body.
/// Prefix bytes are discarded as they arrive; a suffix byte proves this is not EOF.
pub(super) struct RangeBuffer {
    start: u64,
    end_exclusive: u64,
    position: u64,
    data: Vec<u8>,
    has_suffix: bool,
}
impl RangeBuffer {
    pub(super) fn new(start: u64, end: u64) -> Self {
        Self {
            start,
            end_exclusive: end + 1,
            position: 0,
            data: Vec::new(),
            has_suffix: false,
        }
    }
    /// True means enough ciphertext has arrived and the response can be dropped.
    pub(super) fn push(&mut self, chunk: &[u8]) -> Result<bool, ProviderError> {
        let previous = self.position;
        self.position = previous
            .checked_add(chunk.len() as u64)
            .ok_or(ProviderError::Malformed)?;
        let first = self.start.max(previous);
        let last = self.end_exclusive.min(self.position);
        if last > first {
            self.data
                .extend_from_slice(&chunk[(first - previous) as usize..(last - previous) as usize]);
        }
        self.has_suffix = self.position > self.end_exclusive;
        Ok(self.has_suffix)
    }
    pub(super) fn finish(self) -> Result<(Vec<u8>, bool), ProviderError> {
        if self.position <= self.start {
            return Err(ProviderError::Malformed);
        }
        Ok((self.data, !self.has_suffix))
    }
}

pub(super) fn trusted_cdn_base(raw: &str) -> Result<reqwest::Url, ProviderError> {
    let url = trusted_base(raw)?;
    if url.host_str() != Some("novac2c.cdn.weixin.qq.com") {
        return Err(ProviderError::UnsafeUrl);
    }
    Ok(url)
}
fn locator(media: &Value) -> Result<(String, Vec<(String, String)>), ProviderError> {
    if let Some(query) = media["encrypt_query_param"]
        .as_str()
        .filter(|v| !v.is_empty())
    {
        return Ok((
            CDN_BASE.into(),
            vec![("encrypted_query_param".into(), query.into())],
        ));
    }
    let url = reqwest::Url::parse(media["full_url"].as_str().ok_or(ProviderError::Malformed)?)
        .map_err(|_| ProviderError::UnsafeUrl)?;
    trusted_cdn_base(url.origin().ascii_serialization().as_str())?;
    if url.path() != "/c2c/download"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(ProviderError::UnsafeUrl);
    }
    let query: Vec<_> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if query.len() != 1 || query[0].0 != "encrypted_query_param" || query[0].1.is_empty() {
        return Err(ProviderError::UnsafeUrl);
    }
    Ok((CDN_BASE.into(), query))
}
fn aes_key(media: &Value) -> Result<Vec<u8>, ProviderError> {
    let raw = STANDARD
        .decode(media["aes_key"].as_str().ok_or(ProviderError::Malformed)?)
        .map_err(|_| ProviderError::Malformed)?;
    if raw.len() == 16 {
        return Ok(raw);
    }
    if raw.len() == 32 {
        return decode_hex(&raw);
    }
    Err(ProviderError::Malformed)
}
fn decode_hex(raw: &[u8]) -> Result<Vec<u8>, ProviderError> {
    if raw.len() != 32 {
        return Err(ProviderError::Malformed);
    }
    raw.chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).map_err(|_| ProviderError::Malformed)?;
            u8::from_str_radix(pair, 16).map_err(|_| ProviderError::Malformed)
        })
        .collect()
}
pub(super) fn attachment(
    item: &Value,
    message_id: &str,
    index: usize,
) -> Result<Option<NotificationReceivedAttachment>, ProviderError> {
    let (field, default_name, media_type) = match item["type"].as_u64() {
        Some(2) => ("image_item", "image.jpg", "image/jpeg"),
        Some(3) => ("voice_item", "voice.silk", "audio/silk"),
        Some(4) => ("file_item", "document.bin", "application/octet-stream"),
        Some(5) => ("video_item", "video.mp4", "video/mp4"),
        _ => return Ok(None),
    };
    let payload = &item[field];
    let mut media = payload["media"].clone();
    if field == "image_item" {
        if let Some(hex) = payload["aeskey"].as_str() {
            media["aes_key"] = json!(STANDARD.encode(decode_hex(hex.as_bytes())?));
        }
    }
    locator(&media)?;
    aes_key(&media)?;
    let attachment_id = format!("{message_id}:{index}");
    let file_name = payload["file_name"]
        .as_str()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(default_name)
        .to_string();
    let size = payload.get("len").and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|v| v.parse().ok()))
    });
    Ok(Some(NotificationReceivedAttachment {
        provider_attachment_id: attachment_id.clone(),
        file_name,
        media_type: media_type.into(),
        size_bytes: size,
        reference: NotificationAttachmentReference {
            provider_message_id: message_id.into(),
            provider_attachment_id: attachment_id,
            download_token: json!({"media":media,"size_bytes":size}),
        },
    }))
}
fn decrypt(mut bytes: Vec<u8>, key: &[u8], eof: bool) -> Result<Vec<u8>, ProviderError> {
    if bytes.is_empty() || bytes.len() % 16 != 0 {
        return Err(ProviderError::Malformed);
    }
    let key = UnboundCipherKey::new(&AES_128, key)
        .and_then(DecryptingKey::ecb)
        .map_err(|_| ProviderError::Malformed)?;
    key.decrypt(&mut bytes, DecryptionContext::None)
        .map_err(|_| ProviderError::Malformed)?;
    if eof {
        let pad = *bytes.last().ok_or(ProviderError::Malformed)? as usize;
        if !(1..=16).contains(&pad)
            || !bytes[bytes.len() - pad..]
                .iter()
                .all(|v| *v as usize == pad)
        {
            return Err(ProviderError::Malformed);
        }
        bytes.truncate(bytes.len() - pad);
    }
    Ok(bytes)
}
pub(super) async fn download(
    client: &impl ILink,
    secrets: &Secrets,
    attachment: NotificationAttachmentReference,
    offset: u64,
    max_bytes: u32,
) -> Result<NotificationRuntimeResult, ProviderError> {
    if !has_account(secrets) {
        return Ok(NotificationRuntimeResult::DownloadAttachment {
            outcome: NotificationAttachmentDownloadOutcome::Failed {
                failure_code: if status(secrets) == NotificationAuthorizationStatus::Invalid {
                    NotificationFailureCode::AuthorizationInvalid
                } else {
                    NotificationFailureCode::AuthorizationRequired
                },
            },
        });
    }
    if max_bytes == 0 || max_bytes > 48 * 1024 {
        return Err(ProviderError::Malformed);
    }
    attachment
        .validate()
        .map_err(|_| ProviderError::Malformed)?;
    let media = &attachment.download_token["media"];
    let key = aes_key(media)?;
    let (base_url, query) = locator(media)?;
    let start = offset / 16 * 16;
    // One lookahead block identifies the final PKCS#7 block without mistaking padding
    // for file bytes at a chunk boundary. Arbitrary offsets remain byte exact.
    let end = offset
        .checked_add(u64::from(max_bytes))
        .and_then(|v| v.checked_add(31))
        .map(|v| v / 16 * 16 - 1)
        .ok_or(ProviderError::Malformed)?;
    let response = client
        .request(Request {
            base_url,
            endpoint: "c2c/download",
            query,
            token: None,
            body: None,
            timeout: Duration::from_secs(60),
            byte_range: Some((start, end)),
        })
        .await?;
    let bytes = STANDARD
        .decode(response["data"].as_str().ok_or(ProviderError::Malformed)?)
        .map_err(|_| ProviderError::Malformed)?;
    if bytes.len() as u64 > end - start + 1 {
        return Err(ProviderError::Malformed);
    }
    let remote_eof = response["eof"].as_bool().ok_or(ProviderError::Malformed)?;
    let bytes = decrypt(bytes, &key, remote_eof)?;
    if remote_eof
        && attachment.download_token["size_bytes"]
            .as_u64()
            .is_some_and(|size| size != start + bytes.len() as u64)
    {
        return Err(ProviderError::Malformed);
    }
    let skip = (offset - start) as usize;
    if skip > bytes.len() {
        return Err(ProviderError::Malformed);
    }
    let available = &bytes[skip..];
    let count = available.len().min(max_bytes as usize);
    let eof = remote_eof && count == available.len();
    if count == 0 && !eof {
        return Err(ProviderError::Malformed);
    }
    Ok(NotificationRuntimeResult::DownloadAttachment {
        outcome: NotificationAttachmentDownloadOutcome::Succeeded {
            offset,
            data_base64: STANDARD.encode(&available[..count]),
            eof,
        },
    })
}

#[cfg(test)]
#[path = "_tests/media.rs"]
mod tests;
