use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::{Config, SortMethod};
use crate::error::Result;
use crate::render::Stats;
use crate::taskbook::Taskbook;
use taskbook_common::board;
use taskbook_common::limits::SizeNotice;
use taskbook_common::links::{self, LinkIndex, Target};
use taskbook_common::StorageItem;

/// Sort items by the given method
pub fn sort_items_by(items: &mut [&StorageItem], method: SortMethod) {
    match method {
        SortMethod::Id => {
            items.sort_by_key(|item| item.id());
        }
        SortMethod::Priority => {
            items.sort_by(|a, b| {
                let pa = a.as_task().map(|t| t.priority).unwrap_or(0);
                let pb = b.as_task().map(|t| t.priority).unwrap_or(0);
                pb.cmp(&pa).then_with(|| a.id().cmp(&b.id()))
            });
        }
        SortMethod::Status => {
            items.sort_by(|a, b| {
                let status_rank = |item: &StorageItem| -> u8 {
                    if let Some(task) = item.as_task() {
                        if task.is_complete {
                            2
                        } else if task.in_progress {
                            1
                        } else {
                            0 // pending first
                        }
                    } else {
                        3 // notes last
                    }
                };
                status_rank(a)
                    .cmp(&status_rank(b))
                    .then_with(|| a.id().cmp(&b.id()))
            });
        }
    }
}

use super::theme::TuiTheme;
use super::widgets::item_row::plain_text;

/// The link pane is only shown on terminals at least this wide.
pub const PANE_MIN_WIDTH: u16 = 100;

/// Main application state
pub struct App {
    /// Core taskbook instance for business logic
    pub taskbook: Taskbook,
    /// Current view mode
    pub view: ViewMode,
    /// Currently selected item index (global flat index)
    pub selected_index: usize,
    /// List of boards for navigation
    pub boards: Vec<String>,
    /// Items shown by the current view (the archive in Archive view)
    pub items: HashMap<String, StorageItem>,
    /// Active items, loaded in every view; the link index is built from these
    pub active_items: HashMap<String, StorageItem>,
    /// Links and backlinks among the active items
    pub links: LinkIndex,
    /// Active popup/dialog state (Help only)
    pub popup: Option<PopupState>,
    /// Command line state
    pub command_line: CommandLineState,
    /// Status message (success/error feedback)
    pub status_message: Option<StatusMessage>,
    /// Filter state
    pub filter: FilterState,
    /// Application running flag
    pub running: bool,
    /// Theme colors for rendering
    pub theme: TuiTheme,
    /// Configuration
    pub config: Config,
    /// Current sort method for items within boards
    pub sort_method: SortMethod,
    /// Flat list of item IDs in display order (for navigation)
    pub display_order: Vec<u64>,
    /// Cached statistics (recalculated on refresh)
    cached_stats: Stats,
    /// Flag to request a full terminal redraw (e.g. after suspend/resume)
    pub needs_full_redraw: bool,
    /// Last known content area height (updated each render frame)
    pub content_height: u16,
    /// Last known status line width (updated each render frame)
    pub status_width: u16,
    /// Command history (most recent last)
    pub command_history: Vec<String>,
    /// Current position when browsing history (None = not browsing)
    pub history_index: Option<usize>,
    /// Saved input before browsing history
    pub history_saved_input: String,
    /// Where config changes are written (`None` = the user's config file)
    config_path: Option<PathBuf>,
    /// Link side pane state
    pub pane: PaneState,
    /// Archived item count for the size notice. Read at startup and whenever
    /// Archive view loads the archive, so it can lag deletes made since.
    pub archived_count: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    #[default]
    Board,
    Timeline,
    Archive,
    Journal,
}

#[derive(Debug, Clone)]
pub enum PopupState {
    Help { scroll: u16 },
}

/// Command line state for the bottom input bar
#[derive(Debug, Clone, Default)]
pub struct CommandLineState {
    /// Current input text
    pub input: String,
    /// Cursor position (character index)
    pub cursor: usize,
    /// Whether the command line is focused/active
    pub focused: bool,
    /// Autocomplete suggestions
    pub suggestions: Vec<Suggestion>,
    /// Currently selected suggestion index
    pub selected_suggestion: Option<usize>,
    /// Pending confirmation action
    pub pending_confirm: Option<PendingAction>,
    /// Stored text of the item being edited when this edit session began,
    /// so retries after a rejected save diff against the same baseline
    pub link_baseline: Option<EditBaseline>,
    /// Char index of the `[[` whose link dropdown the user closed with Esc
    pub link_dismissed_at: Option<usize>,
}

