use super::app::{App, Suggestion, SuggestionKind};
use super::widgets::item_row::plain_text;
use super::widgets::truncate_chars;
use taskbook_common::board;

/// Static list of all slash commands with descriptions
const COMMANDS: &[(&str, &str)] = &[
    ("task", "Create a new task"),
    ("note", "Create a new note"),
    ("edit", "Edit item description"),
    ("move", "Move item to board"),
    ("delete", "Delete items"),
    ("search", "Search/filter items"),
    ("priority", "Set task priority"),
    ("check", "Toggle task check"),
    ("star", "Toggle star"),
    ("begin", "Toggle in-progress"),
    ("tag", "Add/remove tags on item"),
    ("clear", "Clear completed tasks"),
    ("rename-board", "Rename a board"),
    ("board", "Switch to board view"),
    ("timeline", "Switch to timeline view"),
    ("archive", "Switch to archive view"),
    ("journal", "Switch to journal view"),
    ("sort", "Cycle sort method"),
    ("hide-done", "Toggle hide completed"),
    ("help", "Show help"),
    ("quit", "Quit application"),
];

/// Commands that accept item ID references (@<id>)
const ITEM_COMMANDS: &[&str] = &[
    "check", "star", "begin", "delete", "edit", "move", "priority", "tag",
];

const MAX_SUGGESTIONS: usize = 8;

/// Update suggestions based on current command line input
pub fn update_suggestions(app: &mut App) {
    app.command_line.suggestions.clear();
    app.command_line.selected_suggestion = None;

    let input = &app.command_line.input;
    if input.is_empty() || !input.starts_with('/') {
        return;
    }

    // An unclosed `[[` in a description suggests link targets, unless the
    // user dismissed the dropdown for this `[[` with Esc.
    match open_link_query(app) {
        Some(link) => {
            if app.command_line.link_dismissed_at != Some(link.start) {
                app.command_line.link_dismissed_at = None;
                suggest_links(app, &link);
            }
            return;
        }
        None => app.command_line.link_dismissed_at = None,
    }
    let input = &app.command_line.input;

    // Find the token being typed by scanning backward from cursor
    let chars: Vec<char> = input.chars().collect();
    let cursor = app.command_line.cursor.min(chars.len());

    // Get the text up to the cursor for analysis
    let text_to_cursor: String = chars[..cursor].iter().collect();

    // Check what context we're in
    if !text_to_cursor.contains(' ') {
        // Still typing the command name (e.g., "/ta")
        let partial = &text_to_cursor[1..]; // skip the '/'
        suggest_commands(app, partial);
    } else {
        // We're past the command name — determine context
        let space_pos = text_to_cursor.find(' ').unwrap();
        let command = &text_to_cursor[1..space_pos]; // skip '/'

        // Find the last token start (use char index, not byte index)
        let last_space = chars[..cursor].iter().rposition(|c| *c == ' ').unwrap();
        let last_token: String = chars[last_space + 1..cursor].iter().collect();

        if let Some(after_at) = last_token.strip_prefix('@') {
            // If it's @<digits>, user is typing an ID directly — no suggestions
            if !after_at.is_empty() && after_at.chars().all(|c| c.is_ascii_digit()) {
                return;
            }
            // Otherwise it's a board reference
            suggest_boards(app, after_at);
        } else if ITEM_COMMANDS.contains(&command) {
            // Check if we should suggest items for this argument position
            if should_suggest_items(command, &text_to_cursor, last_space) {
                suggest_items(app, &last_token);
            }
        }
    }
}

/// An unclosed `[[` before the cursor in the description of a command
pub struct OpenLink {
    /// Char index of the `[[` in the input
    pub start: usize,
    /// Text typed after the `[[`
    pub query: String,
}

/// The unclosed `[[` before the cursor in the description of `/task`,
/// `/note` or `/edit`, if any.
pub fn open_link_query(app: &App) -> Option<OpenLink> {
    let input = &app.command_line.input;
    if !input.starts_with('/') {
        return None;
    }
    let cursor = app.command_line.cursor.min(input.chars().count());
    let before: String = input.chars().take(cursor).collect();
    let (command, rest) = before[1..].split_once(' ')?;
    let in_description = match command {
        "task" | "note" => true,
        // /edit @<id> <description>
        "edit" => rest.trim_start().split_once(' ').is_some(),
        _ => false,
    };
    if !in_description {
        return None;
    }
    let open_byte = before.rfind("[[")?;
    let query = &before[open_byte + 2..];
    if query.contains("]]") {
        return None;
    }
    Some(OpenLink {
        start: before[..open_byte].chars().count(),
        query: query.to_string(),
    })
}

