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
