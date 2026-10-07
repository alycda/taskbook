//! Ditto peer-to-peer storage backend.
//!
//! Ditto is a CRDT database that syncs between devices over LAN, Bluetooth,
//! peer-to-peer Wi-Fi, or through a Ditto Cloud / Big Peer server. Taskbook
//! maps its whole-map [`StorageBackend`] contract onto Ditto like this:
//!
//! * One Ditto document per item in a single collection:
//!
//!   ```json
//!   {
//!     "_id": "<uuid>",          // stable identity across devices
//!     "tbId": 7,                // taskbook id (the storage map key)
//!     "archived": false,        // active items vs. archive
//!     "deleted": false,         // soft-delete tombstone
//!     "payload": "...",         // item JSON, or base64 AES-256-GCM ciphertext
//!     "nonce": "...",           // base64 nonce when encrypted, null otherwise
//!     "updatedAt": 1700000000000
//!   }
//!   ```
//!
//!   The payload is one opaque register so an item merges last-writer-wins
//!   as a whole, and so it can be encrypted client-side exactly like the
//!   HTTP server backend does. Field-level merging is deliberately given up.
//!
//! * `set` / `set_archive` diff the new map against the last read snapshot
//!   and only upsert changed items and soft-delete missing ones, inside one
//!   Ditto transaction. The collection is never replaced wholesale, which
//!   would fight the CRDT and clobber concurrent edits from other devices.
//!
//! * Taskbook ids are sequential (`max + 1`), so two devices working offline
//!   can both create item 8. Both documents survive the merge; on the next
//!   read the duplicates are renumbered to fresh ids and written back. Ids
//!   can therefore shift after a sync, but nothing is lost.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use dittolive_ditto::dql::QueryResult;
use dittolive_ditto::prelude::*;
use dittolive_ditto::sync::SyncSubscription;
use serde::{Deserialize, Serialize};
use serde_json::json;
use taskbook_common::encryption::{decrypt_item, encrypt_item, EncryptedItem};
use taskbook_common::StorageItem;

use super::{ChangeCallback, ChangeNotice, StorageBackend, WatchHandle};
use crate::config::{Config, DittoConfig as DittoSettings, DittoConnect};
use crate::credentials::DittoCredentials;
use crate::error::{Result, TaskbookError};

/// Environment variable that turns Ditto's own logging on (`error`, `warn`,
/// `info`, `debug`, `verbose`). Off by default so the TUI stays clean.
const LOG_ENV_VAR: &str = "TB_DITTO_LOG";

/// Grace period after the first peer shows up, so a one-shot CLI write has a
/// chance to be delivered before the process exits.
const FLUSH_GRACE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Doc {
    #[serde(rename = "_id")]
    id: String,
    tb_id: u64,
    archived: bool,
    #[serde(default)]
    deleted: bool,
    payload: String,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    updated_at: i64,
}

/// What was last read for one taskbook id; used to diff on write.
#[derive(Debug, Clone)]
struct Entry {
    doc_id: String,
    json: String,
}

type Snapshot = HashMap<String, Entry>;

#[derive(Default)]
struct Snapshots {
    active: Option<Snapshot>,
    archive: Option<Snapshot>,
}

impl Snapshots {
    fn slot(&mut self, archived: bool) -> &mut Option<Snapshot> {
        if archived {
            &mut self.archive
        } else {
            &mut self.active
        }
    }
}

pub struct DittoStorage {
    ditto: Ditto,
    rt: tokio::runtime::Runtime,
    collection: String,
    key: Option<[u8; 32]>,
    flush_timeout: Duration,
    sync_started: bool,
    _subscription: Option<Arc<SyncSubscription>>,
    snapshots: Mutex<Snapshots>,
    dirty: AtomicBool,
}

