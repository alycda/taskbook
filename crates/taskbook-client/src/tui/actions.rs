use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::editor;
use crate::error::Result;
use crate::taskbook::{NewNote, NewTask};
use taskbook_common::{board, Priority};

use super::app::{App, PendingAction, PopupState, StatusKind, ViewMode};
use super::autocomplete;
use super::command_parser::{self, ParsedCommand};
use super::input_handler::{handle_text_input, InputResult};

/// Handle a key event
pub fn handle_key_event(app: &mut App, key: KeyEvent) -> Result<()> {
    // 1. Help popup → scroll with j/k/arrows, dismiss with q/Esc/other
    if let Some(PopupState::Help { ref mut scroll }) = app.popup {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                *scroll = scroll.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                *scroll = scroll.saturating_sub(1);
            }
            _ => {
                app.popup = None;
            }
        }
        return Ok(());
    }

    // 2. Pending confirm → Enter/Esc only
    if app.command_line.pending_confirm.is_some() {
        return handle_confirm_key(app, key);
    }

    // 3. Command line focused → handle command line input
    if app.command_line.focused {
        return handle_command_line_key(app, key);
    }

    // 4. Normal mode shortcuts
    handle_shortcut_key(app, key)
}

/// Handle keys when a confirmation is pending
fn handle_confirm_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Enter => {
            if let Some(action) = app.command_line.pending_confirm.take() {
                match action {
                    PendingAction::Delete { ids } => {
                        let result = delete_items(app, &ids);
                        report(app, result);
                    }
                    PendingAction::Clear => {
                        let result = clear_completed(app);
                        report(app, result);
                    }
                }
            }
            app.deactivate_command_line();
        }
        KeyCode::Esc => {
            app.command_line.pending_confirm = None;
            app.deactivate_command_line();
        }
        _ => {}
    }
    Ok(())
}

/// Handle keys when the command line is focused
fn handle_command_line_key(app: &mut App, key: KeyEvent) -> Result<()> {
    // Tab accepts the selected suggestion
    if key.code == KeyCode::Tab {
        accept_suggestion(app);
        return Ok(());
    }

    // Up/Down navigate suggestions if visible, otherwise browse history
    match key.code {
        KeyCode::Up => {
            if !app.command_line.suggestions.is_empty() {
                let count = app.command_line.suggestions.len();
                app.command_line.selected_suggestion =
                    Some(match app.command_line.selected_suggestion {
                        None => count - 1,
                        Some(0) => count - 1,
                        Some(i) => i - 1,
                    });
            } else if !app.command_history.is_empty() {
                // Browse history (Up = older)
                let new_idx = match app.history_index {
                    None => {
                        app.history_saved_input = app.command_line.input.clone();
                        app.command_history.len() - 1
                    }
                    Some(0) => 0,
                    Some(i) => i - 1,
                };
                app.history_index = Some(new_idx);
                let entry = app.command_history[new_idx].clone();
                app.command_line.cursor = entry.chars().count();
                app.command_line.input = entry;
                autocomplete::update_suggestions(app);
            }
            return Ok(());
        }
        KeyCode::Down => {
            if !app.command_line.suggestions.is_empty() {
                let count = app.command_line.suggestions.len();
                app.command_line.selected_suggestion =
                    Some(match app.command_line.selected_suggestion {
                        None => 0,
                        Some(i) if i + 1 >= count => 0,
                        Some(i) => i + 1,
                    });
            } else if let Some(idx) = app.history_index {
                // Browse history (Down = newer)
                if idx + 1 >= app.command_history.len() {
                    // Restore saved input
                    app.history_index = None;
                    let saved = app.history_saved_input.clone();
                    app.command_line.cursor = saved.chars().count();
                    app.command_line.input = saved;
                } else {
                    let new_idx = idx + 1;
                    app.history_index = Some(new_idx);
                    let entry = app.command_history[new_idx].clone();
                    app.command_line.cursor = entry.chars().count();
                    app.command_line.input = entry;
                }
                autocomplete::update_suggestions(app);
            }
            return Ok(());
        }
        _ => {}
    }

    // Use the existing text input handler for editing
    let input = app.command_line.input.clone();
    let cursor = app.command_line.cursor;

    match handle_text_input(key, &input, cursor) {
        InputResult::Cancel => {
            app.deactivate_command_line();
        }
        InputResult::Submit => {
            let input = app.command_line.input.clone();
            app.push_history(input.clone());
            app.deactivate_command_line();
            if !input.trim().is_empty() {
                execute_input(app, &input)?;
            }
        }
        InputResult::Changed {
            input: new_input,
            cursor: new_cursor,
        } => {
            app.command_line.input = new_input;
            app.command_line.cursor = new_cursor;
            autocomplete::update_suggestions(app);
        }
        InputResult::Ignored => {}
    }

    Ok(())
}

