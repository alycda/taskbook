//! Storage-agnostic business logic shared by the CLI, TUI and MCP server.
//!
//! Every mutation validates all of its ids (existence, and task-ness where it
//! matters) before changing anything, so an operation either applies to every
//! id or to none. Nothing here prints; callers render outcomes and errors.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;

use arboard::Clipboard;

use crate::config::Config;
use crate::directory::resolve_taskbook_directory;
use crate::error::{Result, TaskbookError};
use crate::storage::{LocalStorage, RemoteStorage, StorageBackend};
use taskbook_common::board::{self, DEFAULT_BOARD};
use taskbook_common::id::IdError;
use taskbook_common::{Note, Priority, StorageItem, Task};

pub type Items = HashMap<String, StorageItem>;

/// Fields for a new task.
#[derive(Debug, Default)]
pub struct NewTask {
    /// Board names (normalized on create). Empty means the default board.
    pub boards: Vec<String>,
    pub description: String,
    pub priority: Priority,
    pub tags: Vec<String>,
    pub due_date: Option<i64>,
}

/// Fields for a new note.
#[derive(Debug, Default)]
pub struct NewNote {
    /// Board names (normalized on create). Empty means the default board.
    pub boards: Vec<String>,
    pub title: String,
    pub body: Option<String>,
    pub tags: Vec<String>,
}

/// Ids partitioned by the state a toggle left them in.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Toggled {
    pub on: Vec<u64>,
    pub off: Vec<u64>,
}

/// An explicit task state (as opposed to the toggles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    InProgress,
    Done,
}

impl FromStr for TaskState {
    type Err = TaskbookError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "pending" => Ok(TaskState::Pending),
            "in_progress" => Ok(TaskState::InProgress),
            "done" => Ok(TaskState::Done),
            other => Err(TaskbookError::General(format!(
                "invalid state '{other}' (use pending, in_progress or done)"
            ))),
        }
    }
}

/// A partial update to one item. `None` leaves a field unchanged.
#[derive(Debug, Default)]
pub struct ItemEdit {
    pub description: Option<String>,
    pub priority: Option<Priority>,
    /// `Some(None)` clears the due date.
    pub due_date: Option<Option<i64>>,
    pub boards: Option<Vec<String>>,
    /// Replaces the item's tags.
    pub tags: Option<Vec<String>>,
    pub starred: Option<bool>,
}

impl ItemEdit {
    /// Names of the fields this edit changes, in a stable order.
    pub fn fields(&self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        if self.description.is_some() {
            fields.push("description");
        }
        if self.priority.is_some() {
            fields.push("priority");
        }
        if self.due_date.is_some() {
            fields.push("due_date");
        }
        if self.boards.is_some() {
            fields.push("boards");
        }
        if self.tags.is_some() {
            fields.push("tags");
        }
        if self.starred.is_some() {
            fields.push("starred");
        }
        fields
    }
}

pub struct Taskbook {
    storage: Box<dyn StorageBackend>,
}

fn next_id(data: &Items) -> u64 {
    data.keys()
        .filter_map(|k| k.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1
}

/// Dedupe `ids` (keeping order) and check every one exists in `data`.
fn resolve_ids(data: &Items, ids: &[u64]) -> Result<Vec<u64>> {
    if ids.is_empty() {
        return Err(IdError::Missing.into());
    }
    let mut seen = HashSet::with_capacity(ids.len());
    let unique: Vec<u64> = ids.iter().copied().filter(|id| seen.insert(*id)).collect();
    let unknown: Vec<u64> = unique
        .iter()
        .copied()
        .filter(|id| !data.contains_key(&id.to_string()))
        .collect();
    if !unknown.is_empty() {
        return Err(TaskbookError::UnknownIds(unknown));
    }
    Ok(unique)
}

/// Like [`resolve_ids`], additionally requiring every item to be a task.
fn resolve_task_ids(data: &Items, ids: &[u64]) -> Result<Vec<u64>> {
    let ids = resolve_ids(data, ids)?;
    let notes: Vec<u64> = ids
        .iter()
        .copied()
        .filter(|id| !data[&id.to_string()].is_task())
        .collect();
    if !notes.is_empty() {
        return Err(TaskbookError::NotATask(notes));
    }
    Ok(ids)
}

/// Normalize and dedupe board names, falling back to the default board.
fn normalize_boards(boards: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for b in boards {
        let b = board::normalize_board_name(&b);
        if !out.iter().any(|o| board::board_eq(o, &b)) {
            out.push(b);
        }
    }
    if out.is_empty() {
        out.push(DEFAULT_BOARD.to_string());
    }
    out
}

/// Normalize and dedupe tags, dropping empty ones.
fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t = board::normalize_tag(&t);
        if !t.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(&t)) {
            out.push(t);
        }
    }
    out
}