impl DittoStorage {
    /// Open the backend described by the config, using secrets from
    /// `~/.taskbook/ditto-credentials.json`, and start syncing.
    pub fn new(settings: &DittoSettings) -> Result<Self> {
        let creds = DittoCredentials::load()?.unwrap_or_default();

        let key = if settings.encrypt {
            Some(creds.encryption_key_bytes()?.ok_or_else(|| {
                general(
                    "ditto.encrypt is on but no encryption key is saved — run `tb --ditto-init`",
                )
            })?)
        } else {
            None
        };

        let connect = match settings.connect {
            DittoConnect::Peers => {
                let private_key = match &creds.private_key_path {
                    Some(path) => {
                        let path = Config::format_taskbook_dir(path);
                        Some(std::fs::read(&path).map_err(|e| {
                            general(format!(
                                "cannot read ditto private key {}: {e}",
                                path.display()
                            ))
                        })?)
                    }
                    None => None,
                };
                DittoConfigConnect::SmallPeersOnly { private_key }
            }
            DittoConnect::Server => {
                let url = settings
                    .url
                    .as_deref()
                    .filter(|u| !u.is_empty())
                    .ok_or_else(|| general("ditto.url is required for connect = \"server\""))?;
                let url = url
                    .parse()
                    .map_err(|e| general(format!("invalid ditto.url {url:?}: {e}")))?;
                DittoConfigConnect::Server { url }
            }
        };

        let license = match settings.connect {
            DittoConnect::Peers => Some(creds.license_token.clone().ok_or_else(|| {
                general(
                    "ditto connect = \"peers\" needs an offline license token from \
                     https://portal.ditto.live — run `tb --ditto-init`",
                )
            })?),
            DittoConnect::Server => None,
        };

        Self::open(settings, connect, key, creds.token.clone(), license, true)
    }

    /// Open a Ditto instance. `start_sync = false` keeps everything local
    /// (used by tests).
    fn open(
        settings: &DittoSettings,
        connect: DittoConfigConnect,
        key: Option<[u8; 32]>,
        token: Option<String>,
        license: Option<String>,
        start_sync: bool,
    ) -> Result<Self> {
        validate_collection_name(&settings.collection)?;
        if settings.app_id.is_empty() {
            return Err(general("ditto.appId is required"));
        }

        let dir = settings.persistence_path();
        std::fs::create_dir_all(&dir)?;

        let config = DittoConfig::new(settings.app_id.clone(), connect)
            .with_persistence_directory(absolute(&dir));

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;

        let ditto = {
            // Ditto's native init (triggered by the first SDK call) prints a
            // tracing-filter warning straight to stderr regardless of log
            // settings; keep it off the terminal for one-shot CLI commands
            // unless the user asked for Ditto logs.
            let _quiet = if std::env::var_os(LOG_ENV_VAR).is_some() {
                None
            } else {
                StderrSilencer::new()
            };
            configure_logging();
            let ditto = Ditto::open_sync(config).map_err(ditto_err)?;
            // Mutating DQL (INSERT/UPDATE) is only enabled once legacy v3 sync
            // is switched off; every peer here runs this version or newer.
            ditto.disable_sync_with_v3().map_err(ditto_err)?;
            ditto
        };

        // Offline (peers) identities must be activated with a license token
        // before sync can start; online identities activate through login.
        if let Some(license) = license.as_deref() {
            ditto
                .set_offline_only_license_token(license)
                .map_err(|e| general(format!("ditto: invalid license token: {e}")))?;
        }

        // `Server` connect mode authenticates with a token supplied from the
        // expiration handler, which Ditto also invokes for the initial login.
        if let Some(auth) = ditto.auth() {
            let token = token.ok_or_else(|| {
                general("ditto connect = \"server\" needs an auth token — run `tb --ditto-init`")
            })?;
            let provider = settings.provider.clone();
            auth.set_expiration_handler(move |ditto: &Ditto, _remaining: Duration| {
                let token = token.clone();
                let provider = provider.clone();
                let auth = ditto.auth();
                async move {
                    if let Some(auth) = auth {
                        if let Err(e) = auth.login(&token, &provider) {
                            eprintln!("ditto: login failed: {e}");
                        }
                    }
                }
            });
        }

        let subscription = if start_sync {
            let subscription = ditto
                .sync()
                .register_subscription_v2(format!("SELECT * FROM {}", settings.collection))
                .map_err(ditto_err)?;
            ditto.sync().start().map_err(ditto_err)?;
            Some(subscription)
        } else {
            None
        };

        Ok(Self {
            ditto,
            rt,
            collection: settings.collection.clone(),
            key,
            flush_timeout: Duration::from_millis(settings.flush_timeout_ms),
            sync_started: start_sync,
            _subscription: subscription,
            snapshots: Mutex::new(Snapshots::default()),
            dirty: AtomicBool::new(false),
        })
    }