/// Accept the currently selected suggestion
fn accept_suggestion(app: &mut App) {
    let selected = app.command_line.selected_suggestion.unwrap_or(0);
    if let Some(suggestion) = app.command_line.suggestions.get(selected).cloned() {
        app.command_line.input = suggestion.completion.clone();
        app.command_line.cursor = suggestion.completion.chars().count();
        app.command_line.suggestions.clear();
        app.command_line.selected_suggestion = None;
        // Re-trigger suggestions for the new input
        autocomplete::update_suggestions(app);
    }
}

/// Parse and execute the command line input. Parse and operation errors
/// (unknown id, note given a priority, ...) go to the status bar instead of
/// ending the TUI.
fn execute_input(app: &mut App, input: &str) -> Result<()> {
    let result = command_parser::parse_command(input)
        .map_err(|e| e.message)
        .and_then(|cmd| execute_command(app, cmd).map_err(|e| e.to_string()));
    if let Err(message) = result {
        app.set_status(message, StatusKind::Error);
    }
    Ok(())
}

/// Explicit boards, else the filtered board, else the default board.
fn target_boards(app: &App, boards: Vec<String>) -> Vec<String> {
    if !boards.is_empty() {
        return boards;
    }
    vec![app
        .filter
        .board_filter
        .clone()
        .unwrap_or_else(|| board::DEFAULT_BOARD.to_string())]
}

