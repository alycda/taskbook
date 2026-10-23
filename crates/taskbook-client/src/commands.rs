use std::collections::HashMap;
use std::path::PathBuf;

use base64::Engine;
use colored::Colorize;

use crate::api_client::{ApiClient, EncryptedItemData};
use crate::auth;
use crate::cli::{Cli, Command};
use crate::config::Config;
use crate::credentials::Credentials;
use crate::directory::resolve_taskbook_directory;
use crate::editor;
use crate::error::{Result, TaskbookError};
use crate::mcp;
use crate::render::{Render, Stats};
use crate::storage::{LocalStorage, StorageBackend};
use crate::taskbook::{Items, NewNote, NewTask, Taskbook};
use crate::tui;
use taskbook_common::encryption::encrypt_item;
use taskbook_common::{board, due, id, Priority, StorageItem};

/// Execute a parsed command line.
pub fn run(cli: Cli) -> Result<()> {
    let dir = cli.taskbook_dir;
    let Some(command) = cli.command else {
        return tui::run(dir.as_deref());
    };

    match command {
        Command::Register {
            server,
            username,
            email,
            password,
        } => auth::register(
            server.as_deref(),
            username.as_deref(),
            email.as_deref(),
            password.as_deref(),
        ),
        Command::Login {
            server,
            username,
            password,
            key,
        } => auth::login(
            server.as_deref(),
            username.as_deref(),
            password.as_deref(),
            key.as_deref(),
        ),
        Command::Logout => auth::logout(),
        Command::Status => auth::status(),
        Command::Migrate => migrate(dir),
        Command::Mcp => mcp::run(dir.as_deref()),
        command => {
            let config = Config::load_or_default();
            let taskbook = Taskbook::from_config(&config, dir.as_deref())?;
            let render = Render::new(config);
            run_item_command(&taskbook, &render, command)
        }
    }
}

fn run_item_command(tb: &Taskbook, render: &Render, command: Command) -> Result<()> {
    match command {
        Command::Board => show_boards(render, &tb.items()?, None),
        Command::Timeline => {
            let data = tb.items()?;
            render.display_by_date(&group_by_date(&data));
            render.display_stats(&Stats::from_items(data.values()));
        }
        Command::Archive => render.display_by_date(&group_by_date(&tb.archived_items()?)),
        Command::Task { input } => {
            let parsed = board::parse_cli_input(&input)?;
            let id = tb.create_task(NewTask {
                boards: parsed.boards,
                description: parsed.description,
                priority: parsed.priority.unwrap_or_default(),
                tags: parsed.tags,
                due_date: parsed.due_date,
            })?;
            render.success_create(id, true);
        }
        Command::Note { input } if input.is_empty() => match editor::create_note_in_editor()? {
            Some(content) => {
                let id = tb.create_note(NewNote {
                    title: content.title,
                    body: content.body,
                    ..NewNote::default()
                })?;
                render.success_create(id, false);
            }
            None => render.note_cancelled(),
        },
        Command::Note { input } => {
            let parsed = board::parse_cli_input(&input)?;
            if parsed.priority.is_some() {
                return Err(TaskbookError::NoteField("priority"));
            }
            if parsed.due_date.is_some() {
                return Err(TaskbookError::NoteField("due date"));
            }
            let id = tb.create_note(NewNote {
                boards: parsed.boards,
                title: parsed.description,
                body: None,
                tags: parsed.tags,
            })?;
            render.success_create(id, false);
        }
        Command::Check { ids } => {
            let out = tb.toggle_check(&ids)?;
            render.mark_complete(&out.on);
            render.mark_incomplete(&out.off);
        }
        Command::Begin { ids } => {
            let out = tb.toggle_begin(&ids)?;
            render.mark_started(&out.on);
            render.mark_paused(&out.off);
        }
        Command::Star { ids } => {
            let out = tb.toggle_star(&ids)?;
            render.mark_starred(&out.on);
            render.mark_unstarred(&out.off);
        }
        Command::Delete { ids } => render.success_delete(&tb.delete(&ids)?),
        Command::Restore { ids } => render.success_restore(&tb.restore(&ids)?),
        Command::Copy { ids } => render.success_copy_to_clipboard(&tb.copy_to_clipboard(&ids)?),
        Command::Edit { id, description } => {
            tb.edit_description(id, description.join(" "))?;
            render.success_edit(id);
        }
        Command::EditNote { id } => {
            let (title, body) = tb.note_content(id)?;
            match editor::edit_existing_note_in_editor(&title, body.as_deref())? {
                Some(content) => {
                    tb.set_note_content(id, content.title, content.body)?;
                    render.success_edit(id);
                }
                None => render.note_cancelled(),
            }
        }
        Command::Move { args } => {
            let (ids, boards) = id::split_leading_ids(&args)?;
            let boards = tb.move_to_boards(&ids, boards.to_vec())?;
            render.success_move(&ids, &boards);
        }
        Command::Priority { args } => {
            let (level, ids) = match args.split_last() {
                Some((level, ids)) if !ids.is_empty() => (level, ids),
                _ => return Err(TaskbookError::Usage("tb priority <ID>... <LEVEL>")),
            };
            let priority: Priority = level.parse()?;
            let ids = id::parse_ids(ids)?;
            tb.set_priority(&ids, priority)?;
            render.success_priority(&ids, priority);
        }
        Command::Due { args } => {
            let (ids, value) = id::split_leading_ids(&args)?;
            let due_date = parse_due_value(&value.join(" "))?;
            tb.set_due_date(&ids, due_date)?;
            render.success_due_date(&ids, due_date);
        }
        Command::Tag { args } => {
            let (ids, tokens) = id::split_leading_ids(&args)?;
            let (mut add, mut remove) = (Vec::new(), Vec::new());
            for token in tokens {
                if let Some(tag) = token.strip_prefix('+').filter(|t| !t.is_empty()) {
                    add.push(tag.to_string());
                } else if let Some(tag) = token.strip_prefix('-').filter(|t| !t.is_empty()) {
                    remove.push(tag.to_string());
                } else {
                    return Err(TaskbookError::InvalidTag(token.clone()));
                }
            }
            let (added, removed) = tb.update_tags(&ids, add, remove)?;
            render.success_tag(&ids, &added, &removed);
        }
        Command::Find { terms } => {
            let mut data = tb.items()?;
            data.retain(|_, item| item_matches_terms(item, &terms));
            show_boards(render, &data, None);
        }
        Command::List { filters } => {
            let mut data = tb.items()?;
            let known_boards = Taskbook::boards_of(&data);
            let boards = apply_list_filters(&mut data, &filters, &known_boards)?;
            show_boards(render, &data, boards);
        }
        Command::Clear => {
            let ids = tb.clear_completed()?;
            if ids.is_empty() {
                render.nothing_to_clear();
            } else {
                render.success_clear(&ids);
            }
        }
        Command::Register { .. }
        | Command::Login { .. }
        | Command::Logout
        | Command::Status
        | Command::Migrate
        | Command::Mcp => unreachable!("handled in run"),
    }
    Ok(())
}

