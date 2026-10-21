/// Common trait for all items (tasks and notes)
pub trait Item {
    fn id(&self) -> u64;
    fn date(&self) -> &str;
    fn timestamp(&self) -> i64;
    fn description(&self) -> &str;
    fn is_starred(&self) -> bool;
    fn boards(&self) -> &[String];
    fn tags(&self) -> &[String];
    fn is_task(&self) -> bool;

    /// The item's stable ID, derived from its creation timestamp.
    /// See [`crate::stable_id`].
    fn stable_id(&self) -> String {
        crate::stable_id::from_timestamp(self.timestamp())
    }
}