    fn execute(&self, query: String, args: serde_json::Value) -> Result<QueryResult> {
        self.rt
            .block_on(self.ditto.store().execute_v2((query, args)))
            .map_err(ditto_err)
    }

    fn select_query(&self) -> String {
        format!(
            "SELECT * FROM {} WHERE archived = :archived AND deleted = false",
            self.collection
        )
    }

    fn upsert_query(&self) -> String {
        format!(
            "INSERT INTO {} DOCUMENTS (:doc) ON ID CONFLICT DO UPDATE",
            self.collection
        )
    }

    fn soft_delete_query(&self) -> String {
        format!(
            "UPDATE {} SET deleted = true, updatedAt = :now WHERE _id = :id",
            self.collection
        )
    }

    /// Serialize an item into a document, encrypting the payload when a key
    /// is configured.
    fn make_doc(
        &self,
        doc_id: String,
        tb_id: u64,
        archived: bool,
        item: &StorageItem,
        now: i64,
    ) -> Result<Doc> {
        let (payload, nonce) = match &self.key {
            Some(key) => {
                let engine = base64::engine::general_purpose::STANDARD;
                let encrypted = encrypt_item(key, item)
                    .map_err(|e| general(format!("encryption failed: {e}")))?;
                (
                    engine.encode(&encrypted.data),
                    Some(engine.encode(&encrypted.nonce)),
                )
            }
            None => (serde_json::to_string(item)?, None),
        };
        Ok(Doc {
            id: doc_id,
            tb_id,
            archived,
            deleted: false,
            payload,
            nonce,
            updated_at: now,
        })
    }

    fn decode(&self, doc: &Doc) -> Result<StorageItem> {
        match (&self.key, &doc.nonce) {
            (Some(key), Some(nonce)) => {
                let engine = base64::engine::general_purpose::STANDARD;
                let data = engine
                    .decode(&doc.payload)
                    .map_err(|e| general(format!("invalid base64 payload: {e}")))?;
                let nonce = engine
                    .decode(nonce)
                    .map_err(|e| general(format!("invalid base64 nonce: {e}")))?;
                decrypt_item(key, &EncryptedItem { data, nonce })
                    .map_err(|e| general(format!("decryption failed: {e}")))
            }
            (Some(_), None) => Err(general(format!(
                "document {} is not encrypted but ditto.encrypt is on",
                doc.id
            ))),
            (None, Some(_)) => Err(general(format!(
                "document {} is encrypted but ditto.encrypt is off",
                doc.id
            ))),
            (None, None) => Ok(serde_json::from_str(&doc.payload)?),
        }
    }