/// Render `data` grouped by board (all of its boards, or just `only`), then
/// the overall stats.
fn show_boards(render: &Render, data: &Items, only: Option<Vec<String>>) {
    let boards = only.unwrap_or_else(|| Taskbook::boards_of(data));
    render.display_by_board(&group_by_board(data, &boards));
    render.display_stats(&Stats::from_items(data.values()));
}

fn parse_due_value(raw: &str) -> Result<Option<i64>> {
    match raw.trim() {
        "" => Err(TaskbookError::MissingDueDate),
        none if none.eq_ignore_ascii_case("none") => Ok(None),
        value => due::parse_due_date(value)
            .map(Some)
            .ok_or_else(|| TaskbookError::InvalidDueDate(value.to_string())),
    }
}

/// True when any term matches the item: `+tag` terms match tags; other terms
/// match the description, note body or tags (case-insensitive substring).
fn item_matches_terms(item: &StorageItem, terms: &[String]) -> bool {
    terms.iter().any(|term| {
        let term = term.to_lowercase();
        let in_tags = |q: &str| item.tags().iter().any(|t| t.to_lowercase().contains(q));
        match term.strip_prefix('+') {
            Some(tag) if !tag.is_empty() => in_tags(tag),
            _ => {
                item.description().to_lowercase().contains(&term)
                    || item
                        .note_body()
                        .is_some_and(|b| b.to_lowercase().contains(&term))
                    || in_tags(&term)
            }
        }
    })
}

/// Item predicates for `tb list`.
fn list_attribute(name: &str) -> Option<fn(&StorageItem) -> bool> {
    let pred: fn(&StorageItem) -> bool = match name {
        "star" | "starred" => |i| i.is_starred(),
        "done" | "checked" | "complete" => |i| i.as_task().is_some_and(|t| t.is_complete),
        "progress" | "started" | "begun" | "in_progress" => {
            |i| i.as_task().is_some_and(|t| t.in_progress)
        }
        "pending" | "unchecked" | "incomplete" => |i| {
            i.as_task()
                .is_some_and(|t| !t.is_complete && !t.in_progress)
        },
        "todo" | "task" | "tasks" => |i| i.is_task(),
        "note" | "notes" => |i| !i.is_task(),
        _ => return None,
    };
    Some(pred)
}

