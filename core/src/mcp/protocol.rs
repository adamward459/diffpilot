//! The MCP side of `diffpilot-mcp`: JSON-RPC over stdio, as much of it as a tools-only server needs.
//! `forward` runs one tool in the app and returns its result or an error message.

use super::tools::{self, ToolDef};
use serde_json::{Value, json};

pub const LIST_TOOLS: &str = "__list_tools";

const VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Tools for the code review open in diffpilot: read the diff, read the review threads, leave, \
reply to, resolve or dismiss review comments, and write the reading guide of the change.";

pub type Forward<'a> = &'a dyn Fn(&str, Value) -> Result<Value, String>;

fn reply(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn failure(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn render(value: Value) -> String {
    match value {
        Value::String(text) => text,
        other => serde_json::to_string_pretty(&other).unwrap_or_else(|_| other.to_string()),
    }
}

fn describe(tool: &ToolDef) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "inputSchema": tool.input_schema,
        "annotations": { "readOnlyHint": tool.read_only },
    })
}

/// The response to one request, or None for a notification.
pub fn handle(request: &Value, forward: Forward) -> Option<Value> {
    let id = request.get("id")?;
    let method = request.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let version = VERSIONS.iter().find(|known| **known == asked).unwrap_or(&VERSIONS[1]);
            reply(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "diffpilot", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => reply(id, json!({})),
        "tools/list" => {
            let allowed: Option<Vec<String>> = forward(LIST_TOOLS, Value::Null)
                .ok()
                .and_then(|names| serde_json::from_value(names).ok());
            let tools: Vec<Value> = tools::definitions()
                .iter()
                .filter(|tool| {
                    allowed
                        .as_ref()
                        .is_none_or(|names| names.iter().any(|name| name == tool.name))
                })
                .map(describe)
                .collect();
            reply(id, json!({ "tools": tools }))
        }
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(failure(id, -32602, "tools/call needs a tool name."));
            };
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
            let (text, is_error) = match forward(name, arguments) {
                Ok(result) => (render(result), false),
                Err(message) => (message, true),
            };
            reply(
                id,
                json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }),
            )
        }
        other => failure(id, -32601, &format!("Method not found: {other}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpc(method: &str, params: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })
    }

    fn echo(tool: &str, args: Value) -> Result<Value, String> {
        match tool {
            LIST_TOOLS => Ok(json!(["list_threads", "reply"])),
            "fail" => Err("nope".into()),
            other => Ok(json!({ "tool": other, "args": args })),
        }
    }

    #[test]
    fn initialize_echoes_a_known_version_and_names_the_server() {
        let answer = handle(&rpc("initialize", json!({ "protocolVersion": "2025-03-26" })), &echo).unwrap();
        assert_eq!(answer["id"], 1);
        assert_eq!(answer["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(answer["result"]["serverInfo"]["name"], "diffpilot");
        assert!(answer["result"]["capabilities"]["tools"].is_object());
        let unknown = handle(&rpc("initialize", json!({ "protocolVersion": "1999-01-01" })), &echo).unwrap();
        assert_eq!(unknown["result"]["protocolVersion"], "2025-06-18");
    }

    #[test]
    fn notifications_get_no_answer() {
        let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert_eq!(handle(&notification, &echo), None);
    }

    #[test]
    fn the_tool_list_is_what_the_app_allows() {
        let answer = handle(&rpc("tools/list", Value::Null), &echo).unwrap();
        let names: Vec<&str> = answer["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["list_threads", "reply"]);
        let first = &answer["result"]["tools"][0];
        assert_eq!(first["annotations"]["readOnlyHint"], true);
        assert_eq!(first["inputSchema"]["type"], "object");
    }

    #[test]
    fn the_full_list_shows_when_the_app_cannot_be_asked() {
        let offline = |_: &str, _: Value| Err("down".to_string());
        let answer = handle(&rpc("tools/list", Value::Null), &offline).unwrap();
        assert_eq!(
            answer["result"]["tools"].as_array().unwrap().len(),
            tools::definitions().len()
        );
    }

    #[test]
    fn a_call_returns_text_content_and_marks_errors() {
        let ok = handle(
            &rpc("tools/call", json!({ "name": "reply", "arguments": { "body": "x" } })),
            &echo,
        )
        .unwrap();
        assert_eq!(ok["result"]["isError"], false);
        let text = ok["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("\"tool\": \"reply\""), "{text}");
        let bad = handle(&rpc("tools/call", json!({ "name": "fail" })), &echo).unwrap();
        assert_eq!(bad["result"]["isError"], true);
        assert_eq!(bad["result"]["content"][0]["text"], "nope");
        let nameless = handle(&rpc("tools/call", json!({})), &echo).unwrap();
        assert_eq!(nameless["error"]["code"], -32602);
    }

    #[test]
    fn unknown_methods_are_errors_and_ping_works() {
        assert_eq!(
            handle(&rpc("nope", Value::Null), &echo).unwrap()["error"]["code"],
            -32601
        );
        assert_eq!(handle(&rpc("ping", Value::Null), &echo).unwrap()["result"], json!({}));
    }
}
