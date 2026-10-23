//! MCP tool definitions and dispatch into Taskbook operations.

use serde_json::{json, Value};

use crate::taskbook::{ItemEdit, NewNote, NewTask, TaskState, Taskbook};
use taskbook_common::{board, due, Priority, StorageItem};

/// JSON Schema definitions for all exposed tools.
pub fn definitions() -> Value {
    json!([
        {
            "name": "list_items",
            "description": "List taskbook items (tasks and notes). Optionally filter by state/type, board, tag, or a text query, or list archived items instead.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "archived": { "type": "boolean", "description": "List archived (deleted) items instead of active ones" },
                    "filter": { "type": "string", "enum": ["pending", "in_progress", "done", "task", "note", "starred"], "description": "Only return items matching this state or type" },
                    "board": { "type": "string", "description": "Only return items on this board" },
                    "tag": { "type": "string", "description": "Only return items with this tag" },
                    "query": { "type": "string", "description": "Case-insensitive search in descriptions and note bodies" }
                }
            }
        },
        {
            "name": "list_boards",
            "description": "List all board names.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "create_task",
            "description": "Create a new task.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "description": { "type": "string", "description": "Task description" },
                    "boards": { "type": "array", "items": { "type": "string" }, "description": "Boards to place the task on (defaults to My Board)" },
                    "priority": { "type": "integer", "minimum": 1, "maximum": 3, "description": "1 = high, 2 = medium, 3 = normal (default 3)" },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Tags for the task" },
                    "due_date": { "type": "string", "description": "Due date: YYYY-MM-DD, 'YYYY-MM-DD HH:MM', today, tomorrow, now, or today+HHMM / tomorrow+HHMM / now+HHMM" }
                },
                "required": ["description"]
            }
        },
        {
            "name": "create_note",
            "description": "Create a new note.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "description": { "type": "string", "description": "Note text" },
                    "boards": { "type": "array", "items": { "type": "string" }, "description": "Boards to place the note on (defaults to My Board)" },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Tags for the note" }
                },
                "required": ["description"]
            }
        },
        {
            "name": "set_task_state",
            "description": "Set a task's state to pending, in_progress, or done.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Task id" },
                    "state": { "type": "string", "enum": ["pending", "in_progress", "done"] }
                },
                "required": ["id", "state"]
            }
        },
        {
            "name": "edit_item",
            "description": "Update fields of an existing item. Only the provided fields change. Tags replace the existing set.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Item id" },
                    "description": { "type": "string", "description": "New description" },
                    "priority": { "type": "integer", "minimum": 1, "maximum": 3, "description": "New priority: 1 = high, 2 = medium, 3 = normal (tasks only)" },
                    "due_date": { "type": "string", "description": "New due date (YYYY-MM-DD, 'YYYY-MM-DD HH:MM', today, tomorrow, now, today+HHMM / tomorrow+HHMM / now+HHMM) or 'none' to clear (tasks only)" },
                    "boards": { "type": "array", "items": { "type": "string" }, "description": "Replace the item's boards" },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Replace the item's tags" },
                    "starred": { "type": "boolean", "description": "Star or unstar the item" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "delete_items",
            "description": "Delete items (moves them to the archive).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "ids": { "type": "array", "items": { "type": "integer" }, "description": "Item ids to delete" }
                },
                "required": ["ids"]
            }
        },
        {
            "name": "restore_items",
            "description": "Restore archived items back to the active list.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "ids": { "type": "array", "items": { "type": "integer" }, "description": "Archive ids to restore" }
                },
                "required": ["ids"]
            }
        }
    ])
}

