use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::tui::ViewMode;

/// RGB color values
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// Theme color palette
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeColors {
    /// Muted/secondary text color
    pub muted: Rgb,
    /// Success indicators (checkmarks, completed counts)
    pub success: Rgb,
    /// Warning indicators (in-progress, medium priority)
    pub warning: Rgb,
    /// Error/high priority indicators
    pub error: Rgb,
    /// Info indicators (notes, in-progress counts)
    pub info: Rgb,
    /// Pending task indicators
    pub pending: Rgb,
    /// Starred item indicator
    pub starred: Rgb,
}

impl Default for ThemeColors {
    fn default() -> Self {
        // Default theme - readable on most terminals
        Self {
            muted: Rgb::new(140, 140, 140),
            success: Rgb::new(134, 239, 172),
            warning: Rgb::new(253, 224, 71),
            error: Rgb::new(252, 129, 129),
            info: Rgb::new(147, 197, 253),
            pending: Rgb::new(216, 180, 254),
            starred: Rgb::new(253, 224, 71),
        }
    }
}

impl ThemeColors {
    /// Catppuccin Macchiato theme
    pub fn catppuccin_macchiato() -> Self {
        Self {
            muted: Rgb::new(165, 173, 203),   // Subtext0
            success: Rgb::new(166, 218, 149), // Green
            warning: Rgb::new(238, 212, 159), // Yellow
            error: Rgb::new(237, 135, 150),   // Red
            info: Rgb::new(138, 173, 244),    // Blue
            pending: Rgb::new(198, 160, 246), // Mauve
            starred: Rgb::new(238, 212, 159), // Yellow
        }
    }

    /// Catppuccin Mocha theme
    pub fn catppuccin_mocha() -> Self {
        Self {
            muted: Rgb::new(166, 173, 200),   // Subtext0
            success: Rgb::new(166, 227, 161), // Green
            warning: Rgb::new(249, 226, 175), // Yellow
            error: Rgb::new(243, 139, 168),   // Red
            info: Rgb::new(137, 180, 250),    // Blue
            pending: Rgb::new(203, 166, 247), // Mauve
            starred: Rgb::new(249, 226, 175), // Yellow
        }
    }

    /// Catppuccin Frappe theme
    pub fn catppuccin_frappe() -> Self {
        Self {
            muted: Rgb::new(165, 173, 206),   // Subtext0
            success: Rgb::new(166, 209, 137), // Green
            warning: Rgb::new(229, 200, 144), // Yellow
            error: Rgb::new(231, 130, 132),   // Red
            info: Rgb::new(140, 170, 238),    // Blue
            pending: Rgb::new(202, 158, 230), // Mauve
            starred: Rgb::new(229, 200, 144), // Yellow
        }
    }

    /// Catppuccin Latte theme (light theme)
    pub fn catppuccin_latte() -> Self {
        Self {
            muted: Rgb::new(108, 111, 133),  // Subtext0
            success: Rgb::new(64, 160, 43),  // Green
            warning: Rgb::new(223, 142, 29), // Yellow
            error: Rgb::new(210, 15, 57),    // Red
            info: Rgb::new(30, 102, 245),    // Blue
            pending: Rgb::new(136, 57, 239), // Mauve
            starred: Rgb::new(223, 142, 29), // Yellow
        }
    }

    /// High contrast theme for accessibility
    pub fn high_contrast() -> Self {
        Self {
            muted: Rgb::new(200, 200, 200),
            success: Rgb::new(0, 255, 0),
            warning: Rgb::new(255, 255, 0),
            error: Rgb::new(255, 0, 0),
            info: Rgb::new(0, 255, 255),
            pending: Rgb::new(255, 0, 255),
            starred: Rgb::new(255, 255, 0),
        }
    }

    /// Get theme by name
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().replace(['-', '_', ' '], "") {
            s if s == "default" => Some(Self::default()),
            s if s == "catppuccinmacchiato" => Some(Self::catppuccin_macchiato()),
            s if s == "catppuccinmocha" => Some(Self::catppuccin_mocha()),
            s if s == "catppuccinfrappe" => Some(Self::catppuccin_frappe()),
            s if s == "catppuccinlatte" => Some(Self::catppuccin_latte()),
            s if s == "highcontrast" => Some(Self::high_contrast()),
            _ => None,
        }
    }
}

/// Theme configuration - either a preset name or custom colors
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ThemeConfig {
    /// Preset theme name
    Preset(String),
    /// Custom color configuration
    Custom(ThemeColors),
}

impl Default for ThemeConfig {
    fn default() -> Self {
        ThemeConfig::Preset("default".to_string())
    }
}

impl ThemeConfig {
    /// Resolve to actual theme colors
    pub fn resolve(&self) -> ThemeColors {
        match self {
            ThemeConfig::Preset(name) => ThemeColors::from_name(name).unwrap_or_default(),
            ThemeConfig::Custom(colors) => colors.clone(),
        }
    }
}

