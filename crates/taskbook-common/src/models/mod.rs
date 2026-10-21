mod item;
mod note;
mod task;

pub use item::Item;
pub use note::Note;
pub use task::Task;

use serde::Serialize;

/// Unified storage item that can be either a Task or Note.
///
/// Serialization uses serde's untagged representation (inner type serialized directly).
/// Deserialization uses the `_isTask` field as an explicit discriminator for robustness.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum StorageItem {
    Task(Task),
    Note(Note),
}

impl<'de> serde::Deserialize<'de> for StorageItem {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        let is_task = value
            .get("_isTask")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true); // default to task for backward compatibility

        if is_task {
            serde_json::from_value(value)
                .map(StorageItem::Task)
                .map_err(serde::de::Error::custom)
        } else {
            serde_json::from_value(value)
                .map(StorageItem::Note)
                .map_err(serde::de::Error::custom)
        }
    }
}

impl Item for StorageItem {
    fn id(&self) -> u64 {
        match self {
            StorageItem::Task(t) => t.id,
            StorageItem::Note(n) => n.id,
        }
    }

    fn date(&self) -> &str {
        match self {
            StorageItem::Task(t) => &t.date,
            StorageItem::Note(n) => &n.date,
        }
    }

    fn timestamp(&self) -> i64 {
        match self {
            StorageItem::Task(t) => t.timestamp,
            StorageItem::Note(n) => n.timestamp,
        }
    }

    fn description(&self) -> &str {
        match self {
            StorageItem::Task(t) => &t.description,
            StorageItem::Note(n) => &n.description,
        }
    }

    fn is_starred(&self) -> bool {
        match self {
            StorageItem::Task(t) => t.is_starred,
            StorageItem::Note(n) => n.is_starred,
        }
    }

    fn boards(&self) -> &[String] {
        match self {
            StorageItem::Task(t) => &t.boards,
            StorageItem::Note(n) => &n.boards,
        }
    }

    fn tags(&self) -> &[String] {
        match self {
            StorageItem::Task(t) => &t.tags,
            StorageItem::Note(n) => &n.tags,
        }
    }

    fn is_task(&self) -> bool {
        matches!(self, StorageItem::Task(_))
    }
}

// Inherent methods that mirror the Item trait — these allow callers to use
// StorageItem without importing the Item trait, while the trait impl above
// enables polymorphic usage via `&dyn Item`.
impl StorageItem {
    pub fn id(&self) -> u64 {
        match self {
            StorageItem::Task(t) => t.id,
            StorageItem::Note(n) => n.id,
        }
    }

    pub fn date(&self) -> &str {
        match self {
            StorageItem::Task(t) => &t.date,
            StorageItem::Note(n) => &n.date,
        }
    }

    pub fn timestamp(&self) -> i64 {
        match self {
            StorageItem::Task(t) => t.timestamp,
            StorageItem::Note(n) => n.timestamp,
        }
    }

    pub fn description(&self) -> &str {
        match self {
            StorageItem::Task(t) => &t.description,
            StorageItem::Note(n) => &n.description,
        }
    }

    pub fn is_starred(&self) -> bool {
        match self {
            StorageItem::Task(t) => t.is_starred,
            StorageItem::Note(n) => n.is_starred,
        }
    }

    pub fn boards(&self) -> &[String] {
        match self {
            StorageItem::Task(t) => &t.boards,
            StorageItem::Note(n) => &n.boards,
        }
    }

    pub fn is_task(&self) -> bool {
        matches!(self, StorageItem::Task(_))
    }

    /// The item's stable ID, derived from its creation timestamp.
    /// See [`crate::stable_id`].
    pub fn stable_id(&self) -> String {
        crate::stable_id::from_timestamp(self.timestamp())
    }

    pub fn set_description(&mut self, desc: String) {
        match self {
            StorageItem::Task(t) => t.description = desc,
            StorageItem::Note(n) => n.description = desc,
        }
    }

