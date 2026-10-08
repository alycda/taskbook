//! A small, safe wrapper over the raw `dittoffi` bindings in [`super::ffi`]:
//! just what the storage backend needs, with ownership and error handling
//! made explicit so the backend itself stays free of `unsafe`.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::path::Path;
use std::sync::Arc;

use base64::Engine;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::ffi;

/// SDK release the bindings were transcribed from (set by `build.rs`).
pub const SDK_VERSION: &str = env!("TB_DITTO_SDK_VERSION");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkError {
    pub code: i32,
    pub message: String,
}

impl std::fmt::Display for SdkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (code {})", self.message, self.code)
    }
}

impl std::error::Error for SdkError {}

pub type Result<T> = std::result::Result<T, SdkError>;

/// Consume a `dittoffi_error_t *` from a result struct.
///
/// # Safety
/// `error` must be null or a pointer returned by a `*_throws` function that
/// has not been freed.
unsafe fn take_error(error: *mut ffi::Error) -> Result<()> {
    if error.is_null() {
        return Ok(());
    }
    let code = ffi::dittoffi_error_code(error);
    let description = ffi::dittoffi_error_description(error);
    let message = if description.is_null() {
        "unknown Ditto error".to_string()
    } else {
        let s = CStr::from_ptr(description).to_string_lossy().into_owned();
        ffi::ditto_c_string_free(description);
        s
    };
    ffi::dittoffi_error_free(error);
    Err(SdkError { code, message })
}

/// Take ownership of an owned byte slice from Ditto and free it.
///
/// # Safety
/// `slice` must have been returned by a `dittoffi` function and not freed.
unsafe fn take_bytes(slice: ffi::SliceBoxed) -> Vec<u8> {
    let bytes = if slice.ptr.is_null() || slice.len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(slice.ptr, slice.len).to_vec()
    };
    ffi::ditto_c_bytes_free(slice);
    bytes
}

fn c_string(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| SdkError {
        code: -1,
        message: format!("string contains an interior NUL: {s:?}"),
    })
}

fn encode_cbor(value: &Value) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    ciborium::into_writer(value, &mut buf).map_err(|e| SdkError {
        code: -1,
        message: format!("CBOR encoding failed: {e}"),
    })?;
    Ok(buf)
}

fn decode_cbor<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    ciborium::from_reader(bytes).map_err(|e| SdkError {
        code: -1,
        message: format!("CBOR decoding failed: {e}"),
    })
}

