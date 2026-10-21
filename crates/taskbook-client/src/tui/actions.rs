use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::editor;
use crate::error::Result;
use taskbook_common::board;
use taskbook_common::links::{self, Rejection, Rewrite, Target};

use super::app::{
    App, PendingAction, PopupState, StatusKind, SuggestionKind, ViewMode, PANE_MIN_WIDTH,
};
use super::autocomplete;
use super::command_parser::{self, ParsedCommand};
use super::input_handler::{handle_text_input, InputResult};

/// Handle a key event
pub fn handle_key_event(app: &mut App, key: KeyEvent) -> Result<()> {
    // A sticky status (e.g. a rejected save) stays until the next key press.
    if app.status_message.as_ref().is_some_and(|m| m.sticky) {
        app.status_message = None;
    }

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

    // 4. Link pane focused → move, jump, or return to the list. Focus goes
    //    back to the list if the pane was hidden (toggled off or too narrow).
    if app.pane.focused {
        if app.pane_shown() {
            return handle_pane_key(app, key);
        }
        app.pane.focused = false;
    }

    // 5. Normal mode shortcuts
    handle_shortcut_key(app, key)
}

/// Handle keys while the link pane has focus
fn handle_pane_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let content = app.pane_content();
    let cursor = app.pane_cursor(&content);
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.pane.cursor = (cursor + 1).min(content.len().saturating_sub(1));
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.pane.cursor = cursor.saturating_sub(1);
        }
        KeyCode::Enter => match content.get(cursor).map(|e| (e.jump_target(), e.target)) {
            Some((Some(numeric), _)) => app.jump_to(numeric)?,
            Some((None, Target::Divergent)) => {
                app.set_status(
                    "Two items share that ID; cannot jump".to_string(),
                    StatusKind::Info,
                );
            }
            Some((None, _)) => {
                app.set_status("That link is broken".to_string(), StatusKind::Info);
            }
            None => {}
        },
        KeyCode::Esc | KeyCode::Char('o') => {
            app.pane.focused = false;
        }
        _ => {}
    }
    Ok(())
}