fn non_empty(text: String) -> Result<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        Err(TaskbookError::MissingDescription)
    } else {
        Ok(trimmed.to_string())
    }
}

impl Taskbook {
    /// Open the taskbook selected by the config file (remote when sync is
    /// enabled, otherwise the resolved local directory).
    pub fn new(taskbook_dir: Option<&Path>) -> Result<Self> {
        Self::from_config(&Config::load_or_default(), taskbook_dir)
    }

    pub fn from_config(config: &Config, taskbook_dir: Option<&Path>) -> Result<Self> {
        let storage: Box<dyn StorageBackend> = if config.sync.enabled {
            Box::new(RemoteStorage::new(&config.sync.server_url)?)
        } else {
            let resolved_dir = resolve_taskbook_directory(taskbook_dir)?;
            Box::new(LocalStorage::new(&resolved_dir)?)
        };
        Ok(Self { storage })
    }

    /// Construct a Taskbook over an explicit storage backend (for tests).
    #[allow(dead_code)]
    pub fn with_storage(storage: Box<dyn StorageBackend>) -> Self {
        Self { storage }
    }

    // ---- queries ----

    pub fn items(&self) -> Result<Items> {
        self.storage.get()
    }

    pub fn archived_items(&self) -> Result<Items> {
        self.storage.get_archive()
    }

    /// The items with these ids (deduped, in the given order), from the
    /// archive when `archived` is set.
    pub fn get(&self, ids: &[u64], archived: bool) -> Result<Vec<StorageItem>> {
        let data = if archived {
            self.archived_items()?
        } else {
            self.items()?
        };
        let ids = resolve_ids(&data, ids)?;
        Ok(ids.iter().map(|id| data[&id.to_string()].clone()).collect())
    }

    pub fn boards(&self) -> Result<Vec<String>> {
        Ok(Self::boards_of(&self.items()?))
    }