/// The stored text of an item when an edit session began
#[derive(Debug, Clone)]
pub struct EditBaseline {
    pub id: u64,
    pub text: String,
}

/// An autocomplete suggestion
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// Display text shown in the dropdown
    pub display: String,
    /// Text to insert when accepted
    pub completion: String,
    /// Optional description shown dimmed
    pub description: Option<String>,
    /// Kind of suggestion for styling
    pub kind: SuggestionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionKind {
    Command,
    Board,
    Item,
    /// A `[[` link target
    Link,
}

/// An action waiting for confirmation
#[derive(Debug, Clone)]
pub enum PendingAction {
    Delete { ids: Vec<u64> },
    Clear,
}

#[derive(Debug, Clone, Default)]
pub struct FilterState {
    #[allow(dead_code)]
    pub attributes: Vec<String>,
    pub search_term: Option<String>,
    /// Filter to show only items from this board
    pub board_filter: Option<String>,
    /// Hide completed tasks
    pub hide_completed: bool,
    /// A completed task shown despite `hide_completed`, after a jump to it
    pub revealed_completed: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub kind: StatusKind,
    pub expires_at: Instant,
    /// Stays until the next key press instead of expiring
    pub sticky: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum StatusKind {
    Success,
    Error,
    Info,
}

impl App {
    pub fn new(taskbook_dir: Option<&Path>) -> Result<Self> {
        let taskbook = Taskbook::new(taskbook_dir)?;
        let config = Config::load_or_default();
        Self::build(taskbook, config, None)
    }

    /// Build an app over an explicit store, writing config changes to
    /// `config_path` instead of the user's config file (for tests).
    #[cfg(test)]
    pub fn with_taskbook(taskbook: Taskbook, config: Config, config_path: PathBuf) -> Result<Self> {
        Self::build(taskbook, config, Some(config_path))
    }

    fn build(taskbook: Taskbook, config: Config, config_path: Option<PathBuf>) -> Result<Self> {
        let theme = TuiTheme::from(&config.theme.resolve());

        let initial_view = config.default_view;

        let mut app = Self {
            taskbook,
            view: initial_view,
            selected_index: 0,
            boards: Vec::new(),
            items: HashMap::new(),
            active_items: HashMap::new(),
            links: LinkIndex::default(),
            popup: None,
            command_line: CommandLineState::default(),
            status_message: None,
            filter: FilterState {
                hide_completed: !config.display_complete_tasks,
                ..Default::default()
            },
            running: true,
            theme,
            sort_method: config.sort_method,
            config,
            display_order: Vec::new(),
            needs_full_redraw: false,
            content_height: 20,
            status_width: 80,
            command_history: Vec::new(),
            history_index: None,
            history_saved_input: String::new(),
            config_path,
            pane: PaneState {
                visible: true,
                focused: false,
                cursor: 0,
            },
            archived_count: 0,
            cached_stats: Stats {
                percent: 0,
                complete: 0,
                in_progress: 0,
                pending: 0,
                notes: 0,
            },
        };

        app.refresh_items()?;
        if app.view != ViewMode::Archive {
            app.archived_count = app.taskbook.get_all_archive_items()?.len();
        }

        Ok(app)
    }

    /// Refresh items from storage and rebuild the link index
    pub fn refresh_items(&mut self) -> Result<()> {
        self.active_items = self.taskbook.get_all_items()?;
        self.links = LinkIndex::build(self.active_items.values());
        self.boards = self.taskbook.boards_of(&self.active_items);
        self.items = if self.view == ViewMode::Archive {
            let archive = self.taskbook.get_all_archive_items()?;
            self.archived_count = archive.len();
            archive
        } else {
            self.active_items.clone()
        };
        self.update_display_order();
        self.recalculate_stats();

        // Clamp selection to valid range
        if !self.display_order.is_empty() && self.selected_index >= self.display_order.len() {
            self.selected_index = self.display_order.len() - 1;
        }

        Ok(())
    }

    /// Header text when the store is past the size taskbook is designed for,
    /// suggesting `/clear` only when there are completed tasks to archive.
    pub fn size_notice(&self) -> Option<String> {
        let notice = SizeNotice::check(self.active_items.len(), self.archived_count)?;
        let completed = self
            .active_items
            .values()
            .filter(|item| item.as_task().is_some_and(|task| task.is_complete))
            .count();
        let mut text = notice.summary();
        if notice.active_over_recommended() && completed > 0 {
            text.push_str(&format!(" · /clear archives {completed} done"));
        }
        Some(text)
    }

