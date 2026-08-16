use serde_json::{Map, Value};

const DSML_TOOL_CALLS_OPEN: &str = "<｜DSML｜tool_calls>";
const DSML_TOOL_CALLS_CLOSE: &str = "</｜DSML｜tool_calls>";
const DSML_INVOKE_OPEN: &str = "<｜DSML｜invoke";
const DSML_INVOKE_CLOSE: &str = "</｜DSML｜invoke>";
const DSML_PARAMETER_OPEN: &str = "<｜DSML｜parameter";
const DSML_PARAMETER_CLOSE: &str = "</｜DSML｜parameter>";
const DSML_ENVELOPE_PREFIX: &str = "\n\n<｜DSML｜tool_calls>";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DsmlToolCall {
    pub(crate) name: String,
    pub(crate) arguments: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DsmlParsingOutcome {
    Parsed,
    NoMatchPassthrough,
    InvalidProtocol,
    StructuredToolCallsPrecedence,
}

impl DsmlParsingOutcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Parsed => "parsed",
            Self::NoMatchPassthrough => "no_match_passthrough",
            Self::InvalidProtocol => "invalid_protocol",
            Self::StructuredToolCallsPrecedence => "structured_tool_calls_precedence",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DsmlStreamResolution {
    pub(crate) trailing_text: String,
    pub(crate) tool_calls: Vec<DsmlToolCall>,
    pub(crate) outcome: DsmlParsingOutcome,
    pub(crate) protocol_failure: Option<DsmlProtocolFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DsmlProtocolFailure {
    pub(crate) error_code: &'static str,
    pub(crate) candidate: String,
}

#[derive(Debug, Default)]
pub(crate) struct DsmlStreamDecoder {
    pending: String,
    candidate: Option<String>,
}

impl DsmlStreamDecoder {
    pub(crate) fn push(&mut self, delta: &str) -> Option<String> {
        if let Some(candidate) = self.candidate.as_mut() {
            candidate.push_str(delta);
            return None;
        }

        self.pending.push_str(delta);
        if let Some(marker_start) = find_dsml_marker_start(&self.pending) {
            let candidate_start = if self.pending[..marker_start].ends_with("\n\n") {
                marker_start - 2
            } else {
                marker_start
            };
            let candidate = self.pending.split_off(candidate_start);
            self.candidate = Some(candidate);
            return take_non_empty(&mut self.pending);
        }

        let retained = longest_candidate_prefix_suffix(&self.pending);
        let retained_start = self.pending.len() - retained;
        let suffix = self.pending.split_off(retained_start);
        let visible = std::mem::replace(&mut self.pending, suffix);
        (!visible.is_empty()).then_some(visible)
    }

    pub(crate) fn finish(self, structured_tool_calls_present: bool) -> DsmlStreamResolution {
        let Some(candidate) = self.candidate else {
            return DsmlStreamResolution {
                trailing_text: self.pending,
                tool_calls: Vec::new(),
                outcome: DsmlParsingOutcome::NoMatchPassthrough,
                protocol_failure: None,
            };
        };

        if structured_tool_calls_present {
            return DsmlStreamResolution {
                trailing_text: candidate,
                tool_calls: Vec::new(),
                outcome: DsmlParsingOutcome::StructuredToolCallsPrecedence,
                protocol_failure: None,
            };
        }

        match parse_complete_envelope(&candidate) {
            Some((tool_calls, trailing_text)) => DsmlStreamResolution {
                trailing_text,
                tool_calls,
                outcome: DsmlParsingOutcome::Parsed,
                protocol_failure: None,
            },
            None => DsmlStreamResolution {
                trailing_text: String::new(),
                tool_calls: Vec::new(),
                outcome: DsmlParsingOutcome::InvalidProtocol,
                protocol_failure: Some(DsmlProtocolFailure {
                    error_code: classify_invalid_candidate(&candidate),
                    candidate,
                }),
            },
        }
    }
}

fn take_non_empty(value: &mut String) -> Option<String> {
    (!value.is_empty()).then(|| std::mem::take(value))
}

fn longest_candidate_prefix_suffix(text: &str) -> usize {
    let exact_prefix = [DSML_ENVELOPE_PREFIX, DSML_TOOL_CALLS_OPEN]
        .into_iter()
        .flat_map(|prefix| {
            prefix
                .char_indices()
                .map(|(index, _)| index)
                .chain(std::iter::once(prefix.len()))
                .filter(|length| *length > 0)
                .filter(move |length| {
                    *length <= text.len()
                        && text.is_char_boundary(text.len() - *length)
                        && text.ends_with(&prefix[..*length])
                })
        })
        .max()
        .unwrap_or(0);
    let incomplete_marker = text
        .rfind('<')
        .filter(|start| !text[*start..].contains('>'))
        .map(|start| text.len() - start)
        .unwrap_or(0);
    exact_prefix.max(incomplete_marker)
}

fn find_dsml_marker_start(text: &str) -> Option<usize> {
    text.match_indices('<').find_map(|(start, _)| {
        let marker = &text[start..];
        let end = marker.find('>')?;
        marker[..=end]
            .to_ascii_uppercase()
            .contains("DSML")
            .then_some(start)
    })
}

fn classify_invalid_candidate(candidate: &str) -> &'static str {
    let envelope = candidate.strip_prefix("\n\n").unwrap_or(candidate);
    if !envelope.starts_with(DSML_TOOL_CALLS_OPEN) {
        "invalid_marker"
    } else if envelope.matches(DSML_TOOL_CALLS_OPEN).count() > 1 {
        "ambiguous_envelope"
    } else if !envelope.contains(DSML_TOOL_CALLS_CLOSE) {
        "incomplete_envelope"
    } else {
        "invalid_envelope"
    }
}

fn parse_complete_envelope(candidate: &str) -> Option<(Vec<DsmlToolCall>, String)> {
    let envelope = candidate.strip_prefix("\n\n").unwrap_or(candidate);
    let mut cursor = Cursor::new(envelope);
    cursor.consume(DSML_TOOL_CALLS_OPEN)?;
    cursor.consume("\n")?;

    let mut tool_calls = Vec::new();
    loop {
        if cursor.remaining().starts_with(DSML_TOOL_CALLS_CLOSE) {
            cursor.consume(DSML_TOOL_CALLS_CLOSE)?;
            if tool_calls.is_empty() || cursor.remaining().contains(DSML_TOOL_CALLS_OPEN) {
                return None;
            }
            return Some((tool_calls, cursor.remaining().to_string()));
        }

        cursor.consume(DSML_INVOKE_OPEN)?;
        cursor.consume(" name=\"")?;
        let name = cursor.take_until("\">\n")?;
        if name.is_empty() || name.contains('"') {
            return None;
        }

        let mut arguments = Map::new();
        while cursor.remaining().starts_with(DSML_PARAMETER_OPEN) {
            cursor.consume(DSML_PARAMETER_OPEN)?;
            cursor.consume(" name=\"")?;
            let parameter_name = cursor.take_until("\" string=\"")?;
            if parameter_name.is_empty() || parameter_name.contains('"') {
                return None;
            }
            let string_mode = cursor.take_until("\">")?;
            let raw_value = cursor.take_until(DSML_PARAMETER_CLOSE)?;
            let value = match string_mode.as_str() {
                "true" => Value::String(raw_value),
                "false" => serde_json::from_str(&raw_value).ok()?,
                _ => return None,
            };
            if arguments.insert(parameter_name, value).is_some() {
                return None;
            }
            cursor.consume("\n")?;
        }

        if arguments.is_empty() && cursor.remaining().starts_with('\n') {
            cursor.consume("\n")?;
        }
        cursor.consume(DSML_INVOKE_CLOSE)?;
        cursor.consume("\n")?;
        tool_calls.push(DsmlToolCall {
            name,
            arguments: Value::Object(arguments),
        });
    }
}

struct Cursor<'a> {
    remaining: &'a str,
}

impl<'a> Cursor<'a> {
    fn new(value: &'a str) -> Self {
        Self { remaining: value }
    }

    fn remaining(&self) -> &'a str {
        self.remaining
    }

    fn consume(&mut self, expected: &str) -> Option<()> {
        self.remaining = self.remaining.strip_prefix(expected)?;
        Some(())
    }

    fn take_until(&mut self, delimiter: &str) -> Option<String> {
        let index = self.remaining.find(delimiter)?;
        let value = self.remaining[..index].to_string();
        self.remaining = &self.remaining[index + delimiter.len()..];
        Some(value)
    }
}