    /// Read one logical collection, resolving duplicate taskbook ids, and
    /// refresh the diff snapshot.
    fn read(&self, archived: bool) -> Result<HashMap<String, StorageItem>> {
        let result = self.execute(self.select_query(), json!({ "archived": archived }))?;

        let mut rows: Vec<(Doc, StorageItem)> = Vec::with_capacity(result.item_count());
        for row in result.iter() {
            let doc: Doc = row.deserialize_value().map_err(ditto_err)?;
            let item = self.decode(&doc)?;
            rows.push((doc, item));
        }

        // Deterministic order so every device resolves a collision the same
        // way: the oldest write keeps its id, later ones get renumbered.
        rows.sort_by(|a, b| {
            (a.0.tb_id, a.0.updated_at, &a.0.id).cmp(&(b.0.tb_id, b.0.updated_at, &b.0.id))
        });

        let mut next_id = rows.iter().map(|r| r.0.tb_id).max().unwrap_or(0);
        let mut seen = HashSet::with_capacity(rows.len());
        let mut renumbered = Vec::new();
        for (doc, item) in rows.iter_mut() {
            if !seen.insert(doc.tb_id) {
                next_id += 1;
                doc.tb_id = next_id;
                set_item_id(item, next_id);
                renumbered.push((doc.id.clone(), next_id, item.clone()));
            }
        }

        if !renumbered.is_empty() {
            let now = now_millis();
            let mut docs = Vec::with_capacity(renumbered.len());
            for (doc_id, tb_id, item) in &renumbered {
                docs.push(self.make_doc(doc_id.clone(), *tb_id, archived, item, now)?);
            }
            self.apply(docs, Vec::new())?;
        }

        let mut snapshot = Snapshot::with_capacity(rows.len());
        let mut data = HashMap::with_capacity(rows.len());
        for (doc, item) in rows {
            let key = doc.tb_id.to_string();
            snapshot.insert(
                key.clone(),
                Entry {
                    doc_id: doc.id,
                    json: serde_json::to_string(&item)?,
                },
            );
            data.insert(key, item);
        }

        *lock(&self.snapshots).slot(archived) = Some(snapshot);
        Ok(data)
    }

    /// Diff `data` against the last read snapshot and apply only the changes.
    fn write(&self, archived: bool, data: &HashMap<String, StorageItem>) -> Result<()> {
        // Bind the clone first: a guard living in the `match` scrutinee would
        // still be held while `read` below takes the same lock.
        let existing = lock(&self.snapshots).slot(archived).clone();
        let snapshot = match existing {
            Some(snapshot) => snapshot,
            None => {
                // No read yet (e.g. `--migrate`): establish the baseline so
                // existing documents are updated rather than duplicated.
                self.read(archived)?;
                lock(&self.snapshots)
                    .slot(archived)
                    .clone()
                    .unwrap_or_default()
            }
        };

        let now = now_millis();
        let mut upserts = Vec::new();
        let mut deletes = Vec::new();
        let mut next_snapshot = Snapshot::with_capacity(data.len());

        for (key, item) in data {
            let tb_id: u64 = key
                .parse()
                .map_err(|_| general(format!("non-numeric item key {key:?}")))?;
            let json = serde_json::to_string(item)?;
            let (doc_id, unchanged) = match snapshot.get(key) {
                Some(entry) => (entry.doc_id.clone(), entry.json == json),
                None => (uuid::Uuid::new_v4().to_string(), false),
            };
            if !unchanged {
                upserts.push(self.make_doc(doc_id.clone(), tb_id, archived, item, now)?);
            }
            next_snapshot.insert(key.clone(), Entry { doc_id, json });
        }

        for (key, entry) in &snapshot {
            if !data.contains_key(key) {
                deletes.push(entry.doc_id.clone());
            }
        }

        if !upserts.is_empty() || !deletes.is_empty() {
            self.apply(upserts, deletes)?;
            self.dirty.store(true, Ordering::SeqCst);
        }

        *lock(&self.snapshots).slot(archived) = Some(next_snapshot);
        Ok(())
    }

    /// Run upserts and soft-deletes in one transaction.
    fn apply(&self, upserts: Vec<Doc>, deletes: Vec<String>) -> Result<()> {
        let upsert_query = self.upsert_query();
        let delete_query = self.soft_delete_query();
        let now = now_millis();

        let _committed: TransactionCompletionAction = self
            .rt
            .block_on(self.ditto.store().transaction(async |tx| {
                for doc in &upserts {
                    tx.execute((upsert_query.as_str(), json!({ "doc": doc })))
                        .await?;
                }
                for id in &deletes {
                    tx.execute((delete_query.as_str(), json!({ "id": id, "now": now })))
                        .await?;
                }
                Ok::<_, DittoError>(TransactionCompletionAction::Commit)
            }))
            .map_err(ditto_err)?;
        Ok(())
    }