/// Sort method for items within boards
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortMethod {
    /// Sort by item ID (creation order)
    #[default]
    Id,
    /// Sort by priority (high first), then ID
    Priority,
    /// Sort by status (pending, in-progress, done), then ID
    Status,
}

impl SortMethod {
    /// Cycle to the next sort method
    pub fn next(self) -> Self {
        match self {
            SortMethod::Id => SortMethod::Priority,
            SortMethod::Priority => SortMethod::Status,
            SortMethod::Status => SortMethod::Id,
        }
    }

    /// Display name for the sort method
    pub fn display_name(self) -> &'static str {
        match self {
            SortMethod::Id => "ID",
            SortMethod::Priority => "Priority",
            SortMethod::Status => "Status",
        }
    }
}

/// Which storage backend to use when `sync.enabled` is true.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncBackend {
    /// The taskbook HTTP server (`tb-server`) with client-side encryption.
    #[default]
    Server,
    /// A Ditto peer-to-peer mesh (requires a build with `--features ditto`).
    Ditto,
}

impl SyncBackend {
    pub fn display_name(self) -> &'static str {
        match self {
            SyncBackend::Server => "server",
            SyncBackend::Ditto => "ditto",
        }
    }
}

/// Sync configuration for remote server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_server_url")]
    pub server_url: String,

    /// Backend selected when `enabled` is true. Omitted from the file while it
    /// is the default so existing configs round-trip unchanged.
    #[serde(default, skip_serializing_if = "SyncBackend::is_default")]
    pub backend: SyncBackend,
}

impl SyncBackend {
    fn is_default(&self) -> bool {
        *self == SyncBackend::default()
    }
}

fn default_server_url() -> String {
    "http://localhost:8080".to_string()
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server_url: default_server_url(),
            backend: SyncBackend::default(),
        }
    }
}

/// How the Ditto backend connects to other peers.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DittoConnect {
    /// Small peers only: LAN / peer-to-peer mesh, no cloud account. Uses the
    /// shared private key from the Ditto credentials file when one is set.
    #[default]
    Peers,
    /// Connect to a Ditto Cloud app or a self-hosted Big Peer at `url`,
    /// authenticating with the token from the Ditto credentials file.
    Server,
}

/// Settings for the Ditto peer-to-peer backend (`sync.backend = "ditto"`).
///
/// Secrets (auth token, encryption key, private key) are not stored here; they
/// live in `~/.taskbook/ditto-credentials.json`. See docs/ditto.md.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DittoConfig {
    /// Ditto app / database ID. Every device that should share data uses the
    /// same value. For `peers` mode any string works; Ditto Cloud issues a UUID.
    #[serde(default)]
    pub app_id: String,

    #[serde(default)]
    pub connect: DittoConnect,

    /// Auth / sync URL for `connect = "server"` (e.g. the Ditto portal's
    /// "Auth URL"). Ignored in `peers` mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Name of the authentication provider (webhook) configured on the Ditto
    /// portal, used with the token on login in `server` mode.
    #[serde(default = "default_ditto_provider")]
    pub provider: String,

    /// Where Ditto keeps its local database. Defaults to `~/.taskbook/ditto`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistence_dir: Option<String>,

    /// Ditto collection that holds the items.
    #[serde(default = "default_ditto_collection")]
    pub collection: String,

    /// Encrypt item payloads client-side (AES-256-GCM) with the key from the
    /// Ditto credentials file before they enter the Ditto store.
    #[serde(default = "default_true")]
    pub encrypt: bool,

    /// After a one-shot CLI write, wait up to this long for a peer connection
    /// so the change has a chance to sync before the process exits. `0`
    /// disables the wait. The TUI is long-lived and syncs continuously.
    #[serde(default = "default_flush_timeout_ms")]
    pub flush_timeout_ms: u64,
}

fn default_ditto_provider() -> String {
    "development".to_string()
}

fn default_ditto_collection() -> String {
    "taskbook_items".to_string()
}

fn default_flush_timeout_ms() -> u64 {
    1000
}

impl Default for DittoConfig {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            connect: DittoConnect::default(),
            url: None,
            provider: default_ditto_provider(),
            persistence_dir: None,
            collection: default_ditto_collection(),
            encrypt: true,
            flush_timeout_ms: default_flush_timeout_ms(),
        }
    }
}

impl DittoConfig {
    fn is_default(&self) -> bool {
        *self == DittoConfig::default()
    }

    /// Resolved persistence directory (`~` expanded).
    pub fn persistence_path(&self) -> PathBuf {
        match &self.persistence_dir {
            Some(dir) => Config::format_taskbook_dir(dir),
            None => {
                let home = dirs::home_dir().expect("Could not find home directory");
                home.join(".taskbook").join("ditto")
            }
        }
    }
}