/// Dispatch a tools/call request. Protocol-level failures (unknown tool,
/// malformed params) return Err; tool runtime failures return an
/// `isError: true` result so the calling agent can react.
pub fn call(taskbook: &Taskbook, params: &Value) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((-32602, "missing tool name".to_string()))?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let outcome = match name {
        "list_items" => list_items(taskbook, &args),
        "list_boards" => list_boards(taskbook),
        "create_task" => create_task(taskbook, &args),
        "create_note" => create_note(taskbook, &args),
        "set_task_state" => set_task_state(taskbook, &args),
        "edit_item" => edit_item(taskbook, &args),
        "delete_items" => delete_items(taskbook, &args),
        "restore_items" => restore_items(taskbook, &args),
        other => return Err((-32602, format!("unknown tool: {other}"))),
    };

    Ok(match outcome {
        Ok(value) => json!({
            "content": [{ "type": "text", "text": value.to_string() }],
            "isError": false
        }),
        Err(message) => json!({
            "content": [{ "type": "text", "text": message }],
            "isError": true
        }),
    })
}

fn item_to_json(item: &StorageItem) -> Value {
    let mut v = json!({
        "id": item.id(),
        "type": if item.is_task() { "task" } else { "note" },
        "description": item.description(),
        "boards": item.boards(),
        "tags": item.tags(),
        "starred": item.is_starred(),
        "created": item.date(),
    });
    if let Some(task) = item.as_task() {
        let state = if task.is_complete {
            "done"
        } else if task.in_progress {
            "in_progress"
        } else {
            "pending"
        };
        v["state"] = json!(state);
        v["priority"] = json!(task.priority.level());
        if let Some(millis) = task.due_date {
            v["due_date"] = json!(due::format_due_date(millis));
        }
    }
    v
}

fn arg_u64(args: &Value, key: &str) -> Result<u64, String> {
    args.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing or invalid '{key}'"))
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing or invalid '{key}'"))
}

fn opt_string_vec(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// A non-empty array of ids. Any non-integer element is an error rather than
/// being skipped.
fn arg_ids(args: &Value, key: &str) -> Result<Vec<u64>, String> {
    let invalid = || format!("'{key}' must be a non-empty array of item ids");
    let ids: Vec<u64> = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(invalid)?
        .iter()
        .map(|v| v.as_u64().ok_or_else(invalid))
        .collect::<Result<_, _>>()?;
    if ids.is_empty() {
        return Err(invalid());
    }
    Ok(ids)
}

fn parse_due_arg(raw: &str) -> Result<Option<i64>, String> {
    if raw.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    due::parse_due_date(raw)
        .map(Some)
        .ok_or_else(|| due::invalid_due_date_message(raw))
}

fn parse_priority_arg(args: &Value) -> Result<Option<Priority>, String> {
    match args.get("priority") {
        None => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|p| u8::try_from(p).ok())
            .and_then(Priority::from_level)
            .map(Some)
            .ok_or_else(|| "'priority' must be 1 (high), 2 (medium) or 3 (normal)".to_string()),
    }
}

fn list_items(taskbook: &Taskbook, args: &Value) -> Result<Value, String> {
    let archived = args
        .get("archived")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let data = if archived {
        taskbook.archived_items()
    } else {
        taskbook.items()
    }
    .map_err(|e| e.to_string())?;

    let filter = args.get("filter").and_then(Value::as_str);
    if let Some(f) = filter {
        let valid = ["pending", "in_progress", "done", "task", "note", "starred"];
        if !valid.contains(&f) {
            return Err(format!("invalid filter '{f}' (use one of {valid:?})"));
        }
    }
    let board_filter = args.get("board").and_then(Value::as_str);
    let tag_filter = args.get("tag").and_then(Value::as_str);
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .map(str::to_lowercase);

    let mut items: Vec<&StorageItem> = data.values().collect();
    items.retain(|item| {
        if let Some(f) = filter {
            let keep = match f {
                "task" => item.is_task(),
                "note" => !item.is_task(),
                "starred" => item.is_starred(),
                "pending" => item
                    .as_task()
                    .map(|t| !t.is_complete && !t.in_progress)
                    .unwrap_or(false),
                "in_progress" => item.as_task().map(|t| t.in_progress).unwrap_or(false),
                "done" => item.as_task().map(|t| t.is_complete).unwrap_or(false),
                _ => true,
            };
            if !keep {
                return false;
            }
        }
        if let Some(b) = board_filter {
            let normalized = board::normalize_board_name(b);
            if !item
                .boards()
                .iter()
                .any(|ib| board::board_eq(ib, &normalized))
            {
                return false;
            }
        }
        if let Some(t) = tag_filter {
            let normalized = board::normalize_tag(t);
            if !item
                .tags()
                .iter()
                .any(|it| it.eq_ignore_ascii_case(&normalized))
            {
                return false;
            }
        }
        if let Some(q) = &query {
            let in_desc = item.description().to_lowercase().contains(q);
            let in_body = item
                .note_body()
                .map(|b| b.to_lowercase().contains(q))
                .unwrap_or(false);
            if !in_desc && !in_body {
                return false;
            }
        }
        true
    });
    items.sort_by_key(|item| item.id());

    Ok(Value::Array(
        items.iter().map(|item| item_to_json(item)).collect(),
    ))
}