/// A link target offered by `[[` autocomplete
struct LinkCandidate {
    on_current_board: bool,
    timestamp: i64,
    id: String,
    title: String,
    board: Option<String>,
}

/// Suggest active, non-divergent items to link to, newest first with the
/// current board's items ahead; accepting one inserts `[[<full id>]]`.
fn suggest_links(app: &mut App, link: &OpenLink) {
    let query = link.query.to_lowercase();
    let input_chars: Vec<char> = app.command_line.input.chars().collect();
    let cursor = app.command_line.cursor.min(input_chars.len());

    // The item being edited cannot link to itself through the picker.
    let editing: Option<u64> = app
        .command_line
        .input
        .strip_prefix("/edit @")
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|id| id.parse().ok());
    let current_board = app
        .filter
        .board_filter
        .clone()
        .or_else(|| app.get_board_for_selected());
    let on_board = |boards: &[String]| {
        current_board
            .as_ref()
            .is_some_and(|cb| boards.iter().any(|b| board::board_eq(b, cb)))
    };

    let mut candidates: Vec<LinkCandidate> = app
        .active_items
        .values()
        .filter(|item| Some(item.id()) != editing)
        .filter_map(|item| {
            let id = item.stable_id();
            if app.links.resolver().is_divergent(&id) {
                return None;
            }
            let title = plain_text(app, item.description());
            let matches = query.is_empty()
                || title.to_lowercase().contains(&query)
                || item.id().to_string().starts_with(&query)
                || id.starts_with(&query);
            matches.then(|| LinkCandidate {
                on_current_board: on_board(item.boards()),
                timestamp: item.timestamp(),
                id,
                title,
                board: item.boards().first().cloned(),
            })
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.on_current_board
            .cmp(&a.on_current_board)
            .then(b.timestamp.cmp(&a.timestamp))
    });

    let before_open: String = input_chars[..link.start].iter().collect();
    let after_cursor: String = input_chars[cursor..].iter().collect();
    for candidate in candidates.into_iter().take(MAX_SUGGESTIONS) {
        let display = format!(
            "{} {}",
            truncate_chars(&candidate.title, 28),
            app.links.short_id(&candidate.id)
        );
        app.command_line.suggestions.push(Suggestion {
            display,
            completion: format!("{}[[{}]]{}", before_open, candidate.id, after_cursor),
            description: Some(
                candidate
                    .board
                    .map(|b| board::display_name(&b))
                    .unwrap_or_default(),
            ),
            kind: SuggestionKind::Link,
        });
    }
    if !app.command_line.suggestions.is_empty() {
        app.command_line.selected_suggestion = Some(0);
    }
}

/// Determine whether the current argument position should get item suggestions
fn should_suggest_items(command: &str, text_to_cursor: &str, _last_space: usize) -> bool {
    let after_command = text_to_cursor
        .split_once(' ')
        .map(|(_, rest)| rest)
        .unwrap_or("");
    let args: Vec<&str> = after_command.split_whitespace().collect();

    match command {
        // /edit @<id> <description> — only suggest for the first argument
        "edit" => args.len() <= 1,
        // /move @<id> @<board> — only suggest for the first argument
        "move" => args.len() <= 1,
        // /priority @<id> <1-3> — only suggest for the first argument
        "priority" => args.len() <= 1,
        // /tag @<id> +tag1 -tag2 — only suggest for the first argument
        "tag" => args.len() <= 1,
        // Multi-ID commands: check, star, begin, delete — always suggest
        _ => true,
    }
}

fn suggest_commands(app: &mut App, partial: &str) {
    let partial_lower = partial.to_lowercase();
    for (name, desc) in COMMANDS {
        if name.starts_with(&partial_lower) {
            app.command_line.suggestions.push(Suggestion {
                display: format!("/{}", name),
                completion: format!("/{} ", name),
                description: Some(desc.to_string()),
                kind: SuggestionKind::Command,
            });
            if app.command_line.suggestions.len() >= MAX_SUGGESTIONS {
                break;
            }
        }
    }
}