/// Configuration settings for taskbook
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default = "default_taskbook_directory")]
    pub taskbook_directory: String,

    #[serde(default = "default_true")]
    pub display_complete_tasks: bool,

    #[serde(default = "default_true")]
    pub display_progress_overview: bool,

    #[serde(default)]
    pub theme: ThemeConfig,

    #[serde(default)]
    pub sync: SyncConfig,

    /// Ditto backend settings; only written to the file once customised.
    #[serde(default, skip_serializing_if = "DittoConfig::is_default")]
    pub ditto: DittoConfig,

    #[serde(default)]
    pub sort_method: SortMethod,

    #[serde(default)]
    pub default_view: ViewMode,
}

fn default_taskbook_directory() -> String {
    "~".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            taskbook_directory: default_taskbook_directory(),
            display_complete_tasks: true,
            display_progress_overview: true,
            theme: ThemeConfig::default(),
            sync: SyncConfig::default(),
            ditto: DittoConfig::default(),
            sort_method: SortMethod::default(),
            default_view: ViewMode::default(),
        }
    }
}

impl Config {
    /// Resolve the config file path.
    ///
    /// Prefers the XDG location `~/.config/taskbook/taskbook.json` (honoring
    /// `$XDG_CONFIG_HOME`). Falls back to the legacy `~/.taskbook.json` when it
    /// exists and the XDG file does not, so existing installs keep working.
    /// New installs use the XDG location.
    pub fn config_file_path() -> PathBuf {
        let home = dirs::home_dir().expect("Could not find home directory");
        let xdg = Self::xdg_config_path(&home, std::env::var_os("XDG_CONFIG_HOME").as_deref());
        let legacy = Self::legacy_config_path(&home);
        Self::select_config_path(xdg, legacy)
    }

    /// XDG config path: `$XDG_CONFIG_HOME/taskbook/taskbook.json`, or
    /// `~/.config/taskbook/taskbook.json` when the env var is unset/empty.
    ///
    /// `~/.config` is used on every platform (not the OS-native config dir) to
    /// match the conventional Linux location users expect.
    fn xdg_config_path(home: &Path, xdg_env: Option<&OsStr>) -> PathBuf {
        let base = match xdg_env.filter(|v| !v.is_empty()) {
            Some(v) => PathBuf::from(v),
            None => home.join(".config"),
        };
        base.join("taskbook").join("taskbook.json")
    }

    /// Legacy pre-XDG config path: `~/.taskbook.json`.
    fn legacy_config_path(home: &Path) -> PathBuf {
        home.join(".taskbook.json")
    }

    /// Choose between the XDG and legacy paths: prefer XDG, fall back to an
    /// existing legacy file, otherwise default to XDG for new installs.
    fn select_config_path(xdg: PathBuf, legacy: PathBuf) -> PathBuf {
        if xdg.exists() {
            xdg
        } else if legacy.exists() {
            legacy
        } else {
            xdg
        }
    }

    /// Ensure the config file (and any parent directory) exists, creating it
    /// with defaults if not.
    fn ensure_config_file() -> Result<()> {
        let config_path = Self::config_file_path();
        if !config_path.exists() {
            if let Some(parent) = config_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let default_config = Config::default();
            let data = serde_json::to_string_pretty(&default_config)?;
            fs::write(&config_path, data)?;
        }
        Ok(())
    }

    /// Format a taskbook directory path, expanding ~ to home directory
    pub(crate) fn format_taskbook_dir(path: &str) -> PathBuf {
        if path.starts_with('~') {
            let home = dirs::home_dir().expect("Could not find home directory");
            let rest = path.trim_start_matches('~').trim_start_matches('/');
            if rest.is_empty() {
                home
            } else {
                home.join(rest)
            }
        } else {
            PathBuf::from(path)
        }
    }

    /// Load configuration from file, merging with defaults
    pub fn load() -> Result<Self> {
        Self::ensure_config_file()?;

        let config_path = Self::config_file_path();
        let content = fs::read_to_string(&config_path)?;
        let mut config: Config = serde_json::from_str(&content)?;

        // Expand ~ in taskbook_directory
        if config.taskbook_directory.starts_with('~') {
            config.taskbook_directory = Self::format_taskbook_dir(&config.taskbook_directory)
                .to_string_lossy()
                .to_string();
        }

        Ok(config)
    }

    /// Get the resolved taskbook directory path
    #[allow(dead_code)]
    pub fn get_taskbook_directory(&self) -> PathBuf {
        Self::format_taskbook_dir(&self.taskbook_directory)
    }