    /// Recalculate cached statistics
    fn recalculate_stats(&mut self) {
        let mut complete = 0;
        let mut in_progress = 0;
        let mut pending = 0;
        let mut notes = 0;

        for item in self.items.values() {
            if let Some(task) = item.as_task() {
                if task.is_complete {
                    complete += 1;
                } else if task.in_progress {
                    in_progress += 1;
                } else {
                    pending += 1;
                }
            } else {
                notes += 1;
            }
        }

        let total = complete + pending + in_progress;
        let percent = (complete * 100usize).checked_div(total).unwrap_or(0) as u32;

        self.cached_stats = Stats {
            percent,
            complete,
            in_progress,
            pending,
            notes,
        };
    }

    /// Check if an item should be shown based on current filters
    pub fn should_show_item(&self, item: &StorageItem) -> bool {
        if self.filter.hide_completed && self.filter.revealed_completed != Some(item.id()) {
            if let Some(task) = item.as_task() {
                if task.is_complete {
                    return false;
                }
            }
        }
        if let Some(ref term) = self.filter.search_term {
            let term_lower = term.to_lowercase();
            let in_description = item.description().to_lowercase().contains(&term_lower);
            let in_body = item
                .note_body()
                .map(|b| b.to_lowercase().contains(&term_lower))
                .unwrap_or(false);
            let in_tags = item
                .tags()
                .iter()
                .any(|t| t.to_lowercase().contains(&term_lower));
            if !in_description && !in_body && !in_tags {
                return false;
            }
        }
        true
    }

    /// Update the flat display order of items
    pub fn update_display_order(&mut self) {
        self.display_order.clear();

        match self.view {
            ViewMode::Board => {
                // If filtering by board, only show that board
                let boards_to_show: Vec<String> =
                    if let Some(ref filter_board) = self.filter.board_filter {
                        vec![filter_board.clone()]
                    } else {
                        self.boards.clone()
                    };

                // Order by board, then by ID within each board
                for board in &boards_to_show {
                    let mut board_items: Vec<_> = self
                        .items
                        .values()
                        .filter(|item| {
                            item.boards().iter().any(|b| board::board_eq(b, board))
                                && self.should_show_item(item)
                        })
                        .collect();
                    sort_items_by(&mut board_items, self.sort_method);
                    for item in board_items {
                        if !self.display_order.contains(&item.id()) {
                            self.display_order.push(item.id());
                        }
                    }
                }
            }
            ViewMode::Timeline | ViewMode::Archive => {
                // Order by date (newest first), then by ID
                let mut items: Vec<_> = self
                    .items
                    .values()
                    .filter(|item| self.should_show_item(item))
                    .collect();
                items.sort_by(|a, b| {
                    b.timestamp()
                        .cmp(&a.timestamp())
                        .then_with(|| a.id().cmp(&b.id()))
                });
                for item in items {
                    self.display_order.push(item.id());
                }
            }
            ViewMode::Journal => {
                // Order by date (newest first like timeline), then by ID
                let mut items: Vec<_> = self
                    .items
                    .values()
                    .filter(|item| self.should_show_item(item))
                    .collect();
                items.sort_by(|a, b| {
                    b.timestamp()
                        .cmp(&a.timestamp())
                        .then_with(|| a.id().cmp(&b.id()))
                });
                for item in items {
                    self.display_order.push(item.id());
                }
            }
        }
    }

    /// Cycle through sort methods and persist to config
    pub fn cycle_sort_method(&mut self) {
        self.sort_method = self.sort_method.next();
        self.config.sort_method = self.sort_method;
        self.save_config();
        self.update_display_order();
    }

    /// Toggle hide completed tasks
    pub fn toggle_hide_completed(&mut self) {
        self.filter.hide_completed = !self.filter.hide_completed;
        self.filter.revealed_completed = None;
        self.config.display_complete_tasks = !self.filter.hide_completed;
        self.save_config();
        self.update_display_order();
        // Clamp selection
        if !self.display_order.is_empty() && self.selected_index >= self.display_order.len() {
            self.selected_index = self.display_order.len().saturating_sub(1);
        }
    }

    /// Get the board that the currently selected item belongs to
    pub fn get_board_for_selected(&self) -> Option<String> {
        self.selected_item()
            .and_then(|item| item.boards().first().cloned())
    }

