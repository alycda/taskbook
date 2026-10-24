mod local;
mod remote;

pub use local::LocalStorage;
pub use remote::RemoteStorage;

use std::collections::BTreeMap;

use crate::error::Result;
use taskbook_common::StorageItem;

/// Items keyed by numeric ID. A `BTreeMap` keeps iteration, and therefore the
/// order items are written and listed in, ascending by ID.
pub type Items = BTreeMap<u64, StorageItem>;

/// Trait abstracting storage backends (local file, remote server, etc.)
pub trait StorageBackend {
    fn get(&self) -> Result<Items>;
    fn get_archive(&self) -> Result<Items>;
    fn set(&self, data: &Items) -> Result<()>;
    fn set_archive(&self, data: &Items) -> Result<()>;
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::Path;

    use taskbook_common::Task;

    /// IDs 1..=20 in a fixed non-sorted order (7 is coprime to 20).
    pub fn permuted_ids() -> Vec<u64> {
        (0..20).map(|i| (i * 7) % 20 + 1).collect()
    }

    /// Writes a storage file holding `tasks`, keyed by ID in the given order.
    pub fn write_tasks(path: &Path, tasks: &[Task]) {
        let entries: Vec<String> = tasks
            .iter()
            .map(|task| format!("\"{}\": {}", task.id, serde_json::to_string(task).unwrap()))
            .collect();
        std::fs::write(path, format!("{{{}}}", entries.join(","))).unwrap();
    }
}
