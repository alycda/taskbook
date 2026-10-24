//! MCP tool definitions and dispatch into Taskbook operations.

use serde_json::{json, Value};

use crate::taskbook::Taskbook;
use taskbook_common::{board, due, StorageItem};

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
                    "priority": { "type": "integer", "minimum": 1, "maximum": 3, "description": "1 = normal, 2 = medium, 3 = high (default 1)" },
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
                    "priority": { "type": "integer", "minimum": 1, "maximum": 3, "description": "New priority (tasks only)" },
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
        v["priority"] = json!(task.priority);
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

fn arg_ids(args: &Value, key: &str) -> Result<Vec<u64>, String> {
    let ids: Vec<u64> = args
        .get(key)
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default();
    if ids.is_empty() {
        Err(format!("'{key}' must be a non-empty array of item ids"))
    } else {
        Ok(ids)
    }
}

fn parse_due_arg(raw: &str) -> Result<Option<i64>, String> {
    if raw.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    due::parse_due_date(raw).map(Some).ok_or_else(|| {
        format!("invalid due date '{raw}' (use YYYY-MM-DD, today, tomorrow or none)")
    })
}

fn parse_priority_arg(args: &Value) -> Result<Option<u8>, String> {
    match args.get("priority") {
        None => Ok(None),
        Some(v) => match v.as_u64() {
            Some(p @ 1..=3) => Ok(Some(p as u8)),
            _ => Err("'priority' must be 1, 2 or 3".to_string()),
        },
    }
}

fn list_items(taskbook: &Taskbook, args: &Value) -> Result<Value, String> {
    let archived = args
        .get("archived")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let data = if archived {
        taskbook.get_all_archive_items()
    } else {
        taskbook.get_all_items()
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
    let boards = taskbook.get_all_boards().map_err(|e| e.to_string())?;
    Ok(json!(boards))
}

fn create_task(taskbook: &Taskbook, args: &Value) -> Result<Value, String> {
    let description = arg_str(args, "description")?.trim().to_string();
    if description.is_empty() {
        return Err("'description' must not be empty".to_string());
    }
    let mut boards: Vec<String> = opt_string_vec(args, "boards")
        .iter()
        .map(|b| board::normalize_board_name(b))
        .collect();
    if boards.is_empty() {
        boards.push(board::DEFAULT_BOARD.to_string());
    }
    let priority = parse_priority_arg(args)?.unwrap_or(1);
    let tags: Vec<String> = opt_string_vec(args, "tags")
        .iter()
        .map(|t| board::normalize_tag(t))
        .filter(|t| !t.is_empty())
        .collect();
    let due_date = match args.get("due_date").and_then(Value::as_str) {
        Some(raw) => parse_due_arg(raw)?,
        None => None,
    };

    let id = taskbook
        .create_task_direct_with_tags(boards, description, priority, tags, due_date)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "id": id, "type": "task" }))
}

fn create_note(taskbook: &Taskbook, args: &Value) -> Result<Value, String> {
    let description = arg_str(args, "description")?.trim().to_string();
    if description.is_empty() {
        return Err("'description' must not be empty".to_string());
    }
    let mut boards: Vec<String> = opt_string_vec(args, "boards")
        .iter()
        .map(|b| board::normalize_board_name(b))
        .collect();
    if boards.is_empty() {
        boards.push(board::DEFAULT_BOARD.to_string());
    }
    let tags: Vec<String> = opt_string_vec(args, "tags")
        .iter()
        .map(|t| board::normalize_tag(t))
        .filter(|t| !t.is_empty())
        .collect();

    let id = taskbook
        .create_note_direct_with_tags(boards, description, tags)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "id": id, "type": "note" }))
}

fn set_task_state(taskbook: &Taskbook, args: &Value) -> Result<Value, String> {
    let id = arg_u64(args, "id")?;
    let state = arg_str(args, "state")?;
    taskbook
        .set_task_state_silent(id, state)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "id": id, "state": state }))
}

fn edit_item(taskbook: &Taskbook, args: &Value) -> Result<Value, String> {
    let id = arg_u64(args, "id")?;
    let mut changed: Vec<&str> = Vec::new();

    if let Some(desc) = args.get("description").and_then(Value::as_str) {
        let desc = desc.trim();
        if desc.is_empty() {
            return Err("'description' must not be empty".to_string());
        }
        taskbook
            .edit_description_silent(id, desc)
            .map_err(|e| e.to_string())?;
        changed.push("description");
    }
    if let Some(priority) = parse_priority_arg(args)? {
        taskbook
            .update_priority_silent(id, priority)
            .map_err(|e| e.to_string())?;
        changed.push("priority");
    }
    if let Some(raw) = args.get("due_date").and_then(Value::as_str) {
        let due_date = parse_due_arg(raw)?;
        taskbook
            .set_due_date_silent(id, due_date)
            .map_err(|e| e.to_string())?;
        changed.push("due_date");
    }
    if args.get("boards").is_some() {
        let boards = opt_string_vec(args, "boards");
        if boards.is_empty() {
            return Err("'boards' must be a non-empty array of board names".to_string());
        }
        taskbook
            .move_boards_silent(id, boards)
            .map_err(|e| e.to_string())?;
        changed.push("boards");
    }
    if args.get("tags").is_some() {
        let new_tags: Vec<String> = opt_string_vec(args, "tags")
            .iter()
            .map(|t| board::normalize_tag(t))
            .filter(|t| !t.is_empty())
            .collect();
        let data = taskbook.get_all_items().map_err(|e| e.to_string())?;
        let current: Vec<String> = data
            .get(&id)
            .map(|item| item.tags().to_vec())
            .unwrap_or_default();
        taskbook
            .update_tags_silent(id, &new_tags, &current)
            .map_err(|e| e.to_string())?;
        changed.push("tags");
    }
    if let Some(starred) = args.get("starred").and_then(Value::as_bool) {
        taskbook
            .set_starred_silent(id, starred)
            .map_err(|e| e.to_string())?;
        changed.push("starred");
    }

    if changed.is_empty() {
        return Err(
            "no editable fields provided (description, priority, due_date, boards, tags, starred)"
                .to_string(),
        );
    }
    Ok(json!({ "id": id, "updated": changed }))
}

fn delete_items(taskbook: &Taskbook, args: &Value) -> Result<Value, String> {
    let ids = arg_ids(args, "ids")?;
    taskbook
        .delete_items_silent(&ids)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "deleted": ids }))
}

fn restore_items(taskbook: &Taskbook, args: &Value) -> Result<Value, String> {
    let ids = arg_ids(args, "ids")?;
    taskbook
        .restore_items_silent(&ids)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "restored": ids }))
}