/// How to connect, mirroring the SDK's `DittoConfigConnect`.
#[derive(Debug, Clone)]
pub enum Connect {
    /// Ditto Cloud / Big Peer at `url`; needs a [`Ditto::set_login`].
    Server { url: String },
    /// Small peers only (LAN / P2P mesh), optionally with a shared private
    /// key (DER bytes).
    SmallPeersOnly { private_key: Option<Vec<u8>> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warning,
    Info,
    Debug,
    Verbose,
}

/// Configure Ditto's process-wide logger. Off by default.
pub fn set_logging(enabled: bool, level: LogLevel) {
    let level = match level {
        LogLevel::Error => ffi::CLogLevel::Error,
        LogLevel::Warning => ffi::CLogLevel::Warning,
        LogLevel::Info => ffi::CLogLevel::Info,
        LogLevel::Debug => ffi::CLogLevel::Debug,
        LogLevel::Verbose => ffi::CLogLevel::Verbose,
    };
    // SAFETY: plain setters on global logger state.
    unsafe {
        ffi::ditto_logger_minimum_log_level(level);
        ffi::ditto_logger_enabled(enabled);
    }
}

/// An open Ditto instance. Dropping it stops sync and frees the native
/// object; observers and subscriptions must be dropped before it.
pub struct Ditto {
    ptr: *mut ffi::CDitto,
    login: Option<Arc<LoginContext>>,
}

// SAFETY: the Ditto SDK documents the instance as usable from any thread
// (its own Rust wrapper is `Send + Sync`); all mutation goes through the
// native library's internal locking.
unsafe impl Send for Ditto {}
unsafe impl Sync for Ditto {}

impl Ditto {
    /// Open (or create) the database at `persistence_dir` for `database_id`.
    pub fn open(database_id: &str, connect: &Connect, persistence_dir: &Path) -> Result<Self> {
        let engine = base64::engine::general_purpose::STANDARD;
        let connect_value = match connect {
            Connect::Server { url } => json!({ "type": "server", "url": url }),
            Connect::SmallPeersOnly { private_key: None } => {
                json!({ "type": "small_peers_only" })
            }
            Connect::SmallPeersOnly {
                private_key: Some(key),
            } => json!({ "type": "small_peers_only", "private_key": engine.encode(key) }),
        };
        let dir = persistence_dir.to_string_lossy().into_owned();
        let config = json!({
            "database_id": database_id,
            "connect": connect_value,
            "persistence_directory": dir,
            "experimental": {},
        });
        let config_cbor = encode_cbor(&config)?;
        let root = c_string(&dir)?;

        // SAFETY: the CBOR buffer and C string outlive the call; the
        // returned pointer is owned by us until `ditto_free`.
        let ptr = unsafe {
            let result = ffi::dittoffi_ditto_open_throws(
                ffi::SliceRef::from_bytes(&config_cbor),
                ffi::TransportConfigMode::PlatformIndependent,
                root.as_ptr(),
            );
            take_error(result.error)?;
            result.success
        };
        if ptr.is_null() {
            return Err(SdkError {
                code: -1,
                message: "dittoffi_ditto_open_throws returned no instance".into(),
            });
        }
        Ok(Self { ptr, login: None })
    }

    /// Enable mutating DQL (INSERT/UPDATE) by giving up sync with v3 peers.
    pub fn disable_sync_with_v3(&self) -> Result<()> {
        // SAFETY: `ptr` is a live instance.
        let rc = unsafe { ffi::ditto_disable_sync_with_v3(self.ptr) };
        if rc == 0 {
            Ok(())
        } else {
            Err(SdkError {
                code: rc,
                message: "ditto_disable_sync_with_v3 failed".into(),
            })
        }
    }

    /// Activate an offline (small peers only) instance with a license token.
    pub fn set_offline_only_license_token(&self, token: &str) -> Result<()> {
        let token = c_string(token)?;
        // SAFETY: `ptr` is live; the C string outlives the call.
        unsafe {
            let result =
                ffi::dittoffi_ditto_set_offline_only_license_token_throws(self.ptr, token.as_ptr());
            take_error(result.error)
        }
    }

    /// Register a login provider for `Connect::Server`: whenever Ditto
    /// reports that authentication is required or about to expire, log in
    /// with `token` through the named auth `provider`.
    pub fn set_login(&mut self, token: &str, provider: &str) -> Result<()> {
        let ctx = Arc::new(LoginContext {
            ditto: self.ptr,
            token: c_string(token)?,
            provider: c_string(provider)?,
        });
        // SAFETY: the Arc is leaked into `ctx` with one strong count that
        // `release` gives back; `retain`/`release` adjust the count.
        unsafe {
            let raw = Arc::into_raw(Arc::clone(&ctx)) as *mut c_void;
            let provider_handle = ffi::ditto_auth_client_make_login_provider(
                raw,
                login_retain,
                login_release,
                login_expiring,
            );
            ffi::ditto_auth_set_login_provider(self.ptr, provider_handle);
        }
        self.login = Some(ctx);
        Ok(())
    }

    pub fn start_sync(&self) -> Result<()> {
        // SAFETY: `ptr` is live.
        unsafe { take_error(ffi::dittoffi_ditto_try_start_sync(self.ptr).error) }
    }

    pub fn stop_sync(&self) {
        // SAFETY: `ptr` is live.
        unsafe { ffi::dittoffi_ditto_stop_sync(self.ptr) }
    }