    /// The default board followed by every other board on `data`, sorted
    /// case-insensitively.
    pub fn boards_of(data: &Items) -> Vec<String> {
        let mut boards = vec![DEFAULT_BOARD.to_string()];
        let mut items: Vec<_> = data.iter().collect();
        items.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(u64::MAX));
        for (_, item) in items {
            for b in item.boards() {
                if !boards.iter().any(|existing| board::board_eq(existing, b)) {
                    boards.push(b.clone());
                }
            }
        }
        boards[1..].sort_by_key(|a| a.to_lowercase());
        boards
    }

    // ---- create ----

    pub fn create_task(&self, new: NewTask) -> Result<u64> {
        let description = non_empty(new.description)?;
        let mut data = self.items()?;
        let id = next_id(&data);
        let task = Task::new(id, description, normalize_boards(new.boards), new.priority)
            .with_tags(normalize_tags(new.tags))
            .with_due_date(new.due_date);
        data.insert(id.to_string(), StorageItem::Task(task));
        self.storage.set(&data)?;
        Ok(id)
    }

    pub fn create_note(&self, new: NewNote) -> Result<u64> {
        let title = non_empty(new.title)?;
        let mut data = self.items()?;
        let id = next_id(&data);
        let mut note = Note::new_with_body(id, title, new.body, normalize_boards(new.boards));
        note.tags = normalize_tags(new.tags);
        data.insert(id.to_string(), StorageItem::Note(note));
        self.storage.set(&data)?;
        Ok(id)
    }

    // ---- mutate ----

    /// Load, validate `ids`, apply `f` to each item, save.
    fn update_items(
        &self,
        ids: &[u64],
        tasks_only: bool,
        mut f: impl FnMut(u64, &mut StorageItem),
    ) -> Result<Vec<u64>> {
        let mut data = self.items()?;
        let ids = if tasks_only {
            resolve_task_ids(&data, ids)?
        } else {
            resolve_ids(&data, ids)?
        };
        for id in &ids {
            f(*id, data.get_mut(&id.to_string()).expect("resolved"));
        }
        self.storage.set(&data)?;
        Ok(ids)
    }

    fn update_tasks(&self, ids: &[u64], mut f: impl FnMut(u64, &mut Task)) -> Result<Vec<u64>> {
        self.update_items(ids, true, |id, item| {
            f(id, item.as_task_mut().expect("resolved as task"))
        })
    }

    /// Toggle tasks between done and pending. Clears in-progress.
    pub fn toggle_check(&self, ids: &[u64]) -> Result<Toggled> {
        let mut out = Toggled::default();
        self.update_tasks(ids, |id, task| {
            task.in_progress = false;
            task.is_complete = !task.is_complete;
            let bucket = if task.is_complete {
                &mut out.on
            } else {
                &mut out.off
            };
            bucket.push(id);
        })?;
        Ok(out)
    }

    /// Toggle tasks between in-progress and pending. Clears done.
    pub fn toggle_begin(&self, ids: &[u64]) -> Result<Toggled> {
        let mut out = Toggled::default();
        self.update_tasks(ids, |id, task| {
            task.is_complete = false;
            task.in_progress = !task.in_progress;
            let bucket = if task.in_progress {
                &mut out.on
            } else {
                &mut out.off
            };
            bucket.push(id);
        })?;
        Ok(out)
    }

    pub fn toggle_star(&self, ids: &[u64]) -> Result<Toggled> {
        let mut out = Toggled::default();
        self.update_items(ids, false, |id, item| {
            let starred = !item.is_starred();
            item.set_starred(starred);
            let bucket = if starred { &mut out.on } else { &mut out.off };
            bucket.push(id);
        })?;
        Ok(out)
    }

    pub fn set_task_state(&self, id: u64, state: TaskState) -> Result<()> {
        self.update_tasks(&[id], |_, task| {
            task.is_complete = state == TaskState::Done;
            task.in_progress = state == TaskState::InProgress;
        })?;
        Ok(())
    }

    pub fn set_priority(&self, ids: &[u64], priority: Priority) -> Result<Vec<u64>> {
        self.update_tasks(ids, |_, task| task.priority = priority)
    }

    pub fn set_due_date(&self, ids: &[u64], due_date: Option<i64>) -> Result<Vec<u64>> {
        self.update_tasks(ids, |_, task| task.due_date = due_date)
    }

    /// Put items on exactly `boards`. Returns the normalized board names.
    pub fn move_to_boards(&self, ids: &[u64], boards: Vec<String>) -> Result<Vec<String>> {
        if boards.is_empty() {
            return Err(TaskbookError::MissingBoard);
        }
        let boards = normalize_boards(boards);
        self.update_items(ids, false, |_, item| item.set_boards(boards.clone()))?;
        Ok(boards)
    }

    /// Add and remove tags. Returns the normalized (added, removed) tags.
    pub fn update_tags(
        &self,
        ids: &[u64],
        add: Vec<String>,
        remove: Vec<String>,
    ) -> Result<(Vec<String>, Vec<String>)> {
        let add = normalize_tags(add);
        let remove = normalize_tags(remove);
        if add.is_empty() && remove.is_empty() {
            return Err(TaskbookError::MissingTags);
        }
        self.update_items(ids, false, |_, item| {
            let mut tags: Vec<String> = item.tags().to_vec();
            tags.retain(|t| !remove.iter().any(|r| t.eq_ignore_ascii_case(r)));
            for tag in &add {
                if !tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
                    tags.push(tag.clone());
                }
            }
            item.set_tags(tags);
        })?;
        Ok((add, remove))
    }

    /// Apply a partial update to one item in a single save. Task-only fields
    /// on a note are rejected before anything changes.
    pub fn edit(&self, id: u64, edit: ItemEdit) -> Result<()> {
        let ItemEdit {
            description,
            priority,
            due_date,
            boards,
            tags,
            starred,
        } = edit;
        let description = description.map(non_empty).transpose()?;
        if boards.as_ref().is_some_and(Vec::is_empty) {
            return Err(TaskbookError::MissingBoard);
        }
        let boards = boards.map(normalize_boards);
        let tags = tags.map(normalize_tags);

        let tasks_only = priority.is_some() || due_date.is_some();
        self.update_items(&[id], tasks_only, |_, item| {
            if let Some(d) = &description {
                item.set_description(d.clone());
            }
            if let Some(b) = &boards {
                item.set_boards(b.clone());
            }
            if let Some(t) = &tags {
                item.set_tags(t.clone());
            }
            if let Some(s) = starred {
                item.set_starred(s);
            }
            if let Some(task) = item.as_task_mut() {
                if let Some(p) = priority {
                    task.priority = p;
                }
                if let Some(d) = due_date {
                    task.due_date = d;
                }
            }
        })?;
        Ok(())
    }

    pub fn edit_description(&self, id: u64, description: String) -> Result<()> {
        self.edit(
            id,
            ItemEdit {
                description: Some(description),
                ..ItemEdit::default()
            },
        )
    }

    /// The (title, body) of a note, for opening it in an editor.
    pub fn note_content(&self, id: u64) -> Result<(String, Option<String>)> {
        let data = self.items()?;
        resolve_ids(&data, &[id])?;
        let note = data[&id.to_string()]
            .as_note()
            .ok_or(TaskbookError::NotANote(id))?;
        Ok((note.title().to_string(), note.body().map(str::to_string)))
    }

    pub fn set_note_content(&self, id: u64, title: String, body: Option<String>) -> Result<()> {
        let title = non_empty(title)?;
        let mut data = self.items()?;
        resolve_ids(&data, &[id])?;
        let item = data.get_mut(&id.to_string()).expect("resolved");
        if !item.set_note_body(body) {
            return Err(TaskbookError::NotANote(id));
        }
        item.set_description(title);
        self.storage.set(&data)
    }

    /// Move items to the archive. Returns the (deduped) ids that were deleted.
    pub fn delete(&self, ids: &[u64]) -> Result<Vec<u64>> {
        let mut data = self.items()?;
        let ids = resolve_ids(&data, ids)?;
        let mut archive = self.archived_items()?;
        for id in &ids {
            let mut item = data.remove(&id.to_string()).expect("resolved");
            let archive_id = next_id(&archive);
            item.set_id(archive_id);
            archive.insert(archive_id.to_string(), item);
        }
        // Archive first: a failure between the two writes duplicates rather
        // than loses items.
        self.storage.set_archive(&archive)?;
        self.storage.set(&data)?;
        Ok(ids)
    }

    /// Move archived items back. Returns the (deduped) archive ids restored.
    pub fn restore(&self, ids: &[u64]) -> Result<Vec<u64>> {
        let mut archive = self.archived_items()?;
        let ids = resolve_ids(&archive, ids)?;
        let mut data = self.items()?;
        for id in &ids {
            let mut item = archive.remove(&id.to_string()).expect("resolved");
            let new_id = next_id(&data);
            item.set_id(new_id);
            data.insert(new_id.to_string(), item);
        }
        self.storage.set(&data)?;
        self.storage.set_archive(&archive)?;
        Ok(ids)
    }

    /// Archive every completed task. Returns their ids (ascending).
    pub fn clear_completed(&self) -> Result<Vec<u64>> {
        let data = self.items()?;
        let mut done: Vec<u64> = data
            .values()
            .filter(|item| item.as_task().is_some_and(|t| t.is_complete))
            .map(StorageItem::id)
            .collect();
        if done.is_empty() {
            return Ok(done);
        }
        done.sort_unstable();
        self.delete(&done)
    }

    /// Rename a board across all items. Returns the number of items changed.
    pub fn rename_board(&self, old_name: &str, new_name: &str) -> Result<usize> {
        let mut data = self.items()?;
        let old_name = board::normalize_board_name(old_name);
        let new_name = board::normalize_board_name(new_name);
        let mut count = 0;
        for item in data.values_mut() {
            if item.boards().iter().any(|b| board::board_eq(b, &old_name)) {
                let boards = item
                    .boards()
                    .iter()
                    .map(|b| {
                        if board::board_eq(b, &old_name) {
                            new_name.clone()
                        } else {
                            b.clone()
                        }
                    })
                    .collect();
                // Dedupes when the item was already on the new board.
                item.set_boards(normalize_boards(boards));
                count += 1;
            }
        }
        if count > 0 {
            self.storage.set(&data)?;
        }
        Ok(count)
    }

    /// Copy item descriptions (newline-separated) to the system clipboard.
    pub fn copy_to_clipboard(&self, ids: &[u64]) -> Result<Vec<u64>> {
        let data = self.items()?;
        let ids = resolve_ids(&data, ids)?;
        let text = ids
            .iter()
            .map(|id| data[&id.to_string()].description())
            .collect::<Vec<_>>()
            .join("\n");
        Clipboard::new()
            .and_then(|mut c| c.set_text(text))
            .map_err(|e| TaskbookError::Clipboard(e.to_string()))?;
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taskbook() -> Taskbook {
        let dir = std::env::temp_dir().join(format!("tb-core-test-{}", uuid::Uuid::new_v4()));
        Taskbook::with_storage(Box::new(LocalStorage::new(&dir).unwrap()))
    }

    fn task(tb: &Taskbook, desc: &str) -> u64 {
        tb.create_task(NewTask {
            description: desc.into(),
            ..NewTask::default()
        })
        .unwrap()
    }

    fn note(tb: &Taskbook, title: &str) -> u64 {
        tb.create_note(NewNote {
            title: title.into(),
            ..NewNote::default()
        })
        .unwrap()
    }

    fn get(tb: &Taskbook, id: u64) -> StorageItem {
        tb.items().unwrap()[&id.to_string()].clone()
    }

    #[test]
    fn test_create_rejects_blank_description() {
        let tb = taskbook();
        let err = tb
            .create_task(NewTask {
                description: "   ".into(),
                ..NewTask::default()
            })
            .unwrap_err();
        assert!(matches!(err, TaskbookError::MissingDescription));
    }

    #[test]
    fn test_create_defaults_board_and_priority() {
        let tb = taskbook();
        let id = task(&tb, "a");
        let item = get(&tb, id);
        assert_eq!(item.boards(), [DEFAULT_BOARD]);
        assert_eq!(item.as_task().unwrap().priority, Priority::Normal);
    }

    #[test]
    fn test_unknown_ids_reports_all_and_changes_nothing() {
        let tb = taskbook();
        let id = task(&tb, "a");
        let err = tb.toggle_check(&[id, 7, 9]).unwrap_err();
        assert!(matches!(err, TaskbookError::UnknownIds(ref ids) if ids == &[7, 9]));
        assert!(!get(&tb, id).as_task().unwrap().is_complete);
    }

    #[test]
    fn test_empty_ids_is_missing_id() {
        let tb = taskbook();
        assert!(matches!(
            tb.toggle_star(&[]).unwrap_err(),
            TaskbookError::Id(IdError::Missing)
        ));
    }

    #[test]
    fn test_task_only_ops_reject_notes_atomically() {
        let tb = taskbook();
        let t = task(&tb, "a");
        let n = note(&tb, "n");
        let err = tb.set_priority(&[t, n], Priority::High).unwrap_err();
        assert!(matches!(err, TaskbookError::NotATask(ref ids) if ids == &[n]));
        assert_eq!(get(&tb, t).as_task().unwrap().priority, Priority::Normal);
        assert!(matches!(
            tb.toggle_check(&[n]).unwrap_err(),
            TaskbookError::NotATask(_)
        ));
    }

    #[test]
    fn test_toggle_reports_both_directions() {
        let tb = taskbook();
        let a = task(&tb, "a");
        let b = task(&tb, "b");
        tb.toggle_check(&[a]).unwrap();
        let out = tb.toggle_check(&[a, b, a]).unwrap();
        assert_eq!(
            out,
            Toggled {
                on: vec![b],
                off: vec![a]
            }
        );
    }

    #[test]
    fn test_multi_id_setters() {
        let tb = taskbook();
        let a = task(&tb, "a");
        let b = task(&tb, "b");
        tb.set_priority(&[a, b], Priority::High).unwrap();
        let boards = tb.move_to_boards(&[a, b], vec!["@Work".into()]).unwrap();
        assert_eq!(boards, ["Work"]);
        tb.update_tags(&[a, b], vec!["+x".into()], vec![]).unwrap();
        for id in [a, b] {
            let item = get(&tb, id);
            assert_eq!(item.as_task().unwrap().priority, Priority::High);
            assert_eq!(item.boards(), ["Work"]);
            assert_eq!(item.tags(), ["x"]);
        }
    }

    #[test]
    fn test_edit_rejects_task_fields_on_note_without_partial_update() {
        let tb = taskbook();
        let n = note(&tb, "n");
        let err = tb
            .edit(
                n,
                ItemEdit {
                    description: Some("changed".into()),
                    priority: Some(Priority::High),
                    ..ItemEdit::default()
                },
            )
            .unwrap_err();
        assert!(matches!(err, TaskbookError::NotATask(_)));
        assert_eq!(get(&tb, n).description(), "n");
    }

    #[test]
    fn test_delete_restore_round_trip_and_clear() {
        let tb = taskbook();
        let a = task(&tb, "a");
        let b = task(&tb, "b");
        assert_eq!(tb.delete(&[a, a]).unwrap(), vec![a]);
        assert_eq!(tb.archived_items().unwrap().len(), 1);
        tb.restore(&[1]).unwrap();
        assert!(tb.archived_items().unwrap().is_empty());
        assert_eq!(tb.items().unwrap().len(), 2);

        tb.toggle_check(&[b]).unwrap();
        assert_eq!(tb.clear_completed().unwrap(), vec![b]);
        assert!(tb.clear_completed().unwrap().is_empty());
    }

    #[test]
    fn test_get_returns_requested_items_in_order() {
        let tb = taskbook();
        let a = task(&tb, "a");
        let b = note(&tb, "b");
        let items = tb.get(&[b, a, b], false).unwrap();
        let ids: Vec<u64> = items.iter().map(StorageItem::id).collect();
        assert_eq!(ids, vec![b, a]);
        assert!(matches!(
            tb.get(&[a], true).unwrap_err(),
            TaskbookError::UnknownIds(_)
        ));
    }

    #[test]
    fn test_note_content_requires_note() {
        let tb = taskbook();
        let t = task(&tb, "a");
        assert!(matches!(
            tb.note_content(t).unwrap_err(),
            TaskbookError::NotANote(_)
        ));
        let n = note(&tb, "title");
        tb.set_note_content(n, "new".into(), Some("body".into()))
            .unwrap();
        assert_eq!(
            tb.note_content(n).unwrap(),
            ("new".to_string(), Some("body".to_string()))
        );
    }
}
