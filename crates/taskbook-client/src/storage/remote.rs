use std::collections::HashMap;

use super::{Items, StorageBackend};
use crate::api_client::{ApiClient, EncryptedItemData};
use crate::credentials::Credentials;
use crate::error::{Result, TaskbookError};
use base64::Engine;
use taskbook_common::encryption::{decrypt_item, encrypt_item, EncryptedItem};

/// Remote storage backend that communicates with a taskbook server.
/// All data is encrypted client-side before being sent to the server.
pub struct RemoteStorage {
    client: ApiClient,
    encryption_key: [u8; 32],
}

impl RemoteStorage {
    pub fn new(server_url: &str) -> Result<Self> {
        let creds = Credentials::load()?.ok_or_else(|| {
            TaskbookError::Auth("not logged in — run `tb register` or `tb login` first".to_string())
        })?;

        let encryption_key = creds.encryption_key_bytes()?;
        let client = ApiClient::new(server_url, Some(&creds.token));

        Ok(Self {
            client,
            encryption_key,
        })
    }

    fn decrypt_items(&self, encrypted: &HashMap<String, EncryptedItemData>) -> Result<Items> {
        let engine = base64::engine::general_purpose::STANDARD;
        let mut result = Items::new();

        for (key, item_data) in encrypted {
            // Only canonical keys: "01" or "+1" would parse to an existing ID.
            let id = key
                .parse::<u64>()
                .ok()
                .filter(|id| id.to_string() == *key)
                .ok_or_else(|| TaskbookError::General(format!("invalid item key: {key}")))?;
            let data = engine
                .decode(&item_data.data)
                .map_err(|e| TaskbookError::General(format!("invalid base64 data: {e}")))?;
            let nonce = engine
                .decode(&item_data.nonce)
                .map_err(|e| TaskbookError::General(format!("invalid base64 nonce: {e}")))?;

            let encrypted_item = EncryptedItem { data, nonce };
            let item = decrypt_item(&self.encryption_key, &encrypted_item)
                .map_err(|e| TaskbookError::General(format!("decryption failed: {e}")))?;

            result.insert(id, item);
        }

        Ok(result)
    }

    fn encrypt_items(&self, items: &Items) -> Result<HashMap<String, EncryptedItemData>> {
        let engine = base64::engine::general_purpose::STANDARD;
        let mut result = HashMap::new();

        for (id, item) in items {
            let encrypted = encrypt_item(&self.encryption_key, item)
                .map_err(|e| TaskbookError::General(format!("encryption failed: {e}")))?;

            result.insert(
                id.to_string(),
                EncryptedItemData {
                    data: engine.encode(&encrypted.data),
                    nonce: engine.encode(&encrypted.nonce),
                },
            );
        }

        Ok(result)
    }
}

impl StorageBackend for RemoteStorage {
    fn get(&self) -> Result<Items> {
        let encrypted = self.client.get_items()?;
        self.decrypt_items(&encrypted)
    }

    fn get_archive(&self) -> Result<Items> {
        let encrypted = self.client.get_archive()?;
        self.decrypt_items(&encrypted)
    }

    fn set(&self, data: &Items) -> Result<()> {
        let encrypted = self.encrypt_items(data)?;
        self.client.put_items(&encrypted)
    }

    fn set_archive(&self, data: &Items) -> Result<()> {
        let encrypted = self.encrypt_items(data)?;
        self.client.put_archive(&encrypted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taskbook_common::{StorageItem, Task};

    fn storage() -> RemoteStorage {
        RemoteStorage {
            client: ApiClient::new("http://localhost", None),
            encryption_key: [7; 32],
        }
    }

    fn encrypted(storage: &RemoteStorage, keys: &[&str]) -> HashMap<String, EncryptedItemData> {
        let mut items = Items::new();
        for (n, _) in keys.iter().enumerate() {
            let id = n as u64 + 1;
            items.insert(
                id,
                StorageItem::Task(Task::new(id, format!("task {id}"), vec![], 1)),
            );
        }
        let by_id = storage.encrypt_items(&items).unwrap();
        keys.iter()
            .enumerate()
            .map(|(n, key)| (key.to_string(), by_id[&(n + 1).to_string()].clone()))
            .collect()
    }

    #[test]
    fn decrypts_canonical_keys() {
        let storage = storage();
        let items = storage
            .decrypt_items(&encrypted(&storage, &["1", "2", "10"]))
            .unwrap();
        assert_eq!(items.keys().copied().collect::<Vec<_>>(), [1, 2, 10]);
    }

    #[test]
    fn rejects_keys_that_alias_another_id() {
        // "01" and "+1" parse to 1; accepting them would let one item
        // silently replace another, and the next set() would drop it.
        let storage = storage();
        for alias in ["01", "+1"] {
            let result = storage.decrypt_items(&encrypted(&storage, &["1", alias]));
            assert!(result.is_err(), "key {alias:?} was accepted");
        }
    }
}