    /// Set board filter
    pub fn set_board_filter(&mut self, board: Option<String>) {
        self.filter.board_filter = board;
        self.selected_index = 0;
        self.update_display_order();
    }

    /// Clear board filter
    pub fn clear_board_filter(&mut self) {
        self.filter.board_filter = None;
        self.selected_index = 0;
        self.update_display_order();
    }

    /// Get the currently selected item ID
    pub fn selected_id(&self) -> Option<u64> {
        self.display_order.get(self.selected_index).copied()
    }

    /// Get the currently selected item
    pub fn selected_item(&self) -> Option<&StorageItem> {
        self.selected_id()
            .and_then(|id| self.items.get(&id.to_string()))
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if self.selected_index + 1 < self.display_order.len() {
            self.selected_index += 1;
        }
    }

    /// Go to first item
    pub fn select_first(&mut self) {
        self.selected_index = 0;
    }

    /// Go to last item
    pub fn select_last(&mut self) {
        if !self.display_order.is_empty() {
            self.selected_index = self.display_order.len() - 1;
        }
    }

    /// Move selection up by n items
    pub fn select_up_by(&mut self, n: usize) {
        self.selected_index = self.selected_index.saturating_sub(n);
    }

    /// Move selection down by n items
    pub fn select_down_by(&mut self, n: usize) {
        if !self.display_order.is_empty() {
            self.selected_index = (self.selected_index + n).min(self.display_order.len() - 1);
        }
    }

    /// Set status message
    pub fn set_status(&mut self, text: String, kind: StatusKind) {
        self.status_message = Some(StatusMessage {
            text,
            kind,
            expires_at: Instant::now() + Duration::from_secs(3),
            sticky: false,
        });
    }

    /// Set a status message that stays until the next key press
    pub fn set_sticky_status(&mut self, text: String, kind: StatusKind) {
        self.set_status(text, kind);
        if let Some(msg) = self.status_message.as_mut() {
            msg.sticky = true;
        }
    }

    /// Tick - called periodically for time-based updates
    pub fn tick(&mut self) {
        // Clear expired status messages
        if let Some(ref msg) = self.status_message {
            if !msg.sticky && Instant::now() >= msg.expires_at {
                self.status_message = None;
            }
        }
    }

    /// Get stats for the current view (returns cached value)
    pub fn get_stats(&self) -> &Stats {
        &self.cached_stats
    }

    /// Switch view mode
    pub fn set_view(&mut self, view: ViewMode) -> Result<()> {
        if self.view != view {
            // Persist the view choice
            self.config.default_view = view;
            self.save_config();
            self.switch_view(view)?;
        }
        Ok(())
    }

    /// Switch view without saving it as the default view.
    fn switch_view(&mut self, view: ViewMode) -> Result<()> {
        if self.view != view {
            self.view = view;
            self.selected_index = 0;
            self.refresh_items()?;
        }
        Ok(())
    }

    /// Persist config changes; failures are not worth interrupting the TUI.
    fn save_config(&self) {
        let _ = match &self.config_path {
            Some(path) => self.config.save_to(path),
            None => self.config.save(),
        };
    }

    /// Activate the command line with an optional prefix
    pub fn activate_command_line(&mut self, prefix: &str) {
        self.command_line.input = prefix.to_string();
        self.command_line.cursor = prefix.chars().count();
        self.command_line.focused = true;
        self.command_line.suggestions.clear();
        self.command_line.selected_suggestion = None;
        self.command_line.pending_confirm = None;
        self.command_line.link_baseline = None;
    }

    /// Deactivate the command line and clear state
    pub fn deactivate_command_line(&mut self) {
        self.command_line = CommandLineState::default();
        self.history_index = None;
        self.history_saved_input.clear();
    }

    /// Push a command to history (deduplicates consecutive)
    pub fn push_history(&mut self, cmd: String) {
        if !cmd.trim().is_empty() {
            // Don't duplicate if same as last entry
            if self.command_history.last().map(|s| s.as_str()) != Some(cmd.trim()) {
                self.command_history.push(cmd.trim().to_string());
            }
            // Cap history at 50 entries
            if self.command_history.len() > 50 {
                self.command_history.remove(0);
            }
        }
    }

    /// Quit the application
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// Whether the link pane is toggled on and the terminal is wide enough.
    pub fn pane_shown(&self) -> bool {
        self.pane.visible && self.status_width >= PANE_MIN_WIDTH
    }

