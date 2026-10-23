use thiserror::Error;

use taskbook_common::board::CliParseError;
use taskbook_common::id::IdError;
use taskbook_common::priority::ParsePriorityError;

/// Every user-facing failure. `Display` is the complete message: callers
/// print it once and never render a second, separate explanation.
#[derive(Error, Debug)]
pub enum TaskbookError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Id(#[from] IdError),

    #[error("{}", unknown_ids_message(.0))]
    UnknownIds(Vec<u64>),

    #[error("{}", not_a_task_message(.0))]
    NotATask(Vec<u64>),

    #[error("item {0} is a task, not a note")]
    NotANote(u64),

    #[error("usage: {0}")]
    Usage(&'static str),

    #[error("no description given")]
    MissingDescription,

    #[error("no board given (name one with @board)")]
    MissingBoard,

    #[error("no board named {0}")]
    UnknownBoard(String),

    #[error(transparent)]
    InvalidPriority(#[from] ParsePriorityError),

    #[error("{}", taskbook_common::due::invalid_due_date_message(.0))]
    InvalidDueDate(String),

    #[error("no due date given (use a date, or none to clear it)")]
    MissingDueDate,

    #[error("notes cannot have a {0}")]
    NoteField(&'static str),

    #[error("no tags given (use +tag to add, -tag to remove)")]
    MissingTags,

    #[error("expected +tag or -tag, got '{0}'")]
    InvalidTag(String),

    #[error(
        "unknown filter '{0}' (use pending, progress, done, task, note, starred, +tag or @board)"
    )]
    UnknownFilter(String),

    #[error("Invalid custom directory: {0}")]
    InvalidDirectory(String),

    #[error("Missing taskbook-dir flag value")]
    MissingTaskbookDirValue,

    #[error("Clipboard error: {0}")]
    Clipboard(String),

    #[error("TUI error: {0}")]
    Tui(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("{0}")]
    General(String),
}

impl From<CliParseError> for TaskbookError {
    fn from(e: CliParseError) -> Self {
        match e {
            CliParseError::InvalidDueDate(raw) => TaskbookError::InvalidDueDate(raw),
            CliParseError::InvalidPriority(e) => TaskbookError::InvalidPriority(e),
        }
    }
}

fn join_ids(ids: &[u64]) -> String {
    ids.iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn unknown_ids_message(ids: &[u64]) -> String {
    match ids {
        [id] => format!("no item with id {id}"),
        _ => format!("no items with ids {}", join_ids(ids)),
    }
}

fn not_a_task_message(ids: &[u64]) -> String {
    match ids {
        [id] => format!("item {id} is a note, not a task"),
        _ => format!("items {} are notes, not tasks", join_ids(ids)),
    }
}

pub type Result<T> = std::result::Result<T, TaskbookError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_messages_pluralize() {
        assert_eq!(
            TaskbookError::UnknownIds(vec![4]).to_string(),
            "no item with id 4"
        );
        assert_eq!(
            TaskbookError::UnknownIds(vec![4, 5]).to_string(),
            "no items with ids 4, 5"
        );
        assert_eq!(
            TaskbookError::NotATask(vec![2, 3]).to_string(),
            "items 2, 3 are notes, not tasks"
        );
    }
}