    /// Number of soft-deleted documents (for tests and diagnostics).
    #[cfg(test)]
    fn tombstone_count(&self) -> Result<usize> {
        let result = self.execute(
            format!("SELECT _id FROM {} WHERE deleted = true", self.collection),
            json!({}),
        )?;
        Ok(result.item_count())
    }
}

impl StorageBackend for DittoStorage {
    fn get(&self) -> Result<HashMap<String, StorageItem>> {
        self.read(false)
    }

    fn get_archive(&self) -> Result<HashMap<String, StorageItem>> {
        self.read(true)
    }

    fn set(&self, data: &HashMap<String, StorageItem>) -> Result<()> {
        self.write(false, data)
    }

    fn set_archive(&self, data: &HashMap<String, StorageItem>) -> Result<()> {
        self.write(true, data)
    }

    fn watch(&self, on_change: ChangeCallback) -> Result<Option<WatchHandle>> {
        let on_change = Arc::new(on_change);
        let mut observers = Vec::with_capacity(2);
        for (archived, notice) in [(false, ChangeNotice::Active), (true, ChangeNotice::Archive)] {
            let on_change = Arc::clone(&on_change);
            // Ditto delivers the current result set once right after
            // registration; skip that so the TUI doesn't reload needlessly.
            let mut first = true;
            let observer = self
                .ditto
                .store()
                .register_observer_v2(
                    (self.select_query(), json!({ "archived": archived })),
                    move |_result: QueryResult| {
                        if std::mem::take(&mut first) {
                            return;
                        }
                        on_change(notice);
                    },
                )
                .map_err(ditto_err)?;
            observers.push(observer);
        }
        Ok(Some(WatchHandle::new(observers)))
    }
}