    /// Outgoing links and backlinks of the selected item.
    pub fn pane_content(&self) -> PaneContent {
        let Some(item) = self.selected_item() else {
            return PaneContent::default();
        };

        let outgoing = links::item_link_targets(item)
            .into_iter()
            .map(|id| {
                let target = self.links.target(&id);
                let title = match target {
                    Target::Active { .. } => self.links.title(&id).unwrap_or_default(),
                    Target::Divergent | Target::Missing => String::new(),
                };
                PaneEntry { id, title, target }
            })
            .collect();

        // Backlinks cannot be attributed to one of two items sharing an ID.
        let own_id = item.stable_id();
        let divergent = self.links.resolver().is_divergent(&own_id);
        let mut backlinks: Vec<PaneEntry> = if divergent {
            Vec::new()
        } else {
            self.links
                .backlinks(&own_id)
                .iter()
                .filter_map(|n| self.active_items.get(&n.to_string()))
                .map(|source| {
                    let id = source.stable_id();
                    PaneEntry {
                        target: self.links.target(&id),
                        title: plain_text(self, source.description()),
                        id,
                    }
                })
                .collect()
        };
        // Backlinks follow the main list's order; others keep ID order.
        let position = |entry: &PaneEntry| match entry.target {
            Target::Active { numeric } => self
                .display_order
                .iter()
                .position(|&id| id == numeric)
                .unwrap_or(usize::MAX),
            _ => usize::MAX,
        };
        backlinks.sort_by_cached_key(position);

        PaneContent {
            outgoing,
            backlinks,
            divergent,
        }
    }

    /// The pane cursor, clamped to the current entries.
    pub fn pane_cursor(&self, content: &PaneContent) -> usize {
        self.pane.cursor.min(content.len().saturating_sub(1))
    }