fn list_boards(taskbook: &Taskbook) -> Result<Value, String> {
    let boards = taskbook.boards().map_err(|e| e.to_string())?;
    Ok(json!(boards))
}

fn create_task(taskbook: &Taskbook, args: &Value) -> Result<Value, String> {
    let id = taskbook
        .create_task(NewTask {
            description: arg_str(args, "description")?.to_string(),
            boards: opt_string_vec(args, "boards"),
            priority: parse_priority_arg(args)?.unwrap_or_default(),
            tags: opt_string_vec(args, "tags"),
            due_date: match args.get("due_date").and_then(Value::as_str) {
                Some(raw) => parse_due_arg(raw)?,
                None => None,
            },
        })
        .map_err(|e| e.to_string())?;
    Ok(json!({ "id": id, "type": "task" }))
}

fn create_note(taskbook: &Taskbook, args: &Value) -> Result<Value, String> {
    let id = taskbook
        .create_note(NewNote {
            title: arg_str(args, "description")?.to_string(),
            boards: opt_string_vec(args, "boards"),
            body: None,
            tags: opt_string_vec(args, "tags"),
        })
        .map_err(|e| e.to_string())?;
    Ok(json!({ "id": id, "type": "note" }))
}

fn set_task_state(taskbook: &Taskbook, args: &Value) -> Result<Value, String> {
    let id = arg_u64(args, "id")?;
    let state = arg_str(args, "state")?;
    let parsed: TaskState = state
        .parse()
        .map_err(|e: crate::error::TaskbookError| e.to_string())?;
    taskbook
        .set_task_state(id, parsed)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "id": id, "state": state }))
}

/// Validates every field before applying any, and applies them in one save.
fn edit_item(taskbook: &Taskbook, args: &Value) -> Result<Value, String> {
    let id = arg_u64(args, "id")?;
    let edit = ItemEdit {
        description: args
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string),
        priority: parse_priority_arg(args)?,
        due_date: args
            .get("due_date")
            .and_then(Value::as_str)
            .map(parse_due_arg)
            .transpose()?,
        boards: args.get("boards").map(|_| opt_string_vec(args, "boards")),
        tags: args.get("tags").map(|_| opt_string_vec(args, "tags")),
        starred: args.get("starred").and_then(Value::as_bool),
    };
    let changed = edit.fields();
    if changed.is_empty() {
        return Err(
            "no editable fields provided (description, priority, due_date, boards, tags, starred)"
                .to_string(),
        );
    }
    taskbook.edit(id, edit).map_err(|e| e.to_string())?;
    Ok(json!({ "id": id, "updated": changed }))
}

fn delete_items(taskbook: &Taskbook, args: &Value) -> Result<Value, String> {
    let ids = arg_ids(args, "ids")?;
    let deleted = taskbook.delete(&ids).map_err(|e| e.to_string())?;
    Ok(json!({ "deleted": deleted }))
}

fn restore_items(taskbook: &Taskbook, args: &Value) -> Result<Value, String> {
    let ids = arg_ids(args, "ids")?;
    let restored = taskbook.restore(&ids).map_err(|e| e.to_string())?;
    Ok(json!({ "restored": restored }))
}
