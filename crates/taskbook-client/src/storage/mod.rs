#[cfg(feature = "ditto")]
mod ditto;
mod local;
mod remote;

#[cfg(feature = "ditto")]
pub use ditto::DittoStorage;
pub use local::LocalStorage;
pub use remote::RemoteStorage;

use std::any::Any;
use std::collections::HashMap;
use std::path::Path;

use crate::config::{Config, SyncBackend};
use crate::directory::resolve_taskbook_directory;
use crate::error::Result;
use taskbook_common::StorageItem;

/// Which logical collection changed, as reported through [`StorageBackend::watch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // only constructed by backends with push notifications
pub enum ChangeNotice {
    Active,
    Archive,
}

/// Callback invoked by a backend when another writer (device or process)
/// changed the data. May be called from a background thread.
pub type ChangeCallback = Box<dyn Fn(ChangeNotice) + Send + Sync + 'static>;

/// Keeps a change watch registered for as long as it is alive; dropping it
/// unregisters the watch.
pub struct WatchHandle(#[allow(dead_code)] Box<dyn Any>);

impl WatchHandle {
    #[allow(dead_code)] // only used by backends with push notifications
    pub fn new(inner: impl Any) -> Self {
        Self(Box::new(inner))
    }
}

/// Trait abstracting storage backends (local file, remote server, etc.)
pub trait StorageBackend {
    fn get(&self) -> Result<HashMap<String, StorageItem>>;
    fn get_archive(&self) -> Result<HashMap<String, StorageItem>>;
    fn set(&self, data: &HashMap<String, StorageItem>) -> Result<()>;
    fn set_archive(&self, data: &HashMap<String, StorageItem>) -> Result<()>;

    /// Subscribe to change notifications pushed by the backend. Backends
    /// without push notifications return `Ok(None)` and callers fall back to
    /// whatever polling or out-of-band mechanism they have.
    fn watch(&self, _on_change: ChangeCallback) -> Result<Option<WatchHandle>> {
        Ok(None)
    }
}

/// Build the backend the config selects: the sync backend when
/// `sync.enabled` is true, local files otherwise.
pub fn from_config(
    config: &Config,
    taskbook_dir: Option<&Path>,
) -> Result<Box<dyn StorageBackend>> {
    if config.sync.enabled {
        sync_backend(config)
    } else {
        let resolved_dir = resolve_taskbook_directory(taskbook_dir)?;
        Ok(Box::new(LocalStorage::new(&resolved_dir)?))
    }
}

/// Build the configured sync backend regardless of `sync.enabled`
/// (used by `--migrate`, which runs before sync is switched on).
pub fn sync_backend(config: &Config) -> Result<Box<dyn StorageBackend>> {
    match config.sync.backend {
        SyncBackend::Server => Ok(Box::new(RemoteStorage::new(&config.sync.server_url)?)),
        SyncBackend::Ditto => ditto_backend(config),
    }
}

#[cfg(feature = "ditto")]
fn ditto_backend(config: &Config) -> Result<Box<dyn StorageBackend>> {
    Ok(Box::new(DittoStorage::new(&config.ditto)?))
}

/// Ditto SDK release the native bindings were built against, when compiled in.
pub fn ditto_sdk_version() -> Option<&'static str> {
    #[cfg(feature = "ditto")]
    {
        Some(ditto::sdk::SDK_VERSION)
    }
    #[cfg(not(feature = "ditto"))]
    {
        None
    }
}

#[cfg(not(feature = "ditto"))]
fn ditto_backend(_config: &Config) -> Result<Box<dyn StorageBackend>> {
    Err(crate::error::TaskbookError::General(
        "this build of tb has no Ditto support; rebuild with `cargo build --features ditto`"
            .to_string(),
    ))
}