    pub fn set_starred(&mut self, starred: bool) {
        match self {
            StorageItem::Task(t) => t.is_starred = starred,
            StorageItem::Note(n) => n.is_starred = starred,
        }
    }

    pub fn set_boards(&mut self, boards: Vec<String>) {
        match self {
            StorageItem::Task(t) => t.boards = boards,
            StorageItem::Note(n) => n.boards = boards,
        }
    }

    pub fn tags(&self) -> &[String] {
        match self {
            StorageItem::Task(t) => &t.tags,
            StorageItem::Note(n) => &n.tags,
        }
    }

    pub fn set_tags(&mut self, tags: Vec<String>) {
        match self {
            StorageItem::Task(t) => t.tags = tags,
            StorageItem::Note(n) => n.tags = tags,
        }
    }

    pub fn as_task(&self) -> Option<&Task> {
        match self {
            StorageItem::Task(t) => Some(t),
            StorageItem::Note(_) => None,
        }
    }

    pub fn as_task_mut(&mut self) -> Option<&mut Task> {
        match self {
            StorageItem::Task(t) => Some(t),
            StorageItem::Note(_) => None,
        }
    }

    pub fn as_note(&self) -> Option<&Note> {
        match self {
            StorageItem::Task(_) => None,
            StorageItem::Note(n) => Some(n),
        }
    }

    pub fn as_note_mut(&mut self) -> Option<&mut Note> {
        match self {
            StorageItem::Task(_) => None,
            StorageItem::Note(n) => Some(n),
        }
    }

    /// Set the note body content. Returns false if item is not a note.
    pub fn set_note_body(&mut self, body: Option<String>) -> bool {
        match self {
            StorageItem::Note(n) => {
                n.set_body(body);
                true
            }
            StorageItem::Task(_) => false,
        }
    }

    /// Get the note body content, if this is a note with body
    pub fn note_body(&self) -> Option<&str> {
        match self {
            StorageItem::Note(n) => n.body(),
            StorageItem::Task(_) => None,
        }
    }

    /// Check if this note has body content
    pub fn note_has_body(&self) -> bool {
        match self {
            StorageItem::Note(n) => n.has_body(),
            StorageItem::Task(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stable_id;

    fn task_at(ts: i64) -> StorageItem {
        let mut t = Task::new(3, "Write report".to_string(), vec!["Work".to_string()], 1);
        t.timestamp = ts;
        StorageItem::Task(t)
    }

    fn note_at(ts: i64) -> StorageItem {
        let mut n = Note::new(4, "Meeting notes".to_string(), vec!["Work".to_string()]);
        n.timestamp = ts;
        StorageItem::Note(n)
    }

    #[test]
    fn stable_id_is_derived_from_timestamp_only() {
        let ts = 1_790_000_000_000;
        assert_eq!(task_at(ts).stable_id(), stable_id::from_timestamp(ts));
        assert_eq!(task_at(ts).stable_id(), note_at(ts).stable_id());

        let via_trait: &dyn Item = &task_at(ts);
        assert_eq!(via_trait.stable_id(), stable_id::from_timestamp(ts));
    }

    #[test]
    fn stable_id_survives_every_mutation_but_timestamp() {
        let mut item = task_at(1_790_000_000_000);
        let before = item.stable_id();

        item.set_description("Rewritten".to_string());
        item.set_starred(true);
        item.set_boards(vec!["Home".to_string()]);
        item.set_tags(vec!["urgent".to_string()]);
        if let Some(t) = item.as_task_mut() {
            t.is_complete = true;
            t.in_progress = true;
            t.id = 99;
        }

        assert_eq!(item.stable_id(), before);
    }

    #[test]
    fn stable_id_round_trips_without_a_stored_field() {
        let item = note_at(1_790_000_000_123);
        let json = serde_json::to_string(&item).unwrap();
        assert!(
            !json.contains(&item.stable_id()),
            "stable id must not be serialized: {json}"
        );

        let back: StorageItem = serde_json::from_str(&json).unwrap();
        assert_eq!(back.stable_id(), item.stable_id());
    }
}