impl Drop for DittoStorage {
    fn drop(&mut self) {
        // A one-shot CLI process would otherwise exit before Ditto has even
        // found a peer. Wait (bounded) for one, then a short grace period.
        if self.sync_started && self.dirty.load(Ordering::SeqCst) && !self.flush_timeout.is_zero() {
            let deadline = Instant::now() + self.flush_timeout;
            loop {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                if !self.ditto.presence().graph().remote_peers.is_empty() {
                    std::thread::sleep(FLUSH_GRACE.min(deadline - now));
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        if self.sync_started {
            self.ditto.sync().stop();
        }
    }
}

fn set_item_id(item: &mut StorageItem, id: u64) {
    match item {
        StorageItem::Task(t) => t.id = id,
        StorageItem::Note(n) => n.id = id,
    }
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn absolute(path: &Path) -> std::path::PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

/// Collection names are interpolated into DQL, so restrict them to plain
/// identifiers.
fn validate_collection_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let valid = match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
            chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(general(format!(
            "invalid ditto.collection {name:?}: use letters, digits and underscores only"
        )))
    }
}

/// Apply `TB_DITTO_LOG`; Ditto logging is off unless it is set.
fn configure_logging() {
    match std::env::var(LOG_ENV_VAR) {
        Ok(level) => {
            let level = match level.to_ascii_lowercase().as_str() {
                "error" => LogLevel::Error,
                "warn" | "warning" => LogLevel::Warning,
                "info" => LogLevel::Info,
                "debug" => LogLevel::Debug,
                _ => LogLevel::Verbose,
            };
            DittoLogger::set_logging_enabled(true);
            DittoLogger::set_minimum_log_level(level);
        }
        Err(_) => {
            DittoLogger::set_minimum_log_level(LogLevel::Error);
            DittoLogger::set_logging_enabled(false);
        }
    }
}

/// Points fd 2 at /dev/null for its lifetime and restores it on drop.
/// Best effort: any failure leaves stderr untouched.
struct StderrSilencer {
    #[cfg(unix)]
    saved: libc::c_int,
}

impl StderrSilencer {
    #[cfg(unix)]
    fn new() -> Option<Self> {
        use std::io::Write;
        let _ = std::io::stderr().flush();
        // SAFETY: plain POSIX fd calls on descriptors we own; errors are
        // checked and nothing is left half-swapped.
        unsafe {
            let saved = libc::dup(libc::STDERR_FILENO);
            if saved < 0 {
                return None;
            }
            let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY);
            if devnull < 0 {
                libc::close(saved);
                return None;
            }
            let swapped = libc::dup2(devnull, libc::STDERR_FILENO);
            libc::close(devnull);
            if swapped < 0 {
                libc::close(saved);
                return None;
            }
            Some(Self { saved })
        }
    }

    #[cfg(not(unix))]
    fn new() -> Option<Self> {
        None
    }
}

#[cfg(unix)]
impl Drop for StderrSilencer {
    fn drop(&mut self) {
        // SAFETY: `saved` is a descriptor this struct owns.
        unsafe {
            libc::dup2(self.saved, libc::STDERR_FILENO);
            libc::close(self.saved);
        }
    }
}

fn general(msg: impl Into<String>) -> TaskbookError {
    TaskbookError::General(msg.into())
}

fn ditto_err(e: DittoError) -> TaskbookError {
    TaskbookError::General(format!("ditto: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use taskbook_common::{Note, Task};

    fn temp_settings(encrypt: bool) -> DittoSettings {
        let dir = std::env::temp_dir().join(format!("tb-ditto-test-{}", uuid::Uuid::new_v4()));
        DittoSettings {
            app_id: "taskbook-test".to_string(),
            persistence_dir: Some(dir.to_string_lossy().to_string()),
            encrypt,
            ..DittoSettings::default()
        }
    }

    fn open_local(encrypt: bool) -> DittoStorage {
        let settings = temp_settings(encrypt);
        let key = encrypt.then(taskbook_common::encryption::generate_key);
        DittoStorage::open(
            &settings,
            DittoConfigConnect::SmallPeersOnly { private_key: None },
            key,
            None,
            None,
            false,
        )
        .expect("open ditto")
    }

    fn task(id: u64, desc: &str) -> StorageItem {
        StorageItem::Task(Task::new(id, desc.to_string(), vec!["My Board".into()], 1))
    }

    fn note(id: u64, desc: &str) -> StorageItem {
        StorageItem::Note(Note::new(id, desc.to_string(), vec!["My Board".into()]))
    }

    fn map(items: Vec<StorageItem>) -> HashMap<String, StorageItem> {
        items
            .into_iter()
            .map(|item| (item.id().to_string(), item))
            .collect()
    }

    #[test]
    fn empty_store_reads_empty() {
        let storage = open_local(false);
        assert!(storage.get().unwrap().is_empty());
        assert!(storage.get_archive().unwrap().is_empty());
    }

    #[test]
    fn round_trips_items_and_archive() {
        let storage = open_local(false);
        storage
            .set(&map(vec![task(1, "one"), note(2, "two")]))
            .unwrap();
        storage.set_archive(&map(vec![task(1, "old")])).unwrap();

        let items = storage.get().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items["1"].description(), "one");
        assert!(!items["2"].is_task());

        let archive = storage.get_archive().unwrap();
        assert_eq!(archive.len(), 1);
        assert_eq!(archive["1"].description(), "old");
    }

    #[test]
    fn writes_are_diffs_with_soft_deletes() {
        let storage = open_local(false);
        storage
            .set(&map(vec![task(1, "one"), task(2, "two")]))
            .unwrap();
        storage.get().unwrap();

        let mut edited = task(1, "one edited");
        if let StorageItem::Task(t) = &mut edited {
            t.is_complete = true;
        }
        storage.set(&map(vec![edited, task(3, "three")])).unwrap();

        let items = storage.get().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items["1"].description(), "one edited");
        assert!(items["1"].as_task().unwrap().is_complete);
        assert_eq!(items["3"].description(), "three");
        assert_eq!(storage.tombstone_count().unwrap(), 1);

        // The document identity for item 1 survived the edit.
        let snapshot = lock(&storage.snapshots).active.clone().unwrap();
        assert_eq!(snapshot.len(), 2);
    }

    #[test]
    fn keeps_document_identity_across_edits() {
        let storage = open_local(false);
        storage.set(&map(vec![task(1, "a")])).unwrap();
        let before = lock(&storage.snapshots).active.clone().unwrap()["1"]
            .doc_id
            .clone();
        storage.set(&map(vec![task(1, "b")])).unwrap();
        let after = lock(&storage.snapshots).active.clone().unwrap()["1"]
            .doc_id
            .clone();
        assert_eq!(before, after);
        assert_eq!(storage.get().unwrap()["1"].description(), "b");
    }

    #[test]
    fn encrypted_payloads_round_trip() {
        let storage = open_local(true);
        storage.set(&map(vec![task(1, "secret")])).unwrap();

        let raw = storage
            .execute(format!("SELECT * FROM {}", storage.collection), json!({}))
            .unwrap();
        let doc: Doc = raw.iter().next().unwrap().deserialize_value().unwrap();
        assert!(doc.nonce.is_some());
        assert!(!doc.payload.contains("secret"));

        assert_eq!(storage.get().unwrap()["1"].description(), "secret");
    }

    #[test]
    fn duplicate_ids_are_renumbered_on_read() {
        let storage = open_local(false);
        // Simulate two devices that each created item 1 while offline.
        let older = storage
            .make_doc("doc-a".into(), 1, false, &task(1, "first"), 100)
            .unwrap();
        let newer = storage
            .make_doc("doc-b".into(), 1, false, &task(1, "second"), 200)
            .unwrap();
        storage.apply(vec![older, newer], Vec::new()).unwrap();

        let items = storage.get().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items["1"].description(), "first");
        assert_eq!(items["2"].description(), "second");
        assert_eq!(items["2"].id(), 2);

        // The renumbering was persisted.
        let again = storage.get().unwrap();
        assert_eq!(again["2"].description(), "second");
        assert_eq!(storage.tombstone_count().unwrap(), 0);
    }