fn describe_boards(boards: &[String]) -> String {
    boards
        .iter()
        .map(|b| board::display_name(&board::normalize_board_name(b)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Execute a parsed command
fn execute_command(app: &mut App, cmd: ParsedCommand) -> Result<()> {
    match cmd {
        ParsedCommand::Task {
            boards,
            description,
            priority,
            tags,
            due_date,
        } => {
            let boards = target_boards(app, boards);
            let display = describe_boards(&boards);
            app.taskbook.create_task(NewTask {
                boards,
                description,
                priority,
                tags,
                due_date,
            })?;
            app.refresh_items()?;
            app.set_status(format!("Task created in {}", display), StatusKind::Success);
        }
        ParsedCommand::Note {
            boards,
            description,
            tags,
        } => {
            let boards = target_boards(app, boards);
            let display = describe_boards(&boards);
            let id = app.taskbook.create_note(NewNote {
                boards,
                title: description,
                body: None,
                tags,
            })?;
            app.refresh_items()?;
            app.set_status(format!("Note created in {}", display), StatusKind::Success);

            // Open editor for the newly created note
            edit_note_external(app, id)?;
        }
        ParsedCommand::Edit { id, description } => {
            edit_description(app, id, &description)?;
        }
        ParsedCommand::Move { id, board } => {
            move_to_board(app, id, &board)?;
        }
        ParsedCommand::Delete { ids } => {
            app.command_line.pending_confirm = Some(PendingAction::Delete { ids });
        }
        ParsedCommand::Search { term } => {
            app.filter.search_term = Some(term.clone());
            app.update_display_order();
            app.selected_index = 0;
            let count = app.display_order.len();
            app.set_status(
                format!("Search: \"{}\" ({} matches)", term, count),
                StatusKind::Info,
            );
        }
        ParsedCommand::Show { ids } => {
            let unknown: Vec<u64> = ids
                .iter()
                .copied()
                .filter(|id| !app.items.contains_key(&id.to_string()))
                .collect();
            if !unknown.is_empty() {
                return Err(crate::error::TaskbookError::UnknownIds(unknown));
            }
            app.clear_board_filter();
            app.filter.search_term = None;
            app.filter.id_filter = Some(ids);
            app.update_display_order();
            app.selected_index = 0;
        }
        ParsedCommand::Priority { id, level } => {
            set_priority(app, id, level)?;
        }
        ParsedCommand::Check { ids } => toggle_check(app, &ids)?,
        ParsedCommand::Star { ids } => toggle_star(app, &ids)?,
        ParsedCommand::Begin { ids } => toggle_begin(app, &ids)?,
        ParsedCommand::Tag { id, add, remove } => {
            update_tags(app, id, &add, &remove)?;
        }
        ParsedCommand::Clear => {
            app.command_line.pending_confirm = Some(PendingAction::Clear);
        }
        ParsedCommand::RenameBoard { old_name, new_name } => {
            rename_board(app, &old_name, &new_name)?;
        }
        ParsedCommand::Board => {
            app.clear_board_filter();
            app.set_view(ViewMode::Board)?;
        }
        ParsedCommand::Timeline => {
            app.clear_board_filter();
            app.set_view(ViewMode::Timeline)?;
        }
        ParsedCommand::Archive => {
            app.clear_board_filter();
            app.set_view(ViewMode::Archive)?;
        }
        ParsedCommand::Journal => {
            app.clear_board_filter();
            app.set_view(ViewMode::Journal)?;
        }
        ParsedCommand::Sort => {
            app.cycle_sort_method();
            app.set_status(
                format!("Sort: {}", app.sort_method.display_name()),
                StatusKind::Info,
            );
        }
        ParsedCommand::HideDone => {
            app.toggle_hide_completed();
            let msg = if app.filter.hide_completed {
                "Hiding completed tasks"
            } else {
                "Showing completed tasks"
            };
            app.set_status(msg.to_string(), StatusKind::Info);
        }
        ParsedCommand::Help => {
            app.popup = Some(PopupState::Help { scroll: 0 });
        }
        ParsedCommand::Quit => {
            app.quit();
        }
    }
    Ok(())
}

/// Handle shortcut keys in normal (unfocused) mode
fn handle_shortcut_key(app: &mut App, key: KeyEvent) -> Result<()> {
    // Ctrl+D / Ctrl+U for half-page navigation
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('d') => {
                let half = (app.content_height / 2).max(1) as usize;
                app.select_down_by(half);
            }
            KeyCode::Char('u') => {
                let half = (app.content_height / 2).max(1) as usize;
                app.select_up_by(half);
            }
            _ => {}
        }
        return Ok(());
    }

    match key.code {
        // Quit
        KeyCode::Char('q') => app.quit(),
        KeyCode::Esc => {
            if app.filter.id_filter.is_some() {
                app.filter.id_filter = None;
                app.update_display_order();
                app.selected_index = 0;
                app.set_status("Showing all items".to_string(), StatusKind::Info);
            } else if app.filter.search_term.is_some() {
                app.filter.search_term = None;
                app.update_display_order();
                app.selected_index = 0;
                app.set_status("Search cleared".to_string(), StatusKind::Info);
            } else if app.filter.board_filter.is_some() {
                app.clear_board_filter();
                app.set_status("Filter cleared".to_string(), StatusKind::Info);
            }
        }

        // Navigation
        KeyCode::Char('j') | KeyCode::Down => app.select_next(),
        KeyCode::Char('k') | KeyCode::Up => app.select_previous(),
        KeyCode::Char('g') => app.select_first(),
        KeyCode::Char('G') => app.select_last(),
        KeyCode::PageDown => {
            let page = app.content_height.max(1) as usize;
            app.select_down_by(page);
        }
        KeyCode::PageUp => {
            let page = app.content_height.max(1) as usize;
            app.select_up_by(page);
        }

        // Enter to open note in editor or filter by board
        KeyCode::Enter => {
            if let Some(item) = app.selected_item() {
                if !item.is_task() {
                    let id = item.id();
                    let result = edit_note_external(app, id);
                    report(app, result);
                } else if app.view == ViewMode::Board && app.filter.board_filter.is_none() {
                    if let Some(board_name) = app.get_board_for_selected() {
                        let display = board::display_name(&board_name);
                        app.set_board_filter(Some(board_name));
                        app.set_status(format!("Filtering by {}", display), StatusKind::Info);
                    }
                }
            }
        }

        // View switching
        KeyCode::Char('1') => {
            app.clear_board_filter();
            app.set_view(ViewMode::Board)?;
        }
        KeyCode::Char('2') => {
            app.clear_board_filter();
            app.set_view(ViewMode::Timeline)?;
        }
        KeyCode::Char('3') => {
            app.clear_board_filter();
            app.set_view(ViewMode::Archive)?;
        }
        KeyCode::Char('4') => {
            app.clear_board_filter();
            app.set_view(ViewMode::Journal)?;
        }

        // Help
        KeyCode::Char('?') => {
            app.popup = Some(PopupState::Help { scroll: 0 });
        }

        // Slash or Tab activates command line
        KeyCode::Char('/') | KeyCode::Tab => {
            app.activate_command_line("/");
            autocomplete::update_suggestions(app);
        }

        // Pre-fill shortcuts — activate command line with partial command
        KeyCode::Char('t') if app.view != ViewMode::Archive => {
            if let Some(ref board) = app.filter.board_filter.clone() {
                app.activate_command_line(&format!("/task @{} ", board));
            } else {
                app.activate_command_line("/task @");
                autocomplete::update_suggestions(app);
            }
        }
        KeyCode::Char('n') if app.view != ViewMode::Archive => {
            if let Some(ref board) = app.filter.board_filter.clone() {
                app.activate_command_line(&format!("/note @{} ", board));
            } else {
                app.activate_command_line("/note @");
                autocomplete::update_suggestions(app);
            }
        }
        KeyCode::Char('e') if app.view != ViewMode::Archive => {
            if let Some(item) = app.selected_item() {
                let id = item.id();
                let desc = item.description().to_string();
                app.activate_command_line(&format!("/edit {} {}", id, desc));
            }
        }
        KeyCode::Char('m') if app.view != ViewMode::Archive => {
            if let Some(id) = app.selected_id() {
                app.activate_command_line(&format!("/move {} @", id));
                autocomplete::update_suggestions(app);
            }
        }
        KeyCode::Char('p') if app.view != ViewMode::Archive => {
            if let Some(item) = app.selected_item() {
                if item.is_task() {
                    app.activate_command_line(&format!("/priority {} ", item.id()));
                }
            }
        }
        KeyCode::Char('d') if app.view != ViewMode::Archive => {
            if let Some(id) = app.selected_id() {
                app.command_line.pending_confirm = Some(PendingAction::Delete { ids: vec![id] });
            }
        }
        KeyCode::Char('C') if app.view != ViewMode::Archive => {
            app.command_line.pending_confirm = Some(PendingAction::Clear);
        }

        // Direct action shortcuts (no command line needed)
        KeyCode::Char('c') if app.view != ViewMode::Archive => {
            if let Some(id) = app.selected_id() {
                let result = toggle_check(app, &[id]);
                report(app, result);
            }
        }
        KeyCode::Char('b') if app.view != ViewMode::Archive => {
            if let Some(id) = app.selected_id() {
                let result = toggle_begin(app, &[id]);
                report(app, result);
            }
        }
        KeyCode::Char('s') if app.view != ViewMode::Archive => {
            if let Some(id) = app.selected_id() {
                let result = toggle_star(app, &[id]);
                report(app, result);
            }
        }
        KeyCode::Char('r') if app.view == ViewMode::Archive => {
            if let Some(id) = app.selected_id() {
                let result = restore_item(app, id);
                report(app, result);
            }
        }
        KeyCode::Char('y') => {
            if let Some(id) = app.selected_id() {
                let result = copy_to_clipboard(app, id);
                report(app, result);
            }
        }

        // Cycle sort method
        KeyCode::Char('S') if app.view == ViewMode::Board => {
            app.cycle_sort_method();
            app.set_status(
                format!("Sort: {}", app.sort_method.display_name()),
                StatusKind::Info,
            );
        }
        // Toggle hide completed
        KeyCode::Char('h') if app.view != ViewMode::Archive => {
            app.toggle_hide_completed();
            let msg = if app.filter.hide_completed {
                "Hiding completed tasks"
            } else {
                "Showing completed tasks"
            };
            app.set_status(msg.to_string(), StatusKind::Info);
        }

        _ => {}
    }

    Ok(())
}

// Action implementations

/// Show an operation error in the status bar instead of ending the TUI.
fn report(app: &mut App, result: Result<()>) {
    if let Err(e) = result {
        app.set_status(e.to_string(), StatusKind::Error);
    }
}

fn join_ids(ids: &[u64]) -> String {
    ids.iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn toggle_check(app: &mut App, ids: &[u64]) -> Result<()> {
    let out = app.taskbook.toggle_check(ids)?;
    app.refresh_items()?;
    let mut parts = Vec::new();
    if !out.on.is_empty() {
        parts.push(format!("checked {}", join_ids(&out.on)));
    }
    if !out.off.is_empty() {
        parts.push(format!("unchecked {}", join_ids(&out.off)));
    }
    app.set_status(format!("Tasks {}", parts.join(", ")), StatusKind::Success);
    Ok(())
}

fn toggle_begin(app: &mut App, ids: &[u64]) -> Result<()> {
    let out = app.taskbook.toggle_begin(ids)?;
    app.refresh_items()?;
    let mut parts = Vec::new();
    if !out.on.is_empty() {
        parts.push(format!("started {}", join_ids(&out.on)));
    }
    if !out.off.is_empty() {
        parts.push(format!("paused {}", join_ids(&out.off)));
    }
    app.set_status(format!("Tasks {}", parts.join(", ")), StatusKind::Success);
    Ok(())
}

fn toggle_star(app: &mut App, ids: &[u64]) -> Result<()> {
    let out = app.taskbook.toggle_star(ids)?;
    app.refresh_items()?;
    let mut parts = Vec::new();
    if !out.on.is_empty() {
        parts.push(format!("starred {}", join_ids(&out.on)));
    }
    if !out.off.is_empty() {
        parts.push(format!("unstarred {}", join_ids(&out.off)));
    }
    app.set_status(format!("Items {}", parts.join(", ")), StatusKind::Success);
    Ok(())
}

fn edit_description(app: &mut App, id: u64, new_desc: &str) -> Result<()> {
    app.taskbook.edit_description(id, new_desc.to_string())?;
    app.refresh_items()?;
    app.set_status(format!("Updated item {}", id), StatusKind::Success);
    Ok(())
}

fn move_to_board(app: &mut App, id: u64, board: &str) -> Result<()> {
    let boards = app
        .taskbook
        .move_to_boards(&[id], vec![board.to_string()])?;
    app.refresh_items()?;
    app.set_status(
        format!("Moved item {} to {}", id, describe_boards(&boards)),
        StatusKind::Success,
    );
    Ok(())
}

fn set_priority(app: &mut App, id: u64, priority: Priority) -> Result<()> {
    app.taskbook.set_priority(&[id], priority)?;
    app.refresh_items()?;
    app.set_status(
        format!("Set priority of task {} to {}", id, priority),
        StatusKind::Success,
    );
    Ok(())
}

fn delete_items(app: &mut App, ids: &[u64]) -> Result<()> {
    let deleted = app.taskbook.delete(ids)?;
    app.refresh_items()?;
    app.set_status(
        format!("Deleted {} item(s)", deleted.len()),
        StatusKind::Success,
    );
    Ok(())
}

fn restore_item(app: &mut App, id: u64) -> Result<()> {
    app.taskbook.restore(&[id])?;
    app.set_view(ViewMode::Archive)?;
    app.set_status(format!("Restored item {}", id), StatusKind::Success);
    Ok(())
}

fn copy_to_clipboard(app: &mut App, id: u64) -> Result<()> {
    app.taskbook.copy_to_clipboard(&[id])?;
    app.set_status(
        format!("Copied item {} to clipboard", id),
        StatusKind::Success,
    );
    Ok(())
}

fn update_tags(app: &mut App, id: u64, add: &[String], remove: &[String]) -> Result<()> {
    app.taskbook
        .update_tags(&[id], add.to_vec(), remove.to_vec())?;
    app.refresh_items()?;
    let mut parts = Vec::new();
    if !add.is_empty() {
        let tags_str = add
            .iter()
            .map(|t| format!("+{}", t))
            .collect::<Vec<_>>()
            .join(" ");
        parts.push(format!("added {}", tags_str));
    }
    if !remove.is_empty() {
        let tags_str = remove
            .iter()
            .map(|t| format!("-{}", t))
            .collect::<Vec<_>>()
            .join(" ");
        parts.push(format!("removed {}", tags_str));
    }
    app.set_status(
        format!("Tags on item {}: {}", id, parts.join(", ")),
        StatusKind::Success,
    );
    Ok(())
}

fn clear_completed(app: &mut App) -> Result<()> {
    let count = app.taskbook.clear_completed()?.len();
    app.refresh_items()?;
    app.set_status(
        format!("Cleared {} completed task(s)", count),
        StatusKind::Success,
    );
    Ok(())
}

fn rename_board(app: &mut App, old_name: &str, new_name: &str) -> Result<()> {
    let new_board = board::normalize_board_name(new_name);
    let count = app.taskbook.rename_board(old_name, &new_board)?;

    if let Some(ref filter) = app.filter.board_filter {
        if board::board_eq(filter, old_name) {
            app.filter.board_filter = Some(new_board.clone());
        }
    }

    app.refresh_items()?;
    let old_display = board::display_name(old_name);
    let new_display = board::display_name(&new_board);
    app.set_status(
        format!(
            "Renamed {} to {} ({} items)",
            old_display, new_display, count
        ),
        StatusKind::Success,
    );
    Ok(())
}

fn edit_note_external(app: &mut App, id: u64) -> Result<()> {
    let item = app.items.get(&id.to_string());
    let note = match item.and_then(|i| i.as_note()) {
        Some(n) => n,
        None => {
            app.set_status("Item is not a note".to_string(), StatusKind::Error);
            return Ok(());
        }
    };

    let title = note.title().to_string();
    let body = note.body().map(|s| s.to_string());

    // Suspend TUI to run external editor
    let guard = super::suspend_tui()?;

    // Open external editor
    let content = editor::edit_existing_note_in_editor(&title, body.as_deref());

    // Resume TUI
    guard.resume()?;

    // After suspend/resume, ratatui's internal buffer is stale — force full redraw
    app.needs_full_redraw = true;

    match content? {
        Some(note_content) => {
            app.taskbook
                .set_note_content(id, note_content.title, note_content.body)?;
            app.refresh_items()?;
            app.set_status(format!("Updated note {}", id), StatusKind::Success);
        }
        None => {
            app.set_status("Edit cancelled".to_string(), StatusKind::Info);
        }
    }

    Ok(())
}