    /// Load configuration, falling back to defaults with a warning on failure.
    pub fn load_or_default() -> Self {
        match Self::load() {
            Ok(config) => config,
            Err(err) => {
                eprintln!("Warning: failed to load config: {err}, using defaults");
                Self::default()
            }
        }
    }

    /// Save the configuration to file
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_file_path();
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        fs::write(&config_path, data)?;
        Ok(())
    }

    /// Enable sync with the given server URL and save
    pub fn enable_sync(&mut self, server_url: &str) -> Result<()> {
        self.sync.enabled = true;
        self.sync.server_url = server_url.to_string();
        self.save()
    }

    /// Disable sync and save
    pub fn disable_sync(&mut self) -> Result<()> {
        self.sync.enabled = false;
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_mode_serde_round_trip() {
        for (variant, expected_json) in [
            (ViewMode::Board, "\"board\""),
            (ViewMode::Timeline, "\"timeline\""),
            (ViewMode::Archive, "\"archive\""),
            (ViewMode::Journal, "\"journal\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_json);
            let deserialized: ViewMode = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, variant);
        }
    }

    #[test]
    fn config_without_default_view_deserializes_as_board() {
        let json = r#"{
            "taskbookDirectory": "~",
            "displayCompleteTasks": true,
            "displayProgressOverview": true,
            "theme": "default",
            "sync": { "enabled": false, "serverUrl": "http://localhost:8080" },
            "sortMethod": "id"
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.default_view, ViewMode::Board);
    }

    #[test]
    fn sync_backend_defaults_to_server_and_is_not_serialized() {
        let json = r#"{ "sync": { "enabled": true, "serverUrl": "http://x" } }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.sync.backend, SyncBackend::Server);
        let out = serde_json::to_string(&config).unwrap();
        assert!(!out.contains("\"backend\""));
        assert!(!out.contains("\"ditto\""));
    }

    #[test]
    fn ditto_config_parses_and_round_trips() {
        let json = r#"{
            "sync": { "enabled": true, "backend": "ditto" },
            "ditto": { "appId": "abc", "connect": "server", "url": "https://x.cloud.ditto.live", "encrypt": false }
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.sync.backend, SyncBackend::Ditto);
        assert_eq!(config.ditto.app_id, "abc");
        assert_eq!(config.ditto.connect, DittoConnect::Server);
        assert_eq!(
            config.ditto.url.as_deref(),
            Some("https://x.cloud.ditto.live")
        );
        assert!(!config.ditto.encrypt);
        assert_eq!(config.ditto.collection, "taskbook_items");
        assert_eq!(config.ditto.flush_timeout_ms, 1000);
        let out = serde_json::to_string(&config).unwrap();
        assert!(out.contains("\"backend\":\"ditto\""));
        assert!(out.contains("\"appId\":\"abc\""));
    }

    #[test]
    fn xdg_path_defaults_to_dot_config() {
        let home = Path::new("/home/alice");
        assert_eq!(
            Config::xdg_config_path(home, None),
            PathBuf::from("/home/alice/.config/taskbook/taskbook.json")
        );
    }

    #[test]
    fn xdg_path_honors_env_var() {
        let home = Path::new("/home/alice");
        assert_eq!(
            Config::xdg_config_path(home, Some(OsStr::new("/custom/xdg"))),
            PathBuf::from("/custom/xdg/taskbook/taskbook.json")
        );
    }

    #[test]
    fn xdg_path_ignores_empty_env_var() {
        let home = Path::new("/home/alice");
        assert_eq!(
            Config::xdg_config_path(home, Some(OsStr::new(""))),
            PathBuf::from("/home/alice/.config/taskbook/taskbook.json")
        );
    }

    #[test]
    fn legacy_path_is_dot_taskbook_json() {
        let home = Path::new("/home/alice");
        assert_eq!(
            Config::legacy_config_path(home),
            PathBuf::from("/home/alice/.taskbook.json")
        );
    }

    #[test]
    fn select_config_path_resolution() {
        let dir = std::env::temp_dir().join(format!("tb_cfg_select_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let xdg = dir.join("xdg.json");
        let legacy = dir.join("legacy.json");
        fs::write(&xdg, "{}").unwrap();
        fs::write(&legacy, "{}").unwrap();
        let missing_xdg = dir.join("missing_xdg.json");
        let missing_legacy = dir.join("missing_legacy.json");

        // XDG present wins, even when legacy also exists.
        assert_eq!(Config::select_config_path(xdg.clone(), legacy.clone()), xdg);
        // XDG absent but legacy present -> legacy (existing installs keep working).
        assert_eq!(
            Config::select_config_path(missing_xdg.clone(), legacy.clone()),
            legacy
        );
        // Neither present -> XDG location for new installs.
        assert_eq!(
            Config::select_config_path(missing_xdg.clone(), missing_legacy),
            missing_xdg
        );

        fs::remove_dir_all(&dir).unwrap();
    }
}
