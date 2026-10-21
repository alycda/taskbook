use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::{Config, SortMethod};
use crate::error::Result;
use crate::render::Stats;
use crate::taskbook::Taskbook;
use taskbook_common::board;
use taskbook_common::links::LinkIndex;
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
    pub link_baseline: Option<(u64, String)>,
    /// Char index of the `[[` whose link dropdown the user closed with Esc
    pub link_dismissed_at: Option<usize>,
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
            cached_stats: Stats {
                percent: 0,
                complete: 0,
                in_progress: 0,
                pending: 0,
                notes: 0,
            },
        };

        app.refresh_items()?;

        Ok(app)
    }

    /// Refresh items from storage and rebuild the link index
    pub fn refresh_items(&mut self) -> Result<()> {
        self.active_items = self.taskbook.get_all_items()?;
        self.links = LinkIndex::build(self.active_items.values());
        self.boards = self.taskbook.get_all_boards()?;
        self.items = if self.view == ViewMode::Archive {
            self.taskbook.get_all_archive_items()?
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
        if self.filter.hide_completed {
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
                    .filter(|item| {
                        if self.filter.hide_completed {
                            if let Some(task) = item.as_task() {
                                if task.is_complete {
                                    return false;
                                }
                            }
                        }
                        if let Some(ref term) = self.filter.search_term {
                            let term_lower = term.to_lowercase();
                            let in_desc = item.description().to_lowercase().contains(&term_lower);
                            let in_body = item
                                .note_body()
                                .map(|b| b.to_lowercase().contains(&term_lower))
                                .unwrap_or(false);
                            let in_tags = item
                                .tags()
                                .iter()
                                .any(|t| t.to_lowercase().contains(&term_lower));
                            if !in_desc && !in_body && !in_tags {
                                return false;
                            }
                        }
                        true
                    })
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
            self.view = view;
            self.selected_index = 0;

            // Persist the view choice
            self.config.default_view = view;
            self.save_config();

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
}
