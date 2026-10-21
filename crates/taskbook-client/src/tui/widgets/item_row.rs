use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::tui::app::App;
use taskbook_common::board;
use taskbook_common::due;
use taskbook_common::links::Segment;
use taskbook_common::StorageItem;

/// Width of the stable-ID field shown after the numeric ID.
const STABLE_ID_WIDTH: usize = 6;

/// Options for rendering an item row
pub struct ItemRowOptions {
    pub show_boards: bool,
    pub show_age: bool,
}

impl ItemRowOptions {
    pub fn for_board_view() -> Self {
        Self {
            show_boards: false,
            show_age: true,
        }
    }

    pub fn for_timeline_view() -> Self {
        Self {
            show_boards: true,
            show_age: false,
        }
    }
}

/// Render a single item as a Line with consistent styling
pub fn render_item_line(
    app: &App,
    item: &StorageItem,
    is_selected: bool,
    options: &ItemRowOptions,
) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();

    // Selection indicator + Item ID + stable ID
    if is_selected {
        spans.push(Span::styled(format!(" >{} ", item.id()), app.theme.info));
    } else {
        spans.push(Span::styled(format!("  {} ", item.id()), app.theme.item_id));
    }
    spans.extend(stable_id_spans(app, item));
    spans.push(Span::raw(" "));

    // Icon
    let (icon, icon_style) = if let Some(task) = item.as_task() {
        if task.is_complete {
            ("✔", app.theme.success)
        } else if task.in_progress {
            ("…", app.theme.warning)
        } else {
            ("☐", app.theme.pending)
        }
    } else {
        ("●", app.theme.info)
    };
    spans.push(Span::styled(format!("{} ", icon), icon_style));

    // Description
    let desc = item.description().to_string();
    let desc_style = if let Some(task) = item.as_task() {
        if task.is_complete {
            app.theme.completed_text
        } else if task.priority == 3 {
            app.theme.error.add_modifier(Modifier::BOLD)
        } else if task.priority == 2 {
            app.theme.warning
        } else {
            Style::default().fg(Color::White)
        }
    } else {
        Style::default().fg(Color::Rgb(200, 200, 220))
    };
    spans.extend(text_spans(app, &desc, desc_style));

    // Note body indicator
    if item.note_has_body() {
        spans.push(Span::styled(" [...]", app.theme.muted));
    }

    // Tags
    let tags = item.tags();
    if !tags.is_empty() {
        spans.push(Span::raw(" "));
        for (i, tag) in tags.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(tag.to_string(), app.theme.tag));
        }
    }

    // Boards (for timeline view)
    if options.show_boards {
        let boards: Vec<&String> = item
            .boards()
            .iter()
            .filter(|b| !board::board_eq(b, board::DEFAULT_BOARD))
            .collect();
        if !boards.is_empty() {
            spans.push(Span::raw(" "));
            for (i, b) in boards.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::styled(b.to_string(), app.theme.board_tag));
            }
        }
    }

    // Priority indicator
    if let Some(task) = item.as_task() {
        if task.priority == 2 {
            spans.push(Span::styled(" (!)", app.theme.warning));
        } else if task.priority == 3 {
            spans.push(Span::styled(" (!!)", app.theme.error));
        }
    }

    // Due date
    if let Some(task) = item.as_task() {
        if !task.is_complete {
            if let Some(millis) = task.due_date {
                let style = match due::due_status(millis) {
                    due::DueStatus::Overdue => app.theme.error,
                    due::DueStatus::DueToday => app.theme.warning,
                    due::DueStatus::Upcoming => app.theme.muted,
                };
                spans.push(Span::styled(
                    format!(" due:{}", due::format_due_date(millis)),
                    style,
                ));
            }
        }
    }

    // Star
    if item.is_starred() {
        spans.push(Span::styled(" ★", app.theme.starred));
    }

    // Age (for board view)
    if options.show_age {
        let age = calculate_age(item.timestamp());
        if !age.is_empty() {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(age, app.theme.muted));
        }
    }

    let mut line = Line::from(spans);
    if is_selected {
        line = line.style(app.theme.selected);
    }
    line
}

/// Spans for `text`, with `[[id]]` tokens rendered as link titles, broken
/// markers (`✗` + short ID) or divergent markers (short ID + `??`).
pub fn text_spans(app: &App, text: &str, base: Style) -> Vec<Span<'static>> {
    app.links
        .render(text)
        .into_iter()
        .map(|segment| match segment {
            Segment::Text(t) => Span::styled(t, base),
            Segment::Link { title, .. } => Span::styled(title, app.theme.link),
            Segment::Broken { id } => Span::styled(
                format!("✗{}", app.links.short_id(&id)),
                app.theme.link_broken,
            ),
            Segment::Divergent { id } => Span::styled(
                format!("{}??", app.links.short_id(&id)),
                app.theme.link_divergent,
            ),
        })
        .collect()
}

