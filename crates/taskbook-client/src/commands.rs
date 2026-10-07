use std::path::PathBuf;

use colored::Colorize;

use crate::config::Config;
use crate::directory::resolve_taskbook_directory;
use crate::error::Result;
use crate::storage::{self, LocalStorage, StorageBackend};
use crate::taskbook::Taskbook;

/// Execute CLI commands
#[allow(clippy::too_many_arguments)]
pub fn run(
    input: Vec<String>,
    archive: bool,
    task: bool,
    restore: bool,
    note: bool,
    delete: bool,
    check: bool,
    begin: bool,
    star: bool,
    priority: bool,
    due: bool,
    copy: bool,
    timeline: bool,
    find: bool,
    list: bool,
    edit: bool,
    edit_note: bool,
    r#move: bool,
    clear: bool,
    tag: bool,
    taskbook_dir: Option<PathBuf>,
) -> Result<()> {
    let taskbook = Taskbook::new(taskbook_dir.as_deref())?;

    if archive {
        return taskbook.display_archive();
    }

    if task {
        return taskbook.create_task(&input);
    }

    if restore {
        let ids: Vec<u64> = input.iter().filter_map(|s| s.parse().ok()).collect();
        return taskbook.restore_items(&ids);
    }

    if note {
        // If no description provided, open external editor
        if input.is_empty() {
            return taskbook.create_note_with_editor();
        }
        return taskbook.create_note(&input);
    }

    if edit_note {
        return taskbook.edit_note_in_editor(&input);
    }

    if delete {
        let ids: Vec<u64> = input.iter().filter_map(|s| s.parse().ok()).collect();
        return taskbook.delete_items(&ids);
    }

    if check {
        let ids: Vec<u64> = input.iter().filter_map(|s| s.parse().ok()).collect();
        return taskbook.check_tasks(&ids);
    }

    if begin {
        let ids: Vec<u64> = input.iter().filter_map(|s| s.parse().ok()).collect();
        return taskbook.begin_tasks(&ids);
    }

    if star {
        let ids: Vec<u64> = input.iter().filter_map(|s| s.parse().ok()).collect();
        return taskbook.star_items(&ids);
    }

    if priority {
        return taskbook.update_priority(&input);
    }

    if due {
        return taskbook.update_due_date(&input);
    }

    if copy {
        let ids: Vec<u64> = input.iter().filter_map(|s| s.parse().ok()).collect();
        return taskbook.copy_to_clipboard(&ids);
    }

    if timeline {
        taskbook.display_by_date()?;
        return taskbook.display_stats();
    }

    if find {
        return taskbook.find_items(&input);
    }

    if list {
        taskbook.list_by_attributes(&input)?;
        return taskbook.display_stats();
    }

    if edit {
        return taskbook.edit_description(&input);
    }

    if r#move {
        return taskbook.move_boards(&input);
    }

    if clear {
        return taskbook.clear();
    }

    if tag {
        return taskbook.update_tags(&input);
    }

    // Default: display board view and stats
    taskbook.display_by_board()?;
    taskbook.display_stats()
}

/// Migrate local data to the configured sync backend (server or Ditto).
pub fn migrate(taskbook_dir: Option<PathBuf>) -> Result<()> {
    let config = Config::load_or_default();

    // Load local data
    let resolved_dir = resolve_taskbook_directory(taskbook_dir.as_deref())?;
    let local = LocalStorage::new(&resolved_dir)?;

    let items = local.get()?;
    let archive = local.get_archive()?;

    // The sync backend handles encryption and transport for its target.
    let target = storage::sync_backend(&config)?;
    target.set(&items)?;
    target.set_archive(&archive)?;

    println!(
        "{}",
        format!(
            "Migrated {} items and {} archived items to {}.",
            items.len(),
            archive.len(),
            config.sync.backend.display_name()
        )
        .green()
        .bold()
    );
    if !config.sync.enabled {
        println!(
            "{}",
            format!(
                "To enable sync, set sync.enabled = true in {}",
                crate::config::Config::config_file_path().display()
            )
            .dimmed()
        );
    }

    Ok(())
}