    /// Execute a DQL statement with optional named arguments and return the
    /// result rows as JSON values.
    pub fn execute(&self, statement: &str, args: Option<&Value>) -> Result<Vec<Value>> {
        let statement = c_string(statement)?;
        let args_cbor = match args {
            Some(args) => Some(encode_cbor(args)?),
            None => None,
        };
        let args_slice = args_cbor
            .as_deref()
            .map(ffi::SliceRef::from_bytes)
            .unwrap_or(ffi::SliceRef::NONE);

        // SAFETY: inputs outlive the call; the result and each item are freed
        // exactly once below.
        unsafe {
            let result = ffi::dittoffi_try_exec_statement(self.ptr, statement.as_ptr(), args_slice);
            take_error(result.error)?;
            let result = QueryResultHandle(result.success);
            if result.0.is_null() {
                return Ok(Vec::new());
            }
            let count = ffi::dittoffi_query_result_item_count(result.0);
            let mut rows = Vec::with_capacity(count);
            for idx in 0..count {
                let item = ffi::dittoffi_query_result_item_at(result.0, idx);
                if item.is_null() {
                    continue;
                }
                let bytes = take_bytes(ffi::dittoffi_query_result_item_cbor(item));
                ffi::dittoffi_query_result_item_free(item);
                rows.push(decode_cbor::<Value>(&bytes)?);
            }
            Ok(rows)
        }
    }

    /// Subscribe to sync for the documents matching `query`.
    pub fn register_subscription(&self, query: &str) -> Result<Subscription> {
        let query = c_string(query)?;
        // SAFETY: `ptr` is live; the string outlives the call.
        unsafe {
            let result = ffi::dittoffi_sync_register_subscription_throws(
                self.ptr,
                query.as_ptr(),
                ffi::SliceRef::NONE,
            );
            take_error(result.error)?;
            Ok(Subscription(result.success))
        }
    }

    /// Observe `query`; `on_change` runs on a Ditto thread whenever the
    /// result set changes (including once right after registration).
    pub fn register_observer<F>(
        &self,
        query: &str,
        args: Option<&Value>,
        on_change: F,
    ) -> Result<Observer>
    where
        F: FnMut() + Send + 'static,
    {
        let query = c_string(query)?;
        let args_cbor = match args {
            Some(args) => Some(encode_cbor(args)?),
            None => None,
        };
        let args_slice = args_cbor
            .as_deref()
            .map(ffi::SliceRef::from_bytes)
            .unwrap_or(ffi::SliceRef::NONE);

        let env: Box<ObserverEnv> = Box::new(Box::new(on_change));
        let callback = ffi::StoreObserverCallback {
            env_ptr: Box::into_raw(env) as *mut c_void,
            call: observer_call,
            free: observer_free,
        };
        // SAFETY: `env_ptr` is owned by Ditto from here on and released by
        // `observer_free`, which Ditto calls exactly once.
        unsafe {
            let result = ffi::dittoffi_store_register_observer_throws(
                self.ptr,
                query.as_ptr(),
                args_slice,
                callback,
            );
            take_error(result.error)?;
            Ok(Observer(result.success))
        }
    }

    /// Number of remote peers currently known to the mesh.
    pub fn remote_peer_count(&self) -> Result<usize> {
        // SAFETY: `ptr` is live; the slice is consumed by `take_bytes`.
        let bytes = unsafe { take_bytes(ffi::dittoffi_presence_graph(self.ptr)) };
        if bytes.is_empty() {
            return Ok(0);
        }
        // Unlike query results, the presence graph is delivered as JSON
        // (Ditto's own SDK parses it with serde_json).
        let graph: Value = serde_json::from_slice(&bytes).map_err(|e| SdkError {
            code: -1,
            message: format!("presence graph is not valid JSON: {e}"),
        })?;
        Ok(graph
            .get("remotePeers")
            .and_then(Value::as_array)
            .map_or(0, Vec::len))
    }
}

impl Drop for Ditto {
    fn drop(&mut self) {
        // SAFETY: `ptr` is live and freed exactly once here. Detaching the
        // login provider first keeps its context from outliving the instance
        // it points at.
        unsafe {
            if self.login.take().is_some() {
                ffi::ditto_auth_set_login_provider(self.ptr, std::ptr::null_mut());
            }
            ffi::ditto_free(self.ptr);
        }
    }
}

/// Frees a query result on drop.
struct QueryResultHandle(*mut ffi::QueryResult);

impl Drop for QueryResultHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: owned result pointer, freed once.
            unsafe { ffi::dittoffi_query_result_free(self.0) }
        }
    }
}