/// Apply `tb list` filters to `data` (every filter must match). Returns the
/// boards to display when any were named.
///
/// A term is, in order: `+tag`; `@board`; a state/type attribute; or a bare
/// name of an existing board. Anything else is an error, as is an `@board`
/// that does not exist.
fn apply_list_filters(
    data: &mut Items,
    filters: &[String],
    known_boards: &[String],
) -> Result<Option<Vec<String>>> {
    let find_board = |name: &str| {
        let normalized = board::normalize_board_name(name);
        known_boards
            .iter()
            .find(|b| board::board_eq(b, &normalized))
            .cloned()
    };

    let mut boards: Vec<String> = Vec::new();
    for term in filters {
        if let Some(tag) = term.strip_prefix('+').filter(|t| !t.is_empty()) {
            let tag = board::normalize_tag(tag);
            data.retain(|_, item| item.tags().iter().any(|t| t.eq_ignore_ascii_case(&tag)));
        } else if term.starts_with('@') && term.len() > 1 {
            let b = find_board(term).ok_or_else(|| {
                TaskbookError::UnknownBoard(board::display_name(&board::normalize_board_name(term)))
            })?;
            boards.push(b);
        } else if let Some(pred) = list_attribute(&term.to_lowercase()) {
            data.retain(|_, item| pred(item));
        } else if let Some(b) = find_board(term) {
            boards.push(b);
        } else {
            return Err(TaskbookError::UnknownFilter(term.clone()));
        }
    }

    if boards.is_empty() {
        return Ok(None);
    }
    boards.dedup_by(|a, b| board::board_eq(a, b));
    Ok(Some(boards))
}

fn group_by_board<'a>(data: &'a Items, boards: &[String]) -> HashMap<String, Vec<&'a StorageItem>> {
    let mut grouped: HashMap<String, Vec<&StorageItem>> = HashMap::new();
    for item in data.values() {
        for b in boards {
            if item.boards().iter().any(|ib| board::board_eq(ib, b)) {
                grouped.entry(b.clone()).or_default().push(item);
            }
        }
    }
    grouped
}

fn group_by_date(data: &Items) -> HashMap<String, Vec<&StorageItem>> {
    let mut grouped: HashMap<String, Vec<&StorageItem>> = HashMap::new();
    for item in data.values() {
        grouped
            .entry(item.date().to_string())
            .or_default()
            .push(item);
    }
    grouped
}

/// Migrate local data to the remote server.
fn migrate(taskbook_dir: Option<PathBuf>) -> Result<()> {
    let creds = Credentials::load()?.ok_or_else(|| {
        TaskbookError::Auth("not logged in — run `tb register` or `tb login` first".to_string())
    })?;

    let config = Config::load_or_default();
    let encryption_key = creds.encryption_key_bytes()?;
    let engine = base64::engine::general_purpose::STANDARD;

    // Load local data
    let resolved_dir = resolve_taskbook_directory(taskbook_dir.as_deref())?;
    let local = LocalStorage::new(&resolved_dir)?;

    let items = local.get()?;
    let archive = local.get_archive()?;

    // Encrypt and upload items
    let client = ApiClient::new(&config.sync.server_url, Some(&creds.token));

    let mut encrypted_items = std::collections::HashMap::new();
    for (key, item) in &items {
        let encrypted = encrypt_item(&encryption_key, item)
            .map_err(|e| TaskbookError::General(format!("encryption failed: {e}")))?;
        encrypted_items.insert(
            key.clone(),
            EncryptedItemData {
                data: engine.encode(&encrypted.data),
                nonce: engine.encode(&encrypted.nonce),
            },
        );
    }
    client.put_items(&encrypted_items)?;

    let mut encrypted_archive = std::collections::HashMap::new();
    for (key, item) in &archive {
        let encrypted = encrypt_item(&encryption_key, item)
            .map_err(|e| TaskbookError::General(format!("encryption failed: {e}")))?;
        encrypted_archive.insert(
            key.clone(),
            EncryptedItemData {
                data: engine.encode(&encrypted.data),
                nonce: engine.encode(&encrypted.nonce),
            },
        );
    }
    client.put_archive(&encrypted_archive)?;

    println!(
        "{}",
        format!(
            "Migrated {} items and {} archived items to server.",
            items.len(),
            archive.len()
        )
        .green()
        .bold()
    );
    println!(
        "{}",
        format!(
            "To enable sync, set sync.enabled = true in {}",
            crate::config::Config::config_file_path().display()
        )
        .dimmed()
    );

    Ok(())
}
