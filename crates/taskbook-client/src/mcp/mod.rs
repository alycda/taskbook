//! MCP (Model Context Protocol) stdio server.
//!
//! Speaks newline-delimited JSON-RPC 2.0 on stdin/stdout so taskbook can be
//! used as an MCP server from agents like Claude Code (`tb --mcp`). Uses the
//! configured storage backend — the remote server storage when sync is
//! enabled. Only JSON-RPC goes to stdout; diagnostics go to stderr.

mod tools;

use std::io::{BufRead, Write};
use std::path::Path;

use serde_json::{json, Value};

use crate::error::Result;
use crate::taskbook::Taskbook;

const SERVER_NAME: &str = "taskbook";
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";

/// Run the MCP server over stdin/stdout using the configured storage backend.
pub fn run(taskbook_dir: Option<&Path>) -> Result<()> {
    let taskbook = Taskbook::new(taskbook_dir)?;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(&taskbook, stdin.lock(), stdout.lock())
}

/// Serve MCP requests from `reader`, writing responses to `writer`.
fn serve(taskbook: &Taskbook, reader: impl BufRead, mut writer: impl Write) -> Result<()> {
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_message(taskbook, &line) {
            writeln!(writer, "{response}")?;
            writer.flush()?;
        }
    }
    Ok(())
}

/// Handle one JSON-RPC message. Returns `None` for notifications.
fn handle_message(taskbook: &Taskbook, line: &str) -> Option<String> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(error_response(
                Value::Null,
                -32700,
                &format!("parse error: {e}"),
            ))
        }
    };

    let method = msg
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let id = match msg.get("id") {
        Some(id) if !id.is_null() => id.clone(),
        _ => return None, // notification — no response
    };

    let result = match method.as_str() {
        "initialize" => Ok(initialize_result(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::definitions() })),
        "tools/call" => tools::call(taskbook, &params),
        other => Err((-32601, format!("method not found: {other}"))),
    };

    Some(match result {
        Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }).to_string(),
        Err((code, message)) => error_response(id, code, &message),
    })
}