fn suggest_boards(app: &mut App, partial: &str) {
    let partial_lower = partial.to_lowercase();
    for b in &app.boards.clone() {
        let display = board::display_name(b);
        if display.to_lowercase().starts_with(&partial_lower)
            || b.to_lowercase().starts_with(&partial_lower)
        {
            // Build the completion: replace the @partial with @board
            // Quote board names that contain spaces
            let input_chars: Vec<char> = app.command_line.input.chars().collect();
            let cursor = app.command_line.cursor.min(input_chars.len());

            // Find the @ position (use char index, not byte index)
            if let Some(at_pos) = input_chars[..cursor].iter().rposition(|c| *c == '@') {
                let before_at: String = input_chars[..at_pos].iter().collect();
                let after_cursor: String = input_chars[cursor..].iter().collect();
                let board_ref = if b.contains(' ') {
                    format!("@\"{}\"", b)
                } else {
                    format!("@{}", b)
                };
                let completion = format!("{}{} {}", before_at, board_ref, after_cursor);

                app.command_line.suggestions.push(Suggestion {
                    display: format!("@{}", display),
                    completion,
                    description: None,
                    kind: SuggestionKind::Board,
                });
            }

            if app.command_line.suggestions.len() >= MAX_SUGGESTIONS {
                break;
            }
        }
    }
}