/// A registered sync subscription; cancelled and freed on drop.
pub struct Subscription(*mut ffi::SyncSubscription);

// SAFETY: opaque handle to a thread-safe native object.
unsafe impl Send for Subscription {}
unsafe impl Sync for Subscription {}

impl Drop for Subscription {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: owned handle, cancelled then freed once.
            unsafe {
                ffi::dittoffi_sync_subscription_cancel(self.0);
                ffi::dittoffi_sync_subscription_free(self.0);
            }
        }
    }
}

/// A registered store observer; cancelled and freed on drop.
pub struct Observer(*mut ffi::StoreObserver);

// SAFETY: opaque handle to a thread-safe native object.
unsafe impl Send for Observer {}
unsafe impl Sync for Observer {}

impl Drop for Observer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: owned handle, cancelled then freed once.
            unsafe {
                ffi::dittoffi_store_observer_cancel(self.0);
                ffi::dittoffi_store_observer_free(self.0);
            }
        }
    }
}

type ObserverEnv = Box<dyn FnMut() + Send>;

unsafe extern "C" fn observer_call(
    env: *mut c_void,
    result: *mut ffi::QueryResult,
    signal_next: ffi::ArcDynFn0,
) {
    // The result set itself is not needed: the backend re-reads on notice.
    if !result.is_null() {
        ffi::dittoffi_query_result_free(result);
    }
    if !env.is_null() {
        let on_change = &mut *(env as *mut ObserverEnv);
        // A panic must not unwind across the FFI boundary.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(on_change));
    }
    // We own one strong reference to the signal: call it (ready for the next
    // callback) and release it.
    if let Some(call) = signal_next.call {
        call(signal_next.env_ptr);
    }
    if let Some(release) = signal_next.release {
        release(signal_next.env_ptr);
    }
}

unsafe extern "C" fn observer_free(env: *mut c_void) {
    if !env.is_null() {
        drop(Box::from_raw(env as *mut ObserverEnv));
    }
}

struct LoginContext {
    ditto: *mut ffi::CDitto,
    token: CString,
    provider: CString,
}

// SAFETY: the raw instance pointer is only used while the owning `Ditto`
// is alive (it detaches the provider before freeing), and the login call is
// thread-safe on the native side.
unsafe impl Send for LoginContext {}
unsafe impl Sync for LoginContext {}

unsafe extern "C" fn login_retain(ctx: *mut c_void) {
    if !ctx.is_null() {
        Arc::increment_strong_count(ctx as *const LoginContext);
    }
}

unsafe extern "C" fn login_release(ctx: *mut c_void) {
    if !ctx.is_null() {
        Arc::decrement_strong_count(ctx as *const LoginContext);
    }
}

unsafe extern "C" fn login_expiring(ctx: *mut c_void, _seconds_remaining: u32) {
    if ctx.is_null() {
        return;
    }
    Arc::increment_strong_count(ctx as *const LoginContext);
    let ctx = Arc::from_raw(ctx as *const LoginContext);
    // Log in off the callback thread so the native side is never re-entered
    // from within its own notification.
    std::thread::spawn(move || {
        // SAFETY: see `LoginContext`.
        let rc = unsafe {
            ffi::ditto_auth_client_login_with_token(
                ctx.ditto,
                ctx.token.as_ptr() as *const c_char,
                ctx.provider.as_ptr() as *const c_char,
            )
        };
        if rc != 0 {
            eprintln!("ditto: login failed (code {rc})");
        }
    });
}