/// `text` as it renders, without styling (for matching and truncation).
pub fn plain_text(app: &App, text: &str) -> String {
    text_spans(app, text, Style::default())
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// The stable-ID field: the unique prefix highlighted, the rest of the first
/// six letters dimmed, and `??` when the ID is divergent.
pub fn stable_id_spans(app: &App, item: &StorageItem) -> Vec<Span<'static>> {
    let id = item.stable_id();
    let unique = app.links.prefix_len(&id).clamp(1, id.len());
    let shown = unique.max(STABLE_ID_WIDTH).min(id.len());
    let mut spans = vec![
        Span::styled(id[..unique].to_string(), app.theme.stable_id),
        Span::styled(id[unique..shown].to_string(), app.theme.stable_id_rest),
    ];
    if app.links.resolver().is_divergent(&id) {
        spans.push(Span::styled("??", app.theme.link_divergent));
    }
    spans
}

/// First line of a note body as rendered text, cut to 50 characters.
pub fn body_preview(app: &App, body: &str) -> String {
    let Some(first_line) = body.lines().next() else {
        return String::new();
    };
    let rendered = plain_text(app, first_line);
    if rendered.chars().count() > 50 {
        format!("  {}…", rendered.chars().take(50).collect::<String>())
    } else if body.lines().count() > 1 {
        format!("  {}…", rendered)
    } else {
        format!("  {}", rendered)
    }
}

fn calculate_age(timestamp: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let diff = now - timestamp;
    let days = diff / (1000 * 60 * 60 * 24);

    if days == 0 {
        String::new()
    } else if days == 1 {
        "1d".to_string()
    } else if days < 30 {
        format!("{}d", days)
    } else if days < 365 {
        format!("{}mo", days / 30)
    } else {
        format!("{}y", days / 365)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::tests::test_app;
    use taskbook_common::links::LinkIndex;
    use taskbook_common::{Note, Task};

    fn line_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn task(id: u64, ts: i64, desc: &str) -> StorageItem {
        let mut t = Task::new(id, desc.to_string(), vec!["my board".into()], 1);
        t.timestamp = ts;
        StorageItem::Task(t)
    }

    fn app_with(items: Vec<StorageItem>) -> crate::tui::app::App {
        let (mut app, _) = test_app();
        app.active_items = items.into_iter().map(|i| (i.id().to_string(), i)).collect();
        app.links = LinkIndex::build(app.active_items.values());
        app.items = app.active_items.clone();
        app
    }

    #[test]
    fn link_renders_as_underlined_title() {
        let target_id = taskbook_common::stable_id::from_timestamp(2_000);
        let app = app_with(vec![
            task(1, 1_000, &format!("see [[{target_id}]]")),
            task(2, 2_000, "Milk run"),
        ]);
        let line = render_item_line(
            &app,
            &app.items["1"],
            false,
            &ItemRowOptions::for_board_view(),
        );
        let text = line_text(&line);

        assert!(text.contains("see Milk run"), "{text}");
        assert!(!text.contains(&target_id), "{text}");
        let link = line.spans.iter().find(|s| s.content == "Milk run").unwrap();
        assert!(link.style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn broken_and_divergent_links_carry_glyphs() {
        let missing = taskbook_common::stable_id::from_timestamp(9_999);
        let shared = taskbook_common::stable_id::from_timestamp(3_000);
        let app = app_with(vec![
            task(1, 1_000, &format!("a [[{missing}]] b [[{shared}]]")),
            task(2, 3_000, "twin one"),
            StorageItem::Note({
                let mut n = Note::new(3, "twin two".into(), vec!["my board".into()]);
                n.timestamp = 3_000;
                n
            }),
        ]);
        let text = line_text(&render_item_line(
            &app,
            &app.items["1"],
            false,
            &ItemRowOptions::for_board_view(),
        ));
        assert!(
            text.contains(&format!("✗{}", app.links.short_id(&missing))),
            "{text}"
        );
        assert!(
            text.contains(&format!("{}??", app.links.short_id(&shared))),
            "{text}"
        );
    }

    #[test]
    fn row_shows_a_six_character_stable_id_after_the_numeric_id() {
        let app = app_with(vec![task(1, 1_000, "x")]);
        let line = render_item_line(
            &app,
            &app.items["1"],
            false,
            &ItemRowOptions::for_board_view(),
        );
        let sid = taskbook_common::stable_id::from_timestamp(1_000);
        assert!(
            line_text(&line).starts_with(&format!("  1 {} ", &sid[..6])),
            "{}",
            line_text(&line)
        );
    }

    #[test]
    fn plain_rendering_and_preview_handle_multibyte_text() {
        let app = app_with(vec![]);
        let body = "ünïcödé ".repeat(10);
        let preview = body_preview(&app, &body);
        assert!(preview.chars().count() <= 53, "{preview}");
        assert!(preview.ends_with('…'));
    }
}