/// Handle keys when a confirmation is pending
fn handle_confirm_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Enter => {
            if let Some(action) = app.command_line.pending_confirm.take() {
                match action {
                    PendingAction::Delete { ids } => {
                        delete_items(app, &ids)?;
                    }
                    PendingAction::Clear => {
                        clear_completed(app)?;
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

    // Inside an unclosed `[[`, Enter picks a link target instead of
    // submitting, and Esc first closes only the dropdown.
    if let Some((open, _)) = autocomplete::open_link_query(app) {
        let has_links = app
            .command_line
            .suggestions
            .iter()
            .any(|s| s.kind == SuggestionKind::Link);
        match key.code {
            KeyCode::Enter => {
                if has_links {
                    accept_suggestion(app);
                } else {
                    app.set_status(
                        "Pick an item, close the brackets with ]], or press Esc".to_string(),
                        StatusKind::Info,
                    );
                }
                return Ok(());
            }
            KeyCode::Esc if has_links => {
                app.command_line.suggestions.clear();
                app.command_line.selected_suggestion = None;
                app.command_line.link_dismissed_at = Some(open);
                return Ok(());
            }
            _ => {}
        }
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
            let cursor = app.command_line.cursor;
            let baseline = app.command_line.link_baseline.take();
            app.push_history(input.clone());
            app.deactivate_command_line();
            if !input.trim().is_empty() {
                if let Some(rejected) = execute_input(app, &input, baseline)? {
                    // Give the draft back so the user can fix the references.
                    app.activate_command_line(&input);
                    app.command_line.cursor = cursor;
                    app.command_line.link_baseline = rejected.baseline;
                    let message = format_rejections(&rejected.rejections, app.status_width);
                    app.set_sticky_status(message, StatusKind::Error);
                }
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

/// A save whose added `[[…]]` references could not all be resolved.
struct Rejected {
    rejections: Vec<Rejection>,
    /// The baseline the draft was diffed against, kept for the retry.
    baseline: Option<(u64, String)>,
}

/// Parse and execute the command line input
fn execute_input(
    app: &mut App,
    input: &str,
    baseline: Option<(u64, String)>,
) -> Result<Option<Rejected>> {
    match command_parser::parse_command(input) {
        Ok(cmd) => execute_command(app, cmd, baseline),
        Err(e) => {
            app.set_status(e.message, StatusKind::Error);
            Ok(None)
        }
    }
}

/// Execute a parsed command
fn execute_command(
    app: &mut App,
    cmd: ParsedCommand,
    baseline: Option<(u64, String)>,
) -> Result<Option<Rejected>> {
    match cmd {
        ParsedCommand::Task {
            board,
            description,
            priority,
            tags,
            due_date,
        } => {
            let rewrite = match links::rewrite_on_save("", &description, app.links.resolver()) {
                Ok(rewrite) => rewrite,
                Err(rejections) => {
                    return Ok(Some(Rejected {
                        rejections,
                        baseline: None,
                    }))
                }
            };
            let board_name = board
                .map(|b| board::normalize_board_name(&b))
                .or_else(|| app.filter.board_filter.clone())
                .unwrap_or_else(|| "my board".to_string());
            app.taskbook.create_task_direct_with_tags(
                vec![board_name.clone()],
                rewrite.text.clone(),
                priority,
                tags,
                due_date,
            )?;
            app.refresh_items()?;
            let display = board::display_name(&board_name);
            let message = format!(
                "Task created in {}{}",
                display,
                linked_suffix(app, &rewrite)
            );
            app.set_status(message, StatusKind::Success);
        }
        ParsedCommand::Note {
            board,
            description,
            tags,
        } => {
            let rewrite = match links::rewrite_on_save("", &description, app.links.resolver()) {
                Ok(rewrite) => rewrite,
                Err(rejections) => {
                    return Ok(Some(Rejected {
                        rejections,
                        baseline: None,
                    }))
                }
            };
            let board_name = board
                .map(|b| board::normalize_board_name(&b))
                .or_else(|| app.filter.board_filter.clone())
                .unwrap_or_else(|| "my board".to_string());
            app.taskbook.create_note_direct_with_tags(
                vec![board_name.clone()],
                rewrite.text.clone(),
                tags,
            )?;
            app.refresh_items()?;
            let display = board::display_name(&board_name);
            let message = format!(
                "Note created in {}{}",
                display,
                linked_suffix(app, &rewrite)
            );
            app.set_status(message, StatusKind::Success);

            // Open editor for the newly created note
            // Find the highest ID note (just created)
            if let Some(max_id) = app.items.values().map(|i| i.id()).max() {
                if let Some(item) = app.items.get(&max_id.to_string()) {
                    if !item.is_task() {
                        edit_note_external(app, max_id)?;
                    }
                }
            }
        }
        ParsedCommand::Edit { id, description } => {
            // The baseline is captured once per edit session (KTD7): from the
            // `e`/`l` prefill, or from storage on the first submit.
            let base = baseline
                .filter(|(base_id, _)| *base_id == id)
                .map(|(_, text)| text)
                .or_else(|| {
                    app.active_items
                        .get(&id.to_string())
                        .map(|item| item.description().to_string())
                })
                .unwrap_or_default();
            match links::rewrite_on_save(&base, &description, app.links.resolver()) {
                Ok(rewrite) => edit_description(app, id, &rewrite)?,
                Err(rejections) => {
                    return Ok(Some(Rejected {
                        rejections,
                        baseline: Some((id, base)),
                    }))
                }
            }
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
        ParsedCommand::Priority { id, level } => {
            set_priority(app, id, level)?;
        }
        ParsedCommand::Check { ids } => {
            for id in &ids {
                toggle_check(app, *id)?;
            }
        }
        ParsedCommand::Star { ids } => {
            for id in &ids {
                toggle_star(app, *id)?;
            }
        }
        ParsedCommand::Begin { ids } => {
            for id in &ids {
                toggle_begin(app, *id)?;
            }
        }
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
    Ok(None)
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
            if app.filter.search_term.is_some() {
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
                    edit_note_external(app, item.id())?;
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
                app.activate_command_line(&format!("/edit @{} {}", id, desc));
                app.command_line.link_baseline = Some((id, desc));
            }
        }
        // Link pane: toggle and focus
        KeyCode::Char('v') => {
            app.pane.visible = !app.pane.visible;
            if app.status_width < PANE_MIN_WIDTH {
                let state = if app.pane.visible { "on" } else { "off" };
                app.set_status(
                    format!(
                        "Link pane needs {} columns (toggle {})",
                        PANE_MIN_WIDTH, state
                    ),
                    StatusKind::Info,
                );
            }
        }
        KeyCode::Char('o') => {
            if !app.pane_shown() {
                app.set_status("Link pane hidden".to_string(), StatusKind::Info);
            } else if app.pane_content().is_empty() {
                app.set_status("No links for this item".to_string(), StatusKind::Info);
            } else {
                app.pane.focused = true;
                app.pane.cursor = 0;
            }
        }

        KeyCode::Char('l') if app.view != ViewMode::Archive => {
            if let Some(item) = app.selected_item() {
                let id = item.id();
                let desc = item.description().to_string();
                app.activate_command_line(&format!("/edit @{} {} [[", id, desc));
                app.command_line.link_baseline = Some((id, desc));
                autocomplete::update_suggestions(app);
            }
        }
        KeyCode::Char('m') if app.view != ViewMode::Archive => {
            if let Some(id) = app.selected_id() {
                app.activate_command_line(&format!("/move @{} @", id));
                autocomplete::update_suggestions(app);
            }
        }
        KeyCode::Char('p') if app.view != ViewMode::Archive => {
            if let Some(item) = app.selected_item() {
                if item.is_task() {
                    app.activate_command_line(&format!("/priority @{} ", item.id()));
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
                toggle_check(app, id)?;
            }
        }
        KeyCode::Char('b') if app.view != ViewMode::Archive => {
            if let Some(id) = app.selected_id() {
                toggle_begin(app, id)?;
            }
        }
        KeyCode::Char('s') if app.view != ViewMode::Archive => {
            if let Some(id) = app.selected_id() {
                toggle_star(app, id)?;
            }
        }
        KeyCode::Char('r') if app.view == ViewMode::Archive => {
            if let Some(id) = app.selected_id() {
                restore_item(app, id)?;
            }
        }
        KeyCode::Char('y') => {
            if let Some(id) = app.selected_id() {
                copy_to_clipboard(app, id)?;
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

fn toggle_check(app: &mut App, id: u64) -> Result<()> {
    if let Some(item) = app.items.get(&id.to_string()) {
        if item.is_task() {
            app.taskbook.check_tasks_silent(&[id])?;
            app.refresh_items()?;
            app.set_status(format!("Toggled task {}", id), StatusKind::Success);
        }
    }
    Ok(())
}

fn toggle_begin(app: &mut App, id: u64) -> Result<()> {
    if let Some(item) = app.items.get(&id.to_string()) {
        if item.is_task() {
            app.taskbook.begin_tasks_silent(&[id])?;
            app.refresh_items()?;
            app.set_status(
                format!("Toggled in-progress for task {}", id),
                StatusKind::Success,
            );
        }
    }
    Ok(())
}

fn toggle_star(app: &mut App, id: u64) -> Result<()> {
    app.taskbook.star_items_silent(&[id])?;
    app.refresh_items()?;
    app.set_status(format!("Toggled star for item {}", id), StatusKind::Success);
    Ok(())
}

fn edit_description(app: &mut App, id: u64, rewrite: &Rewrite) -> Result<()> {
    app.taskbook.edit_description_silent(id, &rewrite.text)?;
    app.refresh_items()?;
    let message = format!("Updated item {}{}", id, linked_suffix(app, rewrite));
    app.set_status(message, StatusKind::Success);
    Ok(())
}

/// Name each reference a save rewrote and the title it now links to, so an
/// accidental match is visible at once.
fn linked_suffix(app: &App, rewrite: &Rewrite) -> String {
    if rewrite.linked.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = rewrite
        .linked
        .iter()
        .map(|(reference, id)| {
            let title = app.links.title(id).unwrap_or_default();
            format!("{} \u{2192} {}", reference, title)
        })
        .collect();
    format!(" \u{00b7} linked {}", parts.join(", "))
}

/// "Not saved: …" naming every rejected reference; when that would not fit
/// in `width` columns, later references collapse into "+N more" (the first is
/// always shown in full).
fn format_rejections(rejections: &[Rejection], width: u16) -> String {
    let width = usize::from(width);
    let mut message = String::from("Not saved: ");
    for (i, rejection) in rejections.iter().enumerate() {
        let part = rejection.to_string();
        let remaining = rejections.len() - i - 1;
        let tail = if remaining > 0 {
            format!(" +{} more", remaining)
        } else {
            String::new()
        };
        let separator = if i == 0 { "" } else { ", " };
        let fits =
            message.chars().count() + separator.len() + part.chars().count() + tail.len() <= width;
        if i > 0 && !fits {
            message.push_str(&format!(" +{} more", rejections.len() - i));
            break;
        }
        message.push_str(separator);
        message.push_str(&part);
    }
    message
}

fn move_to_board(app: &mut App, id: u64, board: &str) -> Result<()> {
    let board_name = board::normalize_board_name(board);
    app.taskbook
        .move_boards_silent(id, vec![board_name.clone()])?;
    app.refresh_items()?;
    let display = board::display_name(&board_name);
    app.set_status(
        format!("Moved item {} to {}", id, display),
        StatusKind::Success,
    );
    Ok(())
}

fn set_priority(app: &mut App, id: u64, priority: u8) -> Result<()> {
    app.taskbook.update_priority_silent(id, priority)?;
    app.refresh_items()?;
    app.set_status(
        format!("Set priority {} for task {}", priority, id),
        StatusKind::Success,
    );
    Ok(())
}

fn delete_items(app: &mut App, ids: &[u64]) -> Result<()> {
    app.taskbook.delete_items_silent(ids)?;
    app.refresh_items()?;
    app.set_status(
        format!("Deleted {} item(s)", ids.len()),
        StatusKind::Success,
    );
    Ok(())
}

fn restore_item(app: &mut App, id: u64) -> Result<()> {
    app.taskbook.restore_items_silent(&[id])?;
    // Already in Archive view, so set_view would not reload anything.
    app.refresh_items()?;
    app.set_status(format!("Restored item {}", id), StatusKind::Success);
    Ok(())
}

fn copy_to_clipboard(app: &mut App, id: u64) -> Result<()> {
    app.taskbook.copy_to_clipboard_silent(&[id])?;
    app.set_status(
        format!("Copied item {} to clipboard", id),
        StatusKind::Success,
    );
    Ok(())
}

fn update_tags(app: &mut App, id: u64, add: &[String], remove: &[String]) -> Result<()> {
    app.taskbook.update_tags_silent(id, add, remove)?;
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
    let count = app.taskbook.clear_silent()?;
    app.refresh_items()?;
    app.set_status(
        format!("Cleared {} completed task(s)", count),
        StatusKind::Success,
    );
    Ok(())
}

fn rename_board(app: &mut App, old_name: &str, new_name: &str) -> Result<()> {
    let new_board = board::normalize_board_name(new_name);
    let count = app.taskbook.rename_board_silent(old_name, &new_board)?;

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

    // The stored text when the session starts is the baseline for every retry.
    let base_title = note.title().to_string();
    let base_body = note.body().map(|s| s.to_string());
    let mut title = base_title.clone();
    let mut body = base_body.clone();
    let mut notice: Option<String> = None;

    loop {
        // Suspend TUI to run external editor
        let guard = super::suspend_tui()?;

        // Open external editor
        let content =
            editor::edit_existing_note_in_editor(&title, body.as_deref(), notice.as_deref());

        // Resume TUI
        guard.resume()?;

        // After suspend/resume, ratatui's internal buffer is stale — force full redraw
        app.needs_full_redraw = true;

        let Some(note_content) = content? else {
            app.set_status("Edit cancelled".to_string(), StatusKind::Info);
            return Ok(());
        };

        let baseline = (base_title.as_str(), base_body.as_deref());
        match save_note_edit(
            app,
            id,
            baseline,
            &note_content.title,
            note_content.body.as_deref(),
        )? {
            Ok(rewrite) => {
                let message = format!("Updated note {}{}", id, linked_suffix(app, &rewrite));
                app.set_status(message, StatusKind::Success);
                return Ok(());
            }
            Err(rejections) => {
                // Reopen on the edited content, explaining what to fix.
                notice = Some(format_rejections(&rejections, u16::MAX));
                title = note_content.title;
                body = note_content.body;
            }
        }
    }
}

/// Validate a note's edited title and body together and, only if every added
/// reference resolves, write both. A rejection writes nothing.
fn save_note_edit(
    app: &mut App,
    id: u64,
    baseline: (&str, Option<&str>),
    title: &str,
    body: Option<&str>,
) -> Result<std::result::Result<Rewrite, Vec<Rejection>>> {
    let base = links::join_note(baseline.0, baseline.1);
    let draft = links::join_note(title, body);
    let rewrite = match links::rewrite_on_save(&base, &draft, app.links.resolver()) {
        Ok(rewrite) => rewrite,
        Err(rejections) => return Ok(Err(rejections)),
    };
    let (title, body) = links::split_note(&rewrite.text);
    app.taskbook.edit_description_silent(id, &title)?;
    app.taskbook.edit_note_body_silent(id, body)?;
    app.refresh_items()?;
    Ok(Ok(rewrite))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::tests::{key, test_app};
    use taskbook_common::links::{RejectReason, Rejection};

    fn press(app: &mut App, code: KeyCode) {
        handle_key_event(app, KeyEvent::new(code, KeyModifiers::NONE)).unwrap();
    }

    fn submit(app: &mut App, input: &str) {
        app.activate_command_line(input);
        press(app, KeyCode::Enter);
    }

    fn create(app: &mut App, desc: &str) -> u64 {
        let id = app
            .taskbook
            .create_task_direct_with_tags(vec!["my board".into()], desc.into(), 1, vec![], None)
            .unwrap();
        app.refresh_items().unwrap();
        id
    }

    fn description(app: &App, id: u64) -> String {
        app.active_items[&id.to_string()].description().to_string()
    }

    fn status(app: &App) -> String {
        app.status_message
            .as_ref()
            .map(|m| m.text.clone())
            .unwrap_or_default()
    }

    /// Covers AE1.
    #[test]
    fn rejected_create_keeps_the_draft_open() {
        let (mut app, _) = test_app();
        submit(&mut app, "/task see [[99]]");

        assert!(app.command_line.focused);
        assert_eq!(app.command_line.input, "/task see [[99]]");
        assert!(
            status(&app).contains("[[99]] not found"),
            "{}",
            status(&app)
        );
        assert!(app.active_items.is_empty());
    }

    #[test]
    fn edit_rewrites_a_numeric_reference_and_names_it() {
        let (mut app, _) = test_app();
        let target = create(&mut app, "Milk run");
        let edited = create(&mut app, "x");
        submit(&mut app, &format!("/edit @{edited} see [[{target}]]"));

        let tid = app.active_items[&target.to_string()].stable_id();
        assert_eq!(description(&app, edited), format!("see [[{tid}]]"));
        assert!(!app.command_line.focused);
        assert!(status(&app).contains("Milk run"), "{}", status(&app));
    }

    /// Covers AE3.
    #[test]
    fn editing_cli_text_keeps_numeric_brackets_plain() {
        let (mut app, _) = test_app();
        create(&mut app, "other");
        let id = create(&mut app, "see [[1]]");
        app.selected_index = app.display_order.iter().position(|&i| i == id).unwrap();

        handle_key_event(&mut app, key('e')).unwrap();
        for c in " fixed".chars() {
            handle_key_event(&mut app, key(c)).unwrap();
        }
        press(&mut app, KeyCode::Enter);

        assert_eq!(description(&app, id), "see [[1]] fixed");
    }

    #[test]
    fn retry_after_rejection_uses_the_original_baseline() {
        let (mut app, _) = test_app();
        let id = create(&mut app, "a");
        submit(&mut app, &format!("/edit @{id} a [[99]]"));
        assert!(app.command_line.focused);

        // Edit the kept draft and submit again: [[99]] is still an addition.
        app.command_line.input.push_str(" b");
        press(&mut app, KeyCode::Enter);
        assert!(app.command_line.focused);
        assert_eq!(description(&app, id), "a");
    }

    #[test]
    fn delete_still_asks_for_confirmation() {
        let (mut app, _) = test_app();
        let id = create(&mut app, "doomed");
        submit(&mut app, &format!("/delete {id}"));
        assert!(app.command_line.pending_confirm.is_some());

        press(&mut app, KeyCode::Enter);
        assert!(app.active_items.is_empty());
    }

    #[test]
    fn rejection_status_stays_until_the_next_key() {
        let (mut app, _) = test_app();
        submit(&mut app, "/task [[98]] [[97]] [[96]]");
        if let Some(msg) = app.status_message.as_mut() {
            msg.expires_at = std::time::Instant::now() - std::time::Duration::from_secs(1);
        }
        app.tick();
        assert!(
            app.status_message.is_some(),
            "sticky status expired on tick"
        );

        press(&mut app, KeyCode::Left);
        assert!(app.status_message.is_none());
    }

    #[test]
    fn rejection_messages_collapse_to_fit() {
        let rejections: Vec<Rejection> = ["[[98]]", "[[97]]", "[[96]]"]
            .iter()
            .map(|r| Rejection {
                reference: r.to_string(),
                reason: RejectReason::NotFound,
            })
            .collect();
        let wide = format_rejections(&rejections, 200);
        assert!(wide.contains("[[98]] not found") && wide.contains("[[96]] not found"));

        let narrow = format_rejections(&rejections, 30);
        assert!(narrow.contains("[[98]] not found"), "{narrow}");
        assert!(narrow.ends_with("+2 more"), "{narrow}");
    }

    #[test]
    fn rejected_note_edit_writes_nothing() {
        let (mut app, _) = test_app();
        let id = app
            .taskbook
            .create_note_with_body_direct(
                vec!["my board".into()],
                "Title".into(),
                Some("body".into()),
            )
            .unwrap();
        app.refresh_items().unwrap();

        let outcome = save_note_edit(
            &mut app,
            id,
            ("Title", Some("body")),
            "Title",
            Some("body [[99]]"),
        )
        .unwrap();
        assert!(outcome.is_err());
        let note = app.active_items[&id.to_string()].clone();
        assert_eq!(note.description(), "Title");
        assert_eq!(note.note_body(), Some("body"));
    }

    /// Covers AE2.
    #[test]
    fn note_edit_with_a_link_to_an_archived_item_saves() {
        let (mut app, _) = test_app();
        let target = create(&mut app, "gone");
        let tid = app.active_items[&target.to_string()].stable_id();
        let body = format!("see [[{tid}]]");
        let id = app
            .taskbook
            .create_note_with_body_direct(vec!["my board".into()], "N".into(), Some(body.clone()))
            .unwrap();
        app.taskbook.delete_items_silent(&[target]).unwrap();
        app.refresh_items().unwrap();

        let edited = format!("{body}\nmore");
        let outcome = save_note_edit(&mut app, id, ("N", Some(&body)), "N", Some(&edited)).unwrap();
        assert!(outcome.is_ok());
        assert_eq!(
            app.active_items[&id.to_string()].note_body(),
            Some(edited.as_str())
        );
    }
}
