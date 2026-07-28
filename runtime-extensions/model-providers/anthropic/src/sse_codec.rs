use anyhow::{bail, Result};

pub(super) const MAX_SSE_EVENT_BYTES: usize = 16 * 1024 * 1024;

/// Bounds the raw bytes retained for one incomplete event while the mature
/// `eventsource-stream` codec owns strict UTF-8 and SSE decoding.
pub(super) struct SseEventSizeGuard {
    bytes_in_event: usize,
    line_has_content: bool,
    pending_cr: bool,
    max_event_bytes: usize,
}

impl Default for SseEventSizeGuard {
    fn default() -> Self {
        Self::new(MAX_SSE_EVENT_BYTES)
    }
}

impl SseEventSizeGuard {
    pub(super) fn new(max_event_bytes: usize) -> Self {
        Self {
            bytes_in_event: 0,
            line_has_content: false,
            pending_cr: false,
            max_event_bytes,
        }
    }

    pub(super) fn observe(&mut self, bytes: &[u8]) -> Result<()> {
        for &byte in bytes {
            if self.pending_cr {
                self.pending_cr = false;
                if byte == b'\n' {
                    continue;
                }
            }

            self.bytes_in_event = self
                .bytes_in_event
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Anthropic SSE event byte count overflowed"))?;
            if self.bytes_in_event > self.max_event_bytes {
                bail!(
                    "Anthropic SSE event exceeded the {} byte limit",
                    self.max_event_bytes
                );
            }

            match byte {
                b'\r' => {
                    self.finish_line();
                    self.pending_cr = true;
                }
                b'\n' => self.finish_line(),
                _ => self.line_has_content = true,
            }
        }
        Ok(())
    }

    fn finish_line(&mut self) {
        if self.line_has_content {
            self.line_has_content = false;
        } else {
            self.bytes_in_event = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eventsource_stream::{EventStreamError, Eventsource};
    use futures_util::{stream, StreamExt};

    #[test]
    fn guard_resets_for_lf_crlf_cr_comments_and_event_boundaries() {
        let mut guard = SseEventSizeGuard::new(24);
        guard.observe(b"data: one\n\n").unwrap();
        guard.observe(b"data: two\r\n\r\n").unwrap();
        guard.observe(b":comment\rdata: three\r\r").unwrap();
        guard.observe(b"data: four\n\n").unwrap();
    }

    #[test]
    fn guard_rejects_an_unbounded_incomplete_event() {
        let mut guard = SseEventSizeGuard::new(8);
        let error = guard.observe(b"data: 123").unwrap_err();
        assert!(error.to_string().contains("8 byte limit"));
    }

    #[tokio::test]
    async fn codec_preserves_chinese_and_emoji_split_across_tcp_chunks() {
        let wire = "event: content_block_delta\r\ndata: {\"type\":\"content_block_delta\",\r\ndata: \"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"中文🙂\"}}\r\n\r\n";
        let bytes = wire.as_bytes();
        let emoji = wire.find('🙂').unwrap();
        let source = stream::iter(vec![
            Ok::<_, std::io::Error>(bytes[..emoji + 1].to_vec()),
            Ok(bytes[emoji + 1..emoji + 3].to_vec()),
            Ok(bytes[emoji + 3..].to_vec()),
        ]);
        let events = source.eventsource().collect::<Vec<_>>().await;
        assert_eq!(events.len(), 1);
        let event = events[0].as_ref().unwrap();
        assert_eq!(event.event, "content_block_delta");
        assert_eq!(
            event.data,
            "{\"type\":\"content_block_delta\",\n\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"中文🙂\"}}"
        );
    }

    #[tokio::test]
    async fn codec_supports_comments_and_multiline_data() {
        let source = stream::iter(vec![Ok::<_, std::io::Error>(
            b":keepalive\ndata: {\"type\":\"ping\",\ndata: \"extra\":true}\n\n".to_vec(),
        )]);
        let events = source.eventsource().collect::<Vec<_>>().await;
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].as_ref().unwrap().data,
            "{\"type\":\"ping\",\n\"extra\":true}"
        );
    }

    #[tokio::test]
    async fn codec_rejects_invalid_utf8_instead_of_replacing_it() {
        let source = stream::iter(vec![Ok::<_, std::io::Error>(vec![
            b'd', b'a', b't', b'a', b':', b' ', 0xf0, 0x9f,
        ])]);
        let events = source.eventsource().collect::<Vec<_>>().await;
        assert!(events
            .iter()
            .any(|event| matches!(event, Err(EventStreamError::Utf8(_)))));
    }
}