    /// Select the active item `numeric`, switching view and clearing filters
    /// as needed without touching saved settings, and say what changed.
    pub fn jump_to(&mut self, numeric: u64) -> Result<()> {
        let mut changed = Vec::new();
        if self.view == ViewMode::Archive {
            self.switch_view(ViewMode::Board)?;
            changed.push("switched to board view");
        }
        if self.filter.search_term.take().is_some() {
            changed.push("cleared search");
        }
        if self.filter.board_filter.take().is_some() {
            changed.push("cleared board filter");
        }
        let completed = self
            .active_items
            .get(&numeric.to_string())
            .and_then(|item| item.as_task())
            .is_some_and(|task| task.is_complete);
        self.filter.revealed_completed = None;
        if completed && self.filter.hide_completed {
            self.filter.revealed_completed = Some(numeric);
            changed.push("showing a completed task");
        }
        self.update_display_order();
        if let Some(position) = self.display_order.iter().position(|&id| id == numeric) {
            self.selected_index = position;
        }
        self.pane.focused = false;
        self.pane.cursor = 0;

        let mut message = format!("Jumped to {}", numeric);
        if !changed.is_empty() {
            message.push_str(&format!(" ({})", changed.join(", ")));
        }
        self.set_status(message, StatusKind::Info);
        Ok(())
    }
}

/// Link side pane state
#[derive(Debug, Clone)]
pub struct PaneState {
    /// Toggled on (it is still hidden below `PANE_MIN_WIDTH`)
    pub visible: bool,
    /// Keys go to the pane instead of the list
    pub focused: bool,
    /// Index into outgoing links followed by backlinks
    pub cursor: usize,
}

/// One entry in the link pane
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneEntry {
    pub id: String,
    /// Rendered title (empty for broken and divergent targets)
    pub title: String,
    pub target: Target,
}

/// What the link pane shows for the selected item
#[derive(Debug, Clone, Default)]
pub struct PaneContent {
    pub outgoing: Vec<PaneEntry>,
    pub backlinks: Vec<PaneEntry>,
    /// The selected item's own ID is divergent, so backlinks are not listed
    pub divergent: bool,
}

impl PaneContent {
    pub fn len(&self) -> usize {
        self.outgoing.len() + self.backlinks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The entry at a cursor position over outgoing links then backlinks.
    pub fn get(&self, index: usize) -> Option<&PaneEntry> {
        self.outgoing
            .get(index)
            .or_else(|| self.backlinks.get(index.checked_sub(self.outgoing.len())?))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::storage::LocalStorage;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use taskbook_common::links::Target;

    /// An `App` over a fresh temp store whose config writes go to a temp file.
    pub(crate) fn test_app() -> (App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("tb-app-test-{}", uuid::Uuid::new_v4()));
        let taskbook = Taskbook::with_storage(Box::new(LocalStorage::new(&dir).unwrap()));
        let config_path = dir.join("config.json");
        let app = App::with_taskbook(taskbook, Config::default(), config_path.clone()).unwrap();
        (app, config_path)
    }

    pub(crate) fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn create_task(app: &mut App, desc: &str) -> u64 {
        let id = app
            .taskbook
            .create_task_direct_with_tags(vec!["My Board".into()], desc.into(), 1, vec![], None)
            .unwrap();
        app.refresh_items().unwrap();
        id
    }

    fn stable_id_of(app: &App, numeric: u64) -> String {
        app.items[&numeric.to_string()].stable_id()
    }

    /// Covers AE5.
    #[test]
    fn restoring_a_linked_item_revives_its_backlinks() {
        let (mut app, _) = test_app();
        let t = create_task(&mut app, "Target");
        let tid = stable_id_of(&app, t);
        for name in ["A", "B", "C"] {
            create_task(&mut app, &format!("{name} [[{tid}]]"));
        }
        assert_eq!(app.links.backlinks(&tid).len(), 3);

        app.taskbook.delete_items_silent(&[t]).unwrap();
        app.refresh_items().unwrap();
        assert_eq!(app.links.target(&tid), Target::Missing);
        assert_eq!(app.links.backlinks(&tid).len(), 3);

        app.set_view(ViewMode::Archive).unwrap();
        let archived = app.display_order[0];
        app.selected_index = 0;
        super::super::actions::handle_key_event(&mut app, key('r')).unwrap();

        assert!(matches!(app.links.target(&tid), Target::Active { .. }));
        assert_eq!(app.links.backlinks(&tid).len(), 3);
        assert!(
            !app.items.contains_key(&archived.to_string()),
            "restored row must leave the archive list at once"
        );
    }

    #[test]
    fn archive_view_keeps_an_index_of_active_items() {
        let (mut app, _) = test_app();
        let t = create_task(&mut app, "Target");
        let tid = stable_id_of(&app, t);
        create_task(&mut app, &format!("A [[{tid}]]"));

        app.set_view(ViewMode::Archive).unwrap();
        assert!(app.items.is_empty());
        assert_eq!(app.links.target(&tid), Target::Active { numeric: t });
        assert_eq!(app.links.backlinks(&tid).len(), 1);

        app.refresh_items().unwrap();
        assert!(
            app.items.is_empty(),
            "refresh in archive view keeps showing the archive"
        );
    }

    #[test]
    fn config_writes_go_to_the_test_path() {
        let (mut app, config_path) = test_app();
        app.set_view(ViewMode::Archive).unwrap();
        let saved = std::fs::read_to_string(&config_path).unwrap();
        assert!(saved.contains("archive"), "{saved}");
    }
    // ----- link pane and jump -----

    fn select(app: &mut App, id: u64) {
        app.selected_index = app.display_order.iter().position(|&i| i == id).unwrap();
    }

    fn press(app: &mut App, code: KeyCode) {
        super::super::actions::handle_key_event(app, KeyEvent::new(code, KeyModifiers::NONE))
            .unwrap();
    }

    fn wide(app: &mut App) {
        app.status_width = 120;
    }

    /// Covers F3.
    #[test]
    fn jumping_from_a_backlink_selects_the_linking_item() {
        let (mut app, _) = test_app();
        wide(&mut app);
        let b = create_task(&mut app, "B");
        let bid = stable_id_of(&app, b);
        let a = create_task(&mut app, &format!("A [[{bid}]]"));
        select(&mut app, b);

        let content = app.pane_content();
        assert_eq!(content.backlinks.len(), 1);
        press(&mut app, KeyCode::Char('o'));
        assert!(app.pane.focused);
        press(&mut app, KeyCode::Enter);

        assert_eq!(app.selected_id(), Some(a));
        assert!(!app.pane.focused, "focus returns to the list after a jump");
    }

    #[test]
    fn jump_reveals_a_hidden_completed_task_without_saving_config() {
        let (mut app, config_path) = test_app();
        wide(&mut app);
        let done = create_task(&mut app, "Done thing");
        app.taskbook.check_tasks_silent(&[done]).unwrap();
        app.refresh_items().unwrap();
        app.filter.hide_completed = true;
        app.update_display_order();
        assert!(!app.display_order.contains(&done));
        let before = std::fs::read_to_string(&config_path).ok();

        app.jump_to(done).unwrap();
        assert_eq!(app.selected_id(), Some(done));
        assert!(app.filter.hide_completed);
        assert_eq!(std::fs::read_to_string(&config_path).ok(), before);
    }

    #[test]
    fn jump_from_archive_view_keeps_the_saved_default_view() {
        let (mut app, config_path) = test_app();
        let t = create_task(&mut app, "Target");
        app.set_view(ViewMode::Archive).unwrap();
        let saved = std::fs::read_to_string(&config_path).unwrap();

        app.jump_to(t).unwrap();
        assert_eq!(app.view, ViewMode::Board);
        assert_eq!(app.selected_id(), Some(t));
        assert_eq!(std::fs::read_to_string(&config_path).unwrap(), saved);
    }

    #[test]
    fn jump_clears_search_and_board_filter_and_says_so() {
        let (mut app, _) = test_app();
        let t = create_task(&mut app, "Target");
        app.filter.search_term = Some("nomatch".into());
        app.filter.board_filter = Some("elsewhere".into());
        app.update_display_order();

        app.jump_to(t).unwrap();
        assert_eq!(app.selected_id(), Some(t));
        assert!(app.filter.search_term.is_none() && app.filter.board_filter.is_none());
        let status = app.status_message.as_ref().unwrap().text.clone();
        assert!(
            status.contains("search") && status.contains("board"),
            "{status}"
        );
    }

    #[test]
    fn broken_and_divergent_entries_are_not_jumpable() {
        let (mut app, _) = test_app();
        wide(&mut app);
        let missing = taskbook_common::stable_id::from_timestamp(9_999);
        let a = create_task(&mut app, &format!("A [[{missing}]]"));
        select(&mut app, a);

        press(&mut app, KeyCode::Char('o'));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.selected_id(), Some(a));
        assert!(app.pane.focused);
        assert_eq!(
            app.status_message.as_ref().unwrap().text,
            "That link is broken"
        );
    }

