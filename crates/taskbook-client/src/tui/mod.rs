mod actions;
mod app;
mod autocomplete;
mod command_parser;
mod event;
mod input_handler;
mod theme;
mod ui;
pub mod widgets;

use crate::config::SyncBackend;
use crate::credentials::Credentials;
use crate::error::{Result, TaskbookError};
pub use app::{App, ViewMode};

use std::io::{self, Write};
use std::path::Path;

use crossterm::{
    cursor,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;

/// Temporarily suspend the TUI to run an external command (like an editor).
/// Returns a guard that restores the terminal when dropped.
pub fn suspend_tui() -> Result<TuiSuspendGuard> {
    // First, pause the event handler thread to stop it from consuming input
    event::pause_event_handler();

    let mut stdout = io::stdout();

    // Disable mouse capture first (while still in raw mode)
    execute!(stdout, DisableMouseCapture).map_err(|e| TaskbookError::Tui(e.to_string()))?;

    // Leave alternate screen
    execute!(stdout, LeaveAlternateScreen).map_err(|e| TaskbookError::Tui(e.to_string()))?;

    // Disable raw mode
    disable_raw_mode().map_err(|e| TaskbookError::Tui(e.to_string()))?;

    // Show cursor and reset terminal state
    execute!(
        stdout,
        cursor::Show,
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
        cursor::MoveTo(0, 0)
    )
    .map_err(|e| TaskbookError::Tui(e.to_string()))?;

    stdout.flush().ok();
    Ok(TuiSuspendGuard { _private: () })
}

/// Guard that restores TUI state when dropped
pub struct TuiSuspendGuard {
    _private: (),
}

impl TuiSuspendGuard {
    /// Explicitly resume the TUI (called automatically on drop)
    pub fn resume(self) -> Result<()> {
        self.do_resume()
    }

    fn do_resume(&self) -> Result<()> {
        let mut stdout = io::stdout();
        enable_raw_mode().map_err(|e| TaskbookError::Tui(e.to_string()))?;
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            cursor::Hide
        )
        .map_err(|e| TaskbookError::Tui(e.to_string()))?;
        stdout.flush().ok();

        // Drain any stale keyboard events buffered while the editor was running
        event::drain_input_buffer();

        // Resume the event handler thread
        event::resume_event_handler();

        Ok(())
    }
}

impl Drop for TuiSuspendGuard {
    fn drop(&mut self) {
        // Best effort to restore terminal on drop
        let _ = self.do_resume();
    }
}

/// Restore the terminal to its normal state.
fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) {
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    let _ = terminal.show_cursor();
}

/// Run the TUI application
pub fn run(taskbook_dir: Option<&Path>) -> Result<()> {
    // Setup terminal
    enable_raw_mode().map_err(|e| TaskbookError::Tui(e.to_string()))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .map_err(|e| TaskbookError::Tui(e.to_string()))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| TaskbookError::Tui(e.to_string()))?;

    // Create app — restore terminal on failure
    let mut app = match App::new(taskbook_dir) {
        Ok(app) => app,
        Err(e) => {
            restore_terminal(&mut terminal);
            return Err(e);
        }
    };

    let res = run_app(&mut terminal, &mut app);

    // Restore terminal
    restore_terminal(&mut terminal);

    res
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    let events = create_event_handler(app);

    while app.running {
        // Force full redraw if requested (e.g. after returning from external editor)
        if app.needs_full_redraw {
            terminal
                .clear()
                .map_err(|e| TaskbookError::Tui(e.to_string()))?;
            app.needs_full_redraw = false;
        }

        terminal
            .draw(|f| ui::render(f, app))
            .map_err(|e| TaskbookError::Tui(e.to_string()))?;

        match events.next()? {
            event::Event::Key(key) => {
                actions::handle_key_event(app, key)?;
            }
            event::Event::Tick => {
                app.tick();
            }
            event::Event::Resize(_, _) => {}
            event::Event::DataChanged { archived } => {
                use app::ViewMode;
                match (app.view, archived) {
                    (ViewMode::Archive, true) => {
                        app.items = app.taskbook.get_all_archive_items()?;
                        app.update_display_order();
                    }
                    (ViewMode::Board | ViewMode::Timeline | ViewMode::Journal, false) => {
                        app.refresh_items()?;
                    }
                    _ => {} // Data will be loaded when user switches views
                }
            }
        }
    }

    Ok(())
}

/// Create the appropriate event handler based on sync configuration.
///
/// Backends that push change notifications themselves (see
/// `StorageBackend::watch`) are preferred; the HTTP server backend falls back
/// to its SSE stream.
fn create_event_handler(app: &App) -> event::EventHandler {
    if let Ok(Some(handler)) = event::EventHandler::new_with_watch(250, &app.taskbook) {
        return handler;
    }
    let config = &app.config;
    if config.sync.enabled && config.sync.backend == SyncBackend::Server {
        if let Ok(Some(creds)) = Credentials::load() {
            return event::EventHandler::new_with_sse(250, creds.server_url, creds.token);
        }
    }
    event::EventHandler::new(250)
}
