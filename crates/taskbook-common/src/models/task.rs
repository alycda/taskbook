use serde::{Deserialize, Serialize};

use super::item::Item;
use crate::board;
use crate::priority::Priority;

/// A task item with completion status and priority
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "TaskRepr", into = "TaskRepr")]
pub struct Task {
    pub id: u64,
    pub date: String,
    pub timestamp: i64,
    pub is_task_flag: bool,
    pub description: String,
    pub is_starred: bool,
    pub is_complete: bool,
    pub in_progress: bool,
    pub priority: Priority,
    /// Due date as epoch millis.
    pub due_date: Option<i64>,
    pub boards: Vec<String>,
    pub tags: Vec<String>,
}

/// On-disk / on-wire shape of a [`Task`].
///
/// Field names match the original Node.js taskbook. Priority is stored twice:
/// `_priority` (1 = high … 3 = normal) is authoritative; the legacy
/// `priority` field (1 = normal … 3 = high) is written for 1.x clients and
/// read only when `_priority` is absent. A 1.x client re-saving an item drops
/// `_priority` (it does not know the field), so the two never disagree.
#[derive(Serialize, Deserialize)]
struct TaskRepr {
    #[serde(rename = "_id")]
    id: u64,
    #[serde(rename = "_date")]
    date: String,
    #[serde(rename = "_timestamp")]
    timestamp: i64,
    #[serde(rename = "_isTask")]
    is_task_flag: bool,
    description: String,
    #[serde(rename = "isStarred")]
    is_starred: bool,
    #[serde(rename = "isComplete")]
    is_complete: bool,
    #[serde(rename = "inProgress")]
    in_progress: bool,
    #[serde(rename = "_priority", default, skip_serializing_if = "Option::is_none")]
    priority: Option<u8>,
    #[serde(rename = "priority", default, skip_serializing_if = "Option::is_none")]
    legacy_priority: Option<u8>,
    #[serde(rename = "_dueDate", default, skip_serializing_if = "Option::is_none")]
    due_date: Option<i64>,
    #[serde(deserialize_with = "board::deserialize_boards")]
    boards: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
}

impl From<TaskRepr> for Task {
    fn from(r: TaskRepr) -> Self {
        let priority = match (r.priority, r.legacy_priority) {
            (Some(p), _) => Priority::from_stored(p),
            (None, Some(legacy)) => Priority::from_legacy(legacy),
            (None, None) => Priority::default(),
        };
        Self {
            id: r.id,
            date: r.date,
            timestamp: r.timestamp,
            is_task_flag: r.is_task_flag,
            description: r.description,
            is_starred: r.is_starred,
            is_complete: r.is_complete,
            in_progress: r.in_progress,
            priority,
            due_date: r.due_date,
            boards: r.boards,
            tags: r.tags,
        }
    }
}

impl From<Task> for TaskRepr {
    fn from(t: Task) -> Self {
        Self {
            id: t.id,
            date: t.date,
            timestamp: t.timestamp,
            is_task_flag: t.is_task_flag,
            description: t.description,
            is_starred: t.is_starred,
            is_complete: t.is_complete,
            in_progress: t.in_progress,
            priority: Some(t.priority.level()),
            legacy_priority: Some(t.priority.to_legacy()),
            due_date: t.due_date,
            boards: t.boards,
            tags: t.tags,
        }
    }
}

impl Task {
    pub fn new(id: u64, description: String, boards: Vec<String>, priority: Priority) -> Self {
        let now = chrono::Local::now();
        Self {
            id,
            date: now.format("%a %b %d %Y").to_string(),
            timestamp: now.timestamp_millis(),
            is_task_flag: true,
            description,
            is_starred: false,
            is_complete: false,
            in_progress: false,
            priority,
            due_date: None,
            boards,
            tags: Vec::new(),
        }
    }

    /// Returns the task with the given due date set (epoch millis).
    pub fn with_due_date(mut self, due_date: Option<i64>) -> Self {
        self.due_date = due_date;
        self
    }

    /// Returns the task with the given tags.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

impl Item for Task {
    fn id(&self) -> u64 {
        self.id
    }

    fn date(&self) -> &str {
        &self.date
    }

    fn timestamp(&self) -> i64 {
        self.timestamp
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn is_starred(&self) -> bool {
        self.is_starred
    }

    fn boards(&self) -> &[String] {
        &self.boards
    }

    fn tags(&self) -> &[String] {
        &self.tags
    }

    fn is_task(&self) -> bool {
        self.is_task_flag
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGACY_HIGH: &str = r#"{"_id":1,"_date":"Fri Jun 12 2026","_timestamp":1,"_isTask":true,"description":"x","isStarred":false,"isComplete":false,"inProgress":false,"priority":3,"boards":["My Board"]}"#;

    fn task(priority: Priority) -> Task {
        Task::new(1, "T".to_string(), vec!["My Board".to_string()], priority)
    }

    #[test]
    fn test_task_is_task_uses_flag() {
        let task = task(Priority::Normal);
        assert!(task.is_task());
        assert!(task.is_task_flag);
    }

    #[test]
    fn test_due_date_serde_round_trip() {
        let task = task(Priority::Normal).with_due_date(Some(1_790_000_000_000));
        let json = serde_json::to_string(&task).unwrap();
        assert!(json.contains("_dueDate"));
        let back: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(back.due_date, Some(1_790_000_000_000));
    }

    #[test]
    fn test_task_without_due_date_omits_field_and_deserializes() {
        let json = serde_json::to_string(&task(Priority::Normal)).unwrap();
        assert!(!json.contains("_dueDate"));
        let back: Task = serde_json::from_str(LEGACY_HIGH).unwrap();
        assert_eq!(back.due_date, None);
    }

    #[test]
    fn test_legacy_priority_field_is_inverted_on_read() {
        let back: Task = serde_json::from_str(LEGACY_HIGH).unwrap();
        assert_eq!(back.priority, Priority::High);
        let normal = LEGACY_HIGH.replace(r#""priority":3"#, r#""priority":1"#);
        let back: Task = serde_json::from_str(&normal).unwrap();
        assert_eq!(back.priority, Priority::Normal);
    }

    #[test]
    fn test_missing_priority_defaults_to_normal() {
        let none = LEGACY_HIGH.replace(r#""priority":3,"#, "");
        let back: Task = serde_json::from_str(&none).unwrap();
        assert_eq!(back.priority, Priority::Normal);
    }

    #[test]
    fn test_new_priority_field_wins_over_legacy() {
        let both = LEGACY_HIGH.replace(r#""priority":3"#, r#""priority":3,"_priority":2"#);
        let back: Task = serde_json::from_str(&both).unwrap();
        assert_eq!(back.priority, Priority::Medium);
    }

    #[test]
    fn test_serialize_writes_both_priority_fields() {
        let value = serde_json::to_value(task(Priority::High)).unwrap();
        assert_eq!(value["_priority"], 1);
        assert_eq!(value["priority"], 3);
        let back: Task = serde_json::from_value(value).unwrap();
        assert_eq!(back.priority, Priority::High);
    }
}