    #[test]
    fn divergent_entries_say_why_they_cannot_be_jumped_to() {
        let (mut app, _) = test_app();
        wide(&mut app);
        let twin_id = taskbook_common::stable_id::from_timestamp(1);
        let linker = create_task(&mut app, &format!("L [[{twin_id}]]"));
        let mut twin = taskbook_common::Task::new(90, "Twin".into(), vec!["my board".into()], 1);
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
        select(&mut app, linker);

        press(&mut app, KeyCode::Char('o'));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.selected_id(), Some(linker));
        assert_eq!(
            app.status_message.as_ref().unwrap().text,
            "Two items share that ID; cannot jump"
        );
    }

    #[test]
    fn pane_keys_move_the_cursor_jump_and_leave() {
        let (mut app, _) = test_app();
        wide(&mut app);
        let t = create_task(&mut app, "Target");
        let tid = stable_id_of(&app, t);
        create_task(&mut app, &format!("A [[{tid}]]"));
        create_task(&mut app, &format!("B [[{tid}]]"));
        select(&mut app, t);
        let expected = match app.pane_content().get(1).map(|e| e.target) {
            Some(Target::Active { numeric }) => numeric,
            other => panic!("unexpected second entry: {other:?}"),
        };

        press(&mut app, KeyCode::Char('o'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.pane.cursor, 1, "j stops at the last entry");
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.pane.cursor, 0);
        press(&mut app, KeyCode::Esc);
        assert!(!app.pane.focused);

        press(&mut app, KeyCode::Char('o'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.selected_id(), Some(expected));
    }

    #[test]
    fn hiding_the_pane_returns_keys_to_the_list() {
        let (mut app, _) = test_app();
        wide(&mut app);
        let first = create_task(&mut app, "first");
        let fid = stable_id_of(&app, first);
        create_task(&mut app, &format!("second [[{fid}]]"));
        select(&mut app, first);
        press(&mut app, KeyCode::Char('o'));
        assert!(app.pane.focused);

        app.status_width = 80;
        press(&mut app, KeyCode::Char('j'));
        assert!(!app.pane.focused);
        assert_ne!(app.selected_id(), Some(first));
    }

    #[test]
    fn narrow_terminal_keys_explain_the_hidden_pane() {
        let (mut app, _) = test_app();
        app.status_width = 80;
        create_task(&mut app, "x");

        press(&mut app, KeyCode::Char('v'));
        let status = app.status_message.as_ref().unwrap().text.clone();
        assert!(status.contains("100 columns"), "{status}");

        press(&mut app, KeyCode::Char('o'));
        assert_eq!(
            app.status_message.as_ref().unwrap().text,
            "Link pane hidden"
        );
        assert!(!app.pane.focused);
    }

    #[test]
    fn pane_cursor_clamps_to_the_current_entries() {
        let (mut app, _) = test_app();
        let b = create_task(&mut app, "B");
        let bid = stable_id_of(&app, b);
        create_task(&mut app, &format!("A [[{bid}]]"));
        select(&mut app, b);
        app.pane.cursor = 5;
        let content = app.pane_content();
        assert_eq!(app.pane_cursor(&content), 0);
    }

    #[test]
    fn archived_item_lists_active_backlinks() {
        let (mut app, _) = test_app();
        let t = create_task(&mut app, "Target");
        let tid = stable_id_of(&app, t);
        for name in ["A", "B", "C"] {
            create_task(&mut app, &format!("{name} [[{tid}]]"));
        }
        app.taskbook.delete_items_silent(&[t]).unwrap();
        app.set_view(ViewMode::Archive).unwrap();
        app.selected_index = 0;

        let content = app.pane_content();
        let titles: Vec<String> = content.backlinks.iter().map(|e| e.title.clone()).collect();
        assert_eq!(titles.len(), 3, "{titles:?}");
    }

    #[test]
    fn divergent_selection_shows_no_backlinks() {
        let (mut app, _) = test_app();
        let mut twin = taskbook_common::Task::new(90, "Twin".into(), vec!["my board".into()], 1);
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
        app.selected_index = 0;

        let content = app.pane_content();
        assert!(content.divergent);
        assert!(content.backlinks.is_empty());
    }

    // ----- size notice -----

    /// Put `pending` open and `done` completed tasks in the app's active set.
    fn fill(app: &mut App, pending: u64, done: u64) {
        for id in 1..=pending + done {
            let mut task =
                taskbook_common::Task::new(id, format!("t{id}"), vec!["my board".into()], 1);
            task.is_complete = id > pending;
            app.active_items
                .insert(id.to_string(), StorageItem::Task(task));
        }
        app.items = app.active_items.clone();
    }

    #[test]
    fn no_size_notice_within_the_recommendation() {
        let (mut app, _) = test_app();
        fill(&mut app, 512, 0);
        assert_eq!(app.size_notice(), None);
    }

    #[test]
    fn size_notice_suggests_clear_only_with_completed_tasks() {
        let (mut app, _) = test_app();
        fill(&mut app, 513, 0);
        assert_eq!(
            app.size_notice().as_deref(),
            Some("513 active items, over the 512 recommended")
        );

        let (mut app, _) = test_app();
        fill(&mut app, 500, 13);
        assert_eq!(
            app.size_notice().as_deref(),
            Some("513 active items, over the 512 recommended · /clear archives 13 done")
        );
    }

    #[test]
    fn archive_only_notice_has_no_action() {
        let (mut app, _) = test_app();
        fill(&mut app, 10, 5);
        app.archived_count = 5000;
        assert_eq!(
            app.size_notice().as_deref(),
            Some("5000 archived items, over the 4096 supported")
        );
    }

    #[test]
    fn startup_counts_the_archive() {
        let dir = std::env::temp_dir().join(format!("tb-app-test-{}", uuid::Uuid::new_v4()));
        let taskbook = Taskbook::with_storage(Box::new(LocalStorage::new(&dir).unwrap()));
        let id = taskbook
            .create_task_direct_with_tags(vec!["my board".into()], "x".into(), 1, vec![], None)
            .unwrap();
        taskbook.delete_items_silent(&[id]).unwrap();

        let app = App::with_taskbook(taskbook, Config::default(), dir.join("config.json")).unwrap();
        assert_eq!(app.archived_count, 1);
    }

    #[test]
    fn header_shows_the_size_notice() {
        use ratatui::{backend::TestBackend, Terminal};
        let (mut app, _) = test_app();
        fill(&mut app, 513, 0);
        app.update_display_order();
        let mut terminal = Terminal::new(TestBackend::new(160, 10)).unwrap();
        terminal
            .draw(|f| crate::tui::ui::render(f, &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let header: String = (0..buffer.area.width)
            .map(|x| buffer[(x, 0)].symbol().to_string())
            .collect();
        assert!(
            header.contains("[513 active items, over the 512 recommended]"),
            "{header}"
        );
    }
}