fn suggest_items(app: &mut App, partial: &str) {
    if partial.is_empty() {
        return;
    }

    let partial_lower = partial.to_lowercase();
    let input_chars: Vec<char> = app.command_line.input.chars().collect();
    let cursor = app.command_line.cursor.min(input_chars.len());
    let last_space = input_chars[..cursor]
        .iter()
        .rposition(|c| *c == ' ')
        .unwrap_or(0);

    // Collect and sort by ID for stable ordering; match the text as it
    // renders, so link tokens are searched by their titles
    let mut matches: Vec<(u64, String, bool, bool, bool)> = app
        .items
        .values()
        .map(|item| (item, plain_text(app, item.description())))
        .filter(|(_, desc)| desc.to_lowercase().contains(&partial_lower))
        .map(|(item, desc)| {
            let (is_complete, in_progress) = match item {
                taskbook_common::StorageItem::Task(t) => (t.is_complete, t.in_progress),
                taskbook_common::StorageItem::Note(_) => (false, false),
            };
            (item.id(), desc, item.is_task(), is_complete, in_progress)
        })
        .collect();
    matches.sort_by_key(|(id, _, _, _, _)| *id);

    for (id, desc, is_task, is_complete, in_progress) in matches {
        // Build status description
        let type_label = if is_task { "task" } else { "note" };
        let status = if is_complete {
            format!("{} · done", type_label)
        } else if in_progress {
            format!("{} · in-progress", type_label)
        } else {
            type_label.to_string()
        };

        // Truncate description for display
        let display = truncate_chars(&desc, 35);

        // Build completion: replace the partial token with @<id>
        let before_token: String = input_chars[..last_space + 1].iter().collect();
        let after_cursor: String = input_chars[cursor..].iter().collect();
        let completion = format!("{}@{} {}", before_token, id, after_cursor);

        app.command_line.suggestions.push(Suggestion {
            display,
            completion,
            description: Some(status),
            kind: SuggestionKind::Item,
        });

        if app.command_line.suggestions.len() >= MAX_SUGGESTIONS {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::actions::handle_key_event;
    use crate::tui::app::tests::{key, test_app};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use taskbook_common::links::LinkIndex;
    use taskbook_common::{StorageItem, Task};

    fn press(app: &mut App, code: KeyCode) {
        handle_key_event(app, KeyEvent::new(code, KeyModifiers::NONE)).unwrap();
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            handle_key_event(app, key(c)).unwrap();
        }
    }

    fn create(app: &mut App, desc: &str) -> u64 {
        let id = app
            .taskbook
            .create_task_direct_with_tags(vec!["my board".into()], desc.into(), 1, vec![], None)
            .unwrap();
        app.refresh_items().unwrap();
        id
    }

    fn sid(app: &App, id: u64) -> String {
        app.active_items[&id.to_string()].stable_id()
    }

    fn displays(app: &App) -> Vec<String> {
        app.command_line
            .suggestions
            .iter()
            .map(|s| s.display.clone())
            .collect()
    }

    /// Covers F1.
    #[test]
    fn bracket_query_suggests_by_title_and_inserts_the_full_token() {
        let (mut app, _) = test_app();
        let milk = create(&mut app, "Milk run");
        create(&mut app, "Other");
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "task buy [[mil");

        assert_eq!(
            app.command_line.suggestions.len(),
            1,
            "{:?}",
            displays(&app)
        );
        assert!(app.command_line.suggestions[0].display.contains("Milk run"));
        assert_eq!(app.command_line.selected_suggestion, Some(0));

        press(&mut app, KeyCode::Tab);
        assert_eq!(
            app.command_line.input,
            format!("/task buy [[{}]] ", sid(&app, milk)).trim_end()
        );
    }

    #[test]
    fn suggestions_match_numeric_id_and_stable_prefix() {
        let (mut app, _) = test_app();
        let a = create(&mut app, "Alpha");
        let b = create(&mut app, "Beta");
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, &format!("task x [[{b}"));
        assert!(displays(&app).iter().any(|d| d.contains("Beta")));

        let prefix: String = sid(&app, a).chars().take(4).collect();
        app.activate_command_line("/");
        type_text(&mut app, &format!("task x [[{prefix}"));
        assert!(
            displays(&app).iter().any(|d| d.contains("Alpha")),
            "{:?}",
            displays(&app)
        );
    }

    #[test]
    fn suggestions_exclude_the_edited_item_and_divergent_items() {
        let (mut app, _) = test_app();
        let edited = create(&mut app, "Self");
        create(&mut app, "Kept");
        // Two items sharing a timestamp are divergent.
        let mut twin = Task::new(90, "Twin".into(), vec!["my board".into()], 1);
        twin.timestamp = 1;
        let mut twin2 = twin.clone();
        twin2.id = 91;
        app.active_items
            .insert("90".into(), StorageItem::Task(twin));
        app.active_items
            .insert("91".into(), StorageItem::Task(twin2));
        app.links = LinkIndex::build(app.active_items.values());

        app.activate_command_line(&format!("/edit @{edited} x [["));
        update_suggestions(&mut app);
        let shown = displays(&app);
        assert!(shown.iter().any(|d| d.contains("Kept")), "{shown:?}");
        assert!(
            !shown
                .iter()
                .any(|d| d.contains("Self") || d.contains("Twin")),
            "{shown:?}"
        );
    }

    #[test]
    fn empty_query_lists_recent_items_with_the_first_selected() {
        let (mut app, _) = test_app();
        create(&mut app, "Older");
        create(&mut app, "Newer");
        app.activate_command_line("/task x [[");
        update_suggestions(&mut app);
        assert!(displays(&app)[0].contains("Newer"), "{:?}", displays(&app));
        assert_eq!(app.command_line.selected_suggestion, Some(0));
    }

    #[test]
    fn enter_with_an_open_dropdown_accepts_instead_of_submitting() {
        let (mut app, _) = test_app();
        let milk = create(&mut app, "Milk run");
        app.activate_command_line("/task buy [[mil");
        update_suggestions(&mut app);
        press(&mut app, KeyCode::Enter);

        assert!(app.command_line.focused);
        assert!(app.command_line.input.contains(&sid(&app, milk)));
        assert_eq!(app.active_items.len(), 1, "no task was created");
    }

    /// Covers F2.
    #[test]
    fn link_key_opens_an_edit_with_suggestions() {
        let (mut app, _) = test_app();
        create(&mut app, "Target");
        let id = create(&mut app, "Source");
        app.selected_index = app.display_order.iter().position(|&i| i == id).unwrap();
        handle_key_event(&mut app, key('l')).unwrap();

        assert_eq!(app.command_line.input, format!("/edit @{id} Source [["));
        assert!(displays(&app).iter().any(|d| d.contains("Target")));
    }

    #[test]
    fn enter_with_no_match_does_not_save_a_stray_bracket() {
        let (mut app, _) = test_app();
        let id = create(&mut app, "Source");
        app.selected_index = 0;
        handle_key_event(&mut app, key('l')).unwrap();
        type_text(&mut app, "zzzz");
        assert!(app.command_line.suggestions.is_empty());
        press(&mut app, KeyCode::Enter);

        assert!(app.command_line.focused);
        assert_eq!(app.active_items[&id.to_string()].description(), "Source");
        let hint = app
            .status_message
            .as_ref()
            .map(|m| m.text.clone())
            .unwrap_or_default();
        assert!(hint.contains("Esc"), "{hint}");
    }

    #[test]
    fn esc_closes_the_dropdown_then_the_command_line() {
        let (mut app, _) = test_app();
        create(&mut app, "Target");
        app.activate_command_line("/task x [[");
        update_suggestions(&mut app);
        assert!(!app.command_line.suggestions.is_empty());

        press(&mut app, KeyCode::Esc);
        assert!(app.command_line.focused);
        assert!(app.command_line.suggestions.is_empty());
        // Typing inside the same [[ keeps it closed.
        type_text(&mut app, "t");
        assert!(app.command_line.suggestions.is_empty());

        press(&mut app, KeyCode::Esc);
        assert!(!app.command_line.focused);
    }
}