    #[test]
    fn set_without_prior_read_updates_existing_documents() {
        let storage = open_local(false);
        storage.set(&map(vec![task(1, "a")])).unwrap();
        // Fresh snapshot state, as `--migrate` would see it.
        *lock(&storage.snapshots) = Snapshots::default();
        storage.set(&map(vec![task(1, "b")])).unwrap();

        let raw = storage
            .execute(
                format!("SELECT * FROM {} WHERE deleted = false", storage.collection),
                json!({}),
            )
            .unwrap();
        assert_eq!(raw.item_count(), 1);
        assert_eq!(storage.get().unwrap()["1"].description(), "b");
    }

    #[test]
    fn watch_reports_changes() {
        let storage = open_local(false);
        let (tx, rx) = mpsc::channel();
        let _watch = storage
            .watch(Box::new(move |notice| {
                let _ = tx.send(notice);
            }))
            .unwrap()
            .expect("ditto supports watch");

        storage.set_archive(&map(vec![task(1, "a")])).unwrap();
        let notice = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("change notice");
        assert_eq!(notice, ChangeNotice::Archive);
    }

    #[test]
    fn rejects_bad_collection_names() {
        assert!(validate_collection_name("taskbook_items").is_ok());
        assert!(validate_collection_name("_x1").is_ok());
        assert!(validate_collection_name("").is_err());
        assert!(validate_collection_name("1abc").is_err());
        assert!(validate_collection_name("items; DROP").is_err());
    }

    #[test]
    fn rejects_non_numeric_keys() {
        let storage = open_local(false);
        let mut data = HashMap::new();
        data.insert("abc".to_string(), task(1, "x"));
        assert!(storage.set(&data).is_err());
    }
}