fn initialize_result(params: &Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(LATEST_PROTOCOL_VERSION);
    let version = if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested) {
        requested
    } else {
        LATEST_PROTOCOL_VERSION
    };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": env!("CARGO_PKG_VERSION"),
        }
    })
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::LocalStorage;

    fn test_taskbook() -> Taskbook {
        let dir = std::env::temp_dir().join(format!("tb-mcp-test-{}", uuid::Uuid::new_v4()));
        let storage = LocalStorage::new(&dir).unwrap();
        Taskbook::with_storage(Box::new(storage))
    }

    fn request(taskbook: &Taskbook, msg: Value) -> Value {
        let response = handle_message(taskbook, &msg.to_string()).expect("expected a response");
        serde_json::from_str(&response).unwrap()
    }

    /// Extract the JSON payload from a tools/call text content result.
    fn tool_payload(response: &Value) -> Value {
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn initialize_negotiates_protocol_version() {
        let tb = test_taskbook();
        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}),
        );
        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(resp["result"]["serverInfo"]["name"], "taskbook");
        assert!(resp["result"]["capabilities"]["tools"].is_object());

        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}),
        );
        assert_eq!(resp["result"]["protocolVersion"], LATEST_PROTOCOL_VERSION);
    }

    #[test]
    fn tools_list_returns_all_tools() {
        let tb = test_taskbook();
        let resp = request(&tb, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}));
        let tools = resp["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "list_items",
                "list_boards",
                "create_task",
                "create_note",
                "set_task_state",
                "edit_item",
                "delete_items",
                "restore_items",
            ]
        );
        for tool in tools {
            assert!(tool["description"].is_string());
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn notification_gets_no_response() {
        let tb = test_taskbook();
        let msg = json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string();
        assert!(handle_message(&tb, &msg).is_none());
    }

    #[test]
    fn unknown_method_returns_error() {
        let tb = test_taskbook();
        let resp = request(&tb, json!({"jsonrpc":"2.0","id":1,"method":"bogus/method"}));
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[test]
    fn parse_error_returns_jsonrpc_error() {
        let tb = test_taskbook();
        let response = handle_message(&tb, "not json at all").unwrap();
        let resp: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["error"]["code"], -32700);
        assert!(resp["id"].is_null());
    }

    #[test]
    fn unknown_tool_returns_invalid_params() {
        let tb = test_taskbook();
        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"bogus_tool","arguments":{}}}),
        );
        assert_eq!(resp["error"]["code"], -32602);
    }

    #[test]
    fn create_task_and_list_round_trip() {
        let tb = test_taskbook();
        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name":"create_task",
                "arguments":{
                    "description":"Pay rent",
                    "boards":["finance"],
                    "priority":2,
                    "tags":["money"],
                    "due_date":"2026-07-01"
                }
            }}),
        );
        assert_eq!(resp["result"]["isError"], false);
        let created = tool_payload(&resp);
        let id = created["id"].as_u64().unwrap();

        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name":"list_items","arguments":{"board":"finance"}
            }}),
        );
        let items = tool_payload(&resp);
        let items = items.as_array().unwrap();
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item["id"].as_u64().unwrap(), id);
        assert_eq!(item["description"], "Pay rent");
        assert_eq!(item["state"], "pending");
        assert_eq!(item["priority"], 2);
        assert_eq!(item["tags"], json!(["money"]));
        assert_eq!(item["due_date"], "2026-07-01");
    }

    #[test]
    fn list_items_returns_items_in_id_order() {
        let tb = test_taskbook();
        for n in 1..=20 {
            request(
                &tb,
                json!({"jsonrpc":"2.0","id":n,"method":"tools/call","params":{
                    "name":"create_task","arguments":{"description":format!("task {n}")}
                }}),
            );
        }

        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":21,"method":"tools/call","params":{
                "name":"list_items","arguments":{}
            }}),
        );
        let ids: Vec<u64> = tool_payload(&resp)
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_u64().unwrap())
            .collect();
        assert_eq!(ids, (1..=20).collect::<Vec<_>>());
    }

    #[test]
    fn set_task_state_and_filters() {
        let tb = test_taskbook();
        request(
            &tb,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name":"create_task","arguments":{"description":"Work on it"}
            }}),
        );
        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name":"set_task_state","arguments":{"id":1,"state":"in_progress"}
            }}),
        );
        assert_eq!(resp["result"]["isError"], false);

        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
                "name":"list_items","arguments":{"filter":"in_progress"}
            }}),
        );
        let items = tool_payload(&resp);
        assert_eq!(items.as_array().unwrap().len(), 1);

        // Invalid state is a tool error, not a protocol error
        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
                "name":"set_task_state","arguments":{"id":1,"state":"finished"}
            }}),
        );
        assert_eq!(resp["result"]["isError"], true);
    }

    #[test]
    fn edit_delete_restore_flow() {
        let tb = test_taskbook();
        request(
            &tb,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name":"create_task","arguments":{"description":"Original"}
            }}),
        );

        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name":"edit_item","arguments":{
                    "id":1,"description":"Updated","priority":3,
                    "due_date":"tomorrow","starred":true,"tags":["urgent"]
                }
            }}),
        );
        assert_eq!(resp["result"]["isError"], false);

        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
                "name":"list_items","arguments":{}
            }}),
        );
        let items = tool_payload(&resp);
        let item = &items.as_array().unwrap()[0];
        assert_eq!(item["description"], "Updated");
        assert_eq!(item["priority"], 3);
        assert_eq!(item["starred"], true);
        assert_eq!(item["tags"], json!(["urgent"]));
        assert!(item["due_date"].is_string());

        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
                "name":"delete_items","arguments":{"ids":[1]}
            }}),
        );
        assert_eq!(resp["result"]["isError"], false);

        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{
                "name":"list_items","arguments":{"archived":true}
            }}),
        );
        let archived = tool_payload(&resp);
        assert_eq!(archived.as_array().unwrap().len(), 1);

        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{
                "name":"restore_items","arguments":{"ids":[1]}
            }}),
        );
        assert_eq!(resp["result"]["isError"], false);

        let resp = request(
            &tb,
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{
                "name":"list_items","arguments":{}
            }}),
        );
        let items = tool_payload(&resp);
        assert_eq!(items.as_array().unwrap().len(), 1);
    }

    #[test]
    fn serve_loop_processes_lines_and_skips_notifications() {
        let tb = test_taskbook();
        let input = format!(
            "{}\n{}\n{}\n",
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        );
        let mut output: Vec<u8> = Vec::new();
        serve(&tb, input.as_bytes(), &mut output).unwrap();

        let lines: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["id"], 1);
        assert_eq!(lines[1]["id"], 2);
        assert!(lines[1]["result"]["tools"].is_array());
    }
}
