//! Parses the lines of `claude -p --output-format stream-json --verbose` into typed events.
//!
//! The event shape follows what Claude Code prints: a `system` init line, `assistant` messages whose
//! content blocks are text, thinking or tool calls, `user` messages that carry tool results, and a final
//! `result` line. diffity reads the same facts from ACP updates (MIT, see THIRD_PARTY_NOTICES).

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct ResultEvent {
    pub is_error: bool,
    pub subtype: String,
    pub result: Option<String>,
    pub structured_output: Option<Value>,
    pub num_turns: Option<u32>,
    pub duration_ms: Option<u64>,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Init {
        session_id: String,
        model: String,
        tools: Vec<String>,
        mcp_servers: Vec<(String, String)>,
    },
    Text {
        message_id: String,
        text: String,
    },
    Thinking {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        text: String,
        is_error: bool,
    },
    Result(ResultEvent),
}

/// Blank lines, lines that are not JSON, and event types this build does not use give no events.
pub fn parse_line(line: &str) -> Vec<StreamEvent> {
    let line = line.trim();
    if line.is_empty() {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    match value.get("type").and_then(Value::as_str) {
        Some("system") => init(&value).into_iter().collect(),
        Some("assistant") => assistant(&value),
        Some("user") => tool_results(&value),
        Some("result") => vec![StreamEvent::Result(result(&value))],
        _ => Vec::new(),
    }
}

fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn init(value: &Value) -> Option<StreamEvent> {
    if value.get("subtype").and_then(Value::as_str) != Some("init") {
        return None;
    }
    let names = |key: &str| -> Vec<String> {
        value
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mcp_servers = value
        .get("mcp_servers")
        .and_then(Value::as_array)
        .map(|servers| {
            servers
                .iter()
                .map(|server| (text(server, "name"), text(server, "status")))
                .collect()
        })
        .unwrap_or_default();
    Some(StreamEvent::Init {
        session_id: text(value, "session_id"),
        model: text(value, "model"),
        tools: names("tools"),
        mcp_servers,
    })
}

fn assistant(value: &Value) -> Vec<StreamEvent> {
    let Some(message) = value.get("message") else {
        return Vec::new();
    };
    let message_id = text(message, "id");
    let Some(blocks) = message.get("content").and_then(Value::as_array) else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter_map(|block| match block.get("type").and_then(Value::as_str)? {
            "text" => Some(StreamEvent::Text {
                message_id: message_id.clone(),
                text: text(block, "text"),
            }),
            "thinking" => Some(StreamEvent::Thinking {
                text: text(block, "thinking"),
            }),
            "tool_use" => Some(StreamEvent::ToolUse {
                id: text(block, "id"),
                name: text(block, "name"),
                input: block.get("input").cloned().unwrap_or(Value::Null),
            }),
            _ => None,
        })
        .collect()
}

fn tool_results(value: &Value) -> Vec<StreamEvent> {
    let Some(blocks) = value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
        .map(|block| StreamEvent::ToolResult {
            tool_use_id: text(block, "tool_use_id"),
            text: content_text(block.get("content")),
            is_error: block.get("is_error").and_then(Value::as_bool).unwrap_or(false),
        })
        .collect()
}

fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn result(value: &Value) -> ResultEvent {
    let subtype = text(value, "subtype");
    let is_error = value.get("is_error").and_then(Value::as_bool).unwrap_or(false) || subtype.starts_with("error");
    ResultEvent {
        is_error,
        subtype,
        result: value.get("result").and_then(Value::as_str).map(String::from),
        structured_output: value
            .get("structured_output")
            .filter(|output| !output.is_null())
            .cloned(),
        num_turns: value
            .get("num_turns")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
        duration_ms: value.get("duration_ms").and_then(Value::as_u64),
        cost_usd: value.get("total_cost_usd").and_then(Value::as_f64),
    }
}

/// Cuts a byte stream into lines. A multi-byte character is never split, because `\n` cannot occur inside one.
#[derive(Debug, Default)]
pub struct LineBuffer {
    pending: Vec<u8>,
}

impl LineBuffer {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.pending.extend_from_slice(bytes);
        let mut lines = Vec::new();
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            lines.push(String::from_utf8_lossy(&line[..end]).into_owned());
        }
        lines
    }

    /// The last line, when the stream ended without a newline.
    pub fn finish(&mut self) -> Option<String> {
        if self.pending.is_empty() {
            return None;
        }
        let line = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INIT: &str = r#"{"type":"system","subtype":"init","cwd":"/r","session_id":"s-1","tools":["Read","Edit","mcp__diffpilot__list_threads"],"mcp_servers":[{"name":"diffpilot","status":"connected"}],"model":"claude-sonnet-5-5","permissionMode":"acceptEdits"}"#;

    #[test]
    fn reads_the_init_line() {
        assert_eq!(
            parse_line(INIT),
            [StreamEvent::Init {
                session_id: "s-1".into(),
                model: "claude-sonnet-5-5".into(),
                tools: vec!["Read".into(), "Edit".into(), "mcp__diffpilot__list_threads".into()],
                mcp_servers: vec![("diffpilot".into(), "connected".into())],
            }]
        );
    }

    #[test]
    fn other_system_lines_are_ignored() {
        assert!(parse_line(r#"{"type":"system","subtype":"hook_started","hook_name":"x"}"#).is_empty());
    }

    #[test]
    fn splits_an_assistant_message_into_its_blocks() {
        let line = r#"{"type":"assistant","message":{"id":"msg_1","role":"assistant","content":[
            {"type":"thinking","thinking":"Plan the edit","signature":"x"},
            {"type":"text","text":"I will rename it."},
            {"type":"tool_use","id":"toolu_1","name":"Edit","input":{"file_path":"/r/a.ts","old_string":"x","new_string":"count"}}
        ]},"session_id":"s-1"}"#
            .replace('\n', "");
        let events = parse_line(&line);
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0],
            StreamEvent::Thinking {
                text: "Plan the edit".into()
            }
        );
        assert_eq!(
            events[1],
            StreamEvent::Text {
                message_id: "msg_1".into(),
                text: "I will rename it.".into()
            }
        );
        let StreamEvent::ToolUse { id, name, input } = &events[2] else {
            panic!("{:?}", events[2]);
        };
        assert_eq!((id.as_str(), name.as_str()), ("toolu_1", "Edit"));
        assert_eq!(input["new_string"], "count");
    }

    #[test]
    fn reads_tool_results_as_text_or_parts() {
        let line = r#"{"type":"user","message":{"role":"user","content":[
            {"type":"tool_result","tool_use_id":"t1","content":"ok"},
            {"type":"tool_result","tool_use_id":"t2","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}],"is_error":true}
        ]}}"#
            .replace('\n', "");
        assert_eq!(
            parse_line(&line),
            [
                StreamEvent::ToolResult {
                    tool_use_id: "t1".into(),
                    text: "ok".into(),
                    is_error: false
                },
                StreamEvent::ToolResult {
                    tool_use_id: "t2".into(),
                    text: "a\nb".into(),
                    is_error: true
                },
            ]
        );
    }

    #[test]
    fn a_user_message_that_is_plain_text_gives_nothing() {
        assert!(parse_line(r#"{"type":"user","message":{"role":"user","content":"hello"}}"#).is_empty());
    }

    #[test]
    fn reads_a_success_result_with_structured_output() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":1200,"num_turns":3,"result":"Done.","total_cost_usd":0.04,"structured_output":{"summary":"ok","findings":[]}}"#;
        let events = parse_line(line);
        let [StreamEvent::Result(result)] = events.as_slice() else {
            panic!("no result");
        };
        assert!(!result.is_error);
        assert_eq!(result.result.as_deref(), Some("Done."));
        assert_eq!(result.num_turns, Some(3));
        assert_eq!(result.duration_ms, Some(1200));
        assert_eq!(result.structured_output.as_ref().unwrap()["summary"], "ok");
    }

    #[test]
    fn an_error_subtype_is_an_error_even_without_the_flag() {
        let events = parse_line(r#"{"type":"result","subtype":"error_max_turns"}"#);
        let [StreamEvent::Result(result)] = events.as_slice() else {
            panic!("no result");
        };
        assert!(result.is_error);
    }

    #[test]
    fn noise_gives_nothing() {
        for line in ["", "   ", "not json", "{}", r#"{"type":"stream_event"}"#, "[1]"] {
            assert!(parse_line(line).is_empty(), "{line}");
        }
    }

    #[test]
    fn the_line_buffer_joins_chunks_and_keeps_characters_whole() {
        let mut buffer = LineBuffer::default();
        let bytes = "a é\nb".as_bytes();
        assert!(buffer.push(&bytes[..3]).is_empty());
        assert_eq!(buffer.push(&bytes[3..]), ["a é"]);
        assert_eq!(buffer.push(b"c\nd\n"), ["bc", "d"]);
        assert_eq!(buffer.finish(), None);
        buffer.push(b"tail");
        assert_eq!(buffer.finish().as_deref(), Some("tail"));
    }
}
