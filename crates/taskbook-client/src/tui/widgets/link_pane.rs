use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::item_row::{broken_marker, divergent_marker};
use super::truncate_chars;
use crate::tui::app::{App, PaneEntry};
use taskbook_common::links::Target;

/// Indent before each entry.
const INDENT: &str = "  ";

/// Render the link side pane for the selected item.
pub fn render_link_pane(frame: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.pane.focused {
        app.theme.info
    } else {
        app.theme.border
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(" Links ", app.theme.title));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let content = app.pane_content();
    let cursor = app.pane_cursor(&content);
    let width = usize::from(inner.width);
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled("Links to", app.theme.header)));
    if content.outgoing.is_empty() {
        lines.push(Line::from(Span::styled("  (none)", app.theme.muted)));
    }
    for (i, entry) in content.outgoing.iter().enumerate() {
        lines.push(entry_line(
            app,
            entry,
            width,
            app.pane.focused && i == cursor,
        ));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Linked from", app.theme.header)));
    if content.divergent {
        lines.push(Line::from(Span::styled(
            "  ?? divergent",
            app.theme.link_divergent,
        )));
    } else if content.backlinks.is_empty() {
        lines.push(Line::from(Span::styled("  (none)", app.theme.muted)));
    }
    let offset = content.outgoing.len();
    for (i, entry) in content.backlinks.iter().enumerate() {
        let selected = app.pane.focused && offset + i == cursor;
        lines.push(entry_line(app, entry, width, selected));
    }

    if app.pane.focused {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "j/k move, Enter jump, Esc back",
            app.theme.muted,
        )));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

/// One entry: the title cut to fit with an ellipsis, then the short stable ID,
/// which always stays visible.
fn entry_line(app: &App, entry: &PaneEntry, width: usize, selected: bool) -> Line<'static> {
    let short = app.links.short_id(&entry.id).to_string();
    let (label, label_style, id_text, id_style) = match entry.target {
        Target::Active { .. } => (
            entry.title.clone(),
            Style::default(),
            short,
            app.theme.stable_id,
        ),
        Target::Missing => (
            String::new(),
            Style::default(),
            broken_marker(app, &entry.id),
            app.theme.link_broken,
        ),
        Target::Divergent => (
            String::new(),
            Style::default(),
            divergent_marker(app, &entry.id),
            app.theme.link_divergent,
        ),
    };

    // Indent, label, a space, then the ID.
    let room = width.saturating_sub(INDENT.len() + 1 + id_text.chars().count());
    let label = truncate_chars(&label, room);

    let mut line = Line::from(vec![
        Span::raw(INDENT),
        Span::styled(label, label_style),
        Span::raw(" "),
        Span::styled(id_text, id_style),
    ]);
    if selected {
        line = line.style(app.theme.selected);
    }
    line
}

#[cfg(test)]
mod tests {
    use crate::tui::app::tests::test_app;
    use crate::tui::app::App;
    use crate::tui::ui;
    use ratatui::{backend::TestBackend, Terminal};
    use taskbook_common::links::LinkIndex;
    use taskbook_common::{StorageItem, Task};

    fn screen(app: &mut App, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 20)).unwrap();
        terminal.draw(|f| ui::render(f, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn create(app: &mut App, desc: &str) -> u64 {
        let id = app
            .taskbook
            .create_task_direct_with_tags(vec!["my board".into()], desc.into(), 1, vec![], None)
            .unwrap();
        app.refresh_items().unwrap();
        id
    }

    /// Covers AE6.
    #[test]
    fn pane_shows_only_on_wide_terminals() {
        let (mut app, _) = test_app();
        create(&mut app, "x");
        assert!(!screen(&mut app, 80).contains("Linked from"));
        assert!(screen(&mut app, 120).contains("Linked from"));
    }

    #[test]
    fn empty_sections_say_none() {
        let (mut app, _) = test_app();
        create(&mut app, "x");
        let s = screen(&mut app, 120);
        assert!(s.contains("Links to"), "{s}");
        assert_eq!(s.matches("(none)").count(), 2, "{s}");
    }

    #[test]
    fn long_titles_truncate_but_keep_the_short_id() {
        let (mut app, _) = test_app();
        let long = create(&mut app, "An extremely long target title that cannot fit");
        let lid = app.active_items[&long.to_string()].stable_id();
        let src = create(&mut app, &format!("see [[{lid}]]"));
        app.selected_index = app.display_order.iter().position(|&i| i == src).unwrap();

        let s = screen(&mut app, 100);
        let short = app.links.short_id(&lid).to_string();
        let line = s.lines().find(|l| l.contains('…')).expect(&s);
        assert!(line.contains(&short), "{line}");
    }

    #[test]
    fn divergent_selection_shows_a_notice() {
        let (mut app, _) = test_app();
        let mut twin = Task::new(90, "Twin".into(), vec!["my board".into()], 1);
        twin.timestamp = 1;
        let mut twin2 = twin.clone();
        twin2.id = 91;
        for t in [twin, twin2] {
            app.active_items
                .insert(t.id.to_string(), StorageItem::Task(t));
        }
        app.items = app.active_items.clone();
        app.links = LinkIndex::build(app.active_items.values());
        app.update_display_order();

        assert!(screen(&mut app, 120).contains("?? divergent"));
    }
}
