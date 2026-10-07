use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Credentials for server authentication and encryption.
/// Stored at ~/.taskbook/credentials.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub server_url: String,
    pub token: String,
    pub encryption_key: String, // base64-encoded 32-byte key
}

impl Credentials {
    fn credentials_path() -> Result<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| {
            crate::error::TaskbookError::General("could not find home directory".to_string())
        })?;
        Ok(home.join(".taskbook").join("credentials.json"))
    }

    /// Load credentials from disk. Returns None if the file doesn't exist.
    pub fn load() -> Result<Option<Self>> {
        let path = Self::credentials_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?;
        let creds: Credentials = serde_json::from_str(&content)?;
        Ok(Some(creds))
    }

    /// Save credentials to disk with restrictive permissions (0600).
    pub fn save(&self) -> Result<()> {
        let path = Self::credentials_path()?;
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&path, json)?;

        // Set file permissions to owner-only read/write (0600)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Delete the credentials file.
    pub fn delete() -> Result<()> {
        let path = Self::credentials_path()?;
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Decode the encryption key from base64.
    pub fn encryption_key_bytes(&self) -> Result<[u8; 32]> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&self.encryption_key)
            .map_err(|e| {
                crate::error::TaskbookError::General(format!("invalid encryption key: {e}"))
            })?;
        if bytes.len() != 32 {
            return Err(crate::error::TaskbookError::General(format!(
                "encryption key must be 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(key)
    }
}

/// Secrets for the Ditto backend. Kept apart from the config file so a
/// declaratively managed `taskbook.json` never carries key material.
/// Stored at `~/.taskbook/ditto-credentials.json` (mode 0600).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DittoCredentials {
    /// Auth token for `connect = "server"` (a playground token, or a token
    /// your auth webhook accepts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// base64-encoded 32-byte AES-256-GCM key used when `ditto.encrypt` is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_key: Option<String>,

    /// Path to a DER-encoded private key for a shared-key `peers` mesh.
    /// When unset, `peers` mode runs as an unauthenticated development mesh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_path: Option<String>,

    /// Offline license token from the Ditto portal; required to start sync
    /// in `peers` mode (not needed in `server` mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_token: Option<String>,
}

impl DittoCredentials {
    pub fn path() -> Result<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| {
            crate::error::TaskbookError::General("could not find home directory".to_string())
        })?;
        Ok(home.join(".taskbook").join("ditto-credentials.json"))
    }

    /// Load from disk. Returns None if the file doesn't exist.
    pub fn load() -> Result<Option<Self>> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?;
        Ok(Some(serde_json::from_str(&content)?))
    }

    /// Save to disk with restrictive permissions (0600).
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(&path, serde_json::to_string_pretty(self)?)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Decode the encryption key, if one is set.
    pub fn encryption_key_bytes(&self) -> Result<Option<[u8; 32]>> {
        let Some(encoded) = &self.encryption_key else {
            return Ok(None);
        };
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| {
                crate::error::TaskbookError::General(format!("invalid ditto encryption key: {e}"))
            })?;
        if bytes.len() != 32 {
            return Err(crate::error::TaskbookError::General(format!(
                "ditto encryption key must be 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(Some(key))
    }
}
