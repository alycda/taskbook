use std::fs::{self, File};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use uuid::Uuid;

use super::{Items, StorageBackend};
use crate::error::Result;

/// Local file-based storage with atomic writes and file locking
pub struct LocalStorage {
    main_app_dir: PathBuf,
    storage_dir: PathBuf,
    archive_dir: PathBuf,
    temp_dir: PathBuf,
    storage_file: PathBuf,
    archive_file: PathBuf,
}

impl LocalStorage {
    pub fn new(taskbook_dir: &Path) -> Result<Self> {
        let main_app_dir = taskbook_dir.to_path_buf();
        let storage_dir = main_app_dir.join("storage");
        let archive_dir = main_app_dir.join("archive");
        let temp_dir = main_app_dir.join(".temp");
        let storage_file = storage_dir.join("storage.json");
        let archive_file = archive_dir.join("archive.json");

        let storage = Self {
            main_app_dir,
            storage_dir,
            archive_dir,
            temp_dir,
            storage_file,
            archive_file,
        };

        storage.ensure_directories()?;

        Ok(storage)
    }

    fn ensure_directories(&self) -> Result<()> {
        if !self.main_app_dir.exists() {
            fs::create_dir_all(&self.main_app_dir)?;
        }
        if !self.storage_dir.exists() {
            fs::create_dir(&self.storage_dir)?;
        }
        if !self.archive_dir.exists() {
            fs::create_dir(&self.archive_dir)?;
        }
        if !self.temp_dir.exists() {
            fs::create_dir(&self.temp_dir)?;
        }

        self.clean_temp_dir()?;

        Ok(())
    }

    fn clean_temp_dir(&self) -> Result<()> {
        if self.temp_dir.exists() {
            for entry in fs::read_dir(&self.temp_dir)? {
                let entry = entry?;
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }

    fn get_temp_file(&self, target_file: &Path) -> PathBuf {
        let random_string = Uuid::new_v4().to_string()[..8].to_string();
        let filename = target_file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let temp_filename = filename.replace(".json", &format!(".TEMP-{}.json", random_string));
        self.temp_dir.join(temp_filename)
    }

    /// Acquire an exclusive lock on the given file path.
    /// Creates the lock file if it doesn't exist.
    fn lock_file(&self, path: &Path) -> Result<File> {
        let lock_path = path.with_extension("lock");
        let lock_file = File::create(&lock_path)?;
        lock_file.lock_exclusive()?;
        Ok(lock_file)
    }

    fn read_json_file(&self, path: &Path) -> Result<Items> {
        if !path.exists() {
            return Ok(Items::new());
        }
        let content = fs::read_to_string(path)?;
        let data: Items = serde_json::from_str(&content)?;
        Ok(data)
    }

    fn write_json_file(&self, path: &Path, data: &Items) -> Result<()> {
        let json = serde_json::to_string_pretty(data)?;
        let temp_file = self.get_temp_file(path);
        fs::write(&temp_file, json)?;
        fs::rename(&temp_file, path)?;
        Ok(())
    }
}

impl StorageBackend for LocalStorage {
    fn get(&self) -> Result<Items> {
        let _lock = self.lock_file(&self.storage_file)?;
        self.read_json_file(&self.storage_file)
    }

    fn get_archive(&self) -> Result<Items> {
        let _lock = self.lock_file(&self.archive_file)?;
        self.read_json_file(&self.archive_file)
    }

    fn set(&self, data: &Items) -> Result<()> {
        let _lock = self.lock_file(&self.storage_file)?;
        self.write_json_file(&self.storage_file, data)
    }

    fn set_archive(&self, data: &Items) -> Result<()> {
        let _lock = self.lock_file(&self.archive_file)?;
        self.write_json_file(&self.archive_file, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_support::{permuted_ids, write_tasks};
    use taskbook_common::Task;

    /// Writes a storage.json whose keys appear in the given order.
    fn storage_with_keys(ids: &[u64]) -> LocalStorage {
        let dir = std::env::temp_dir().join(format!("tb-local-test-{}", Uuid::new_v4()));
        let storage = LocalStorage::new(&dir).unwrap();
        let tasks: Vec<Task> = ids
            .iter()
            .map(|&id| Task::new(id, format!("task {id}"), vec!["My Board".into()], 1))
            .collect();
        write_tasks(&storage.storage_file, &tasks);
        storage
    }

    fn top_level_keys(json: &str) -> Vec<String> {
        json.lines()
            .filter(|line| line.starts_with("  \"") && line.ends_with('{'))
            .map(|line| line.trim().split('"').nth(1).unwrap().to_string())
            .collect()
    }

    #[test]
    fn writes_keys_in_numeric_order() {
        let storage = storage_with_keys(&permuted_ids());

        storage.set(&storage.get().unwrap()).unwrap();

        let json = fs::read_to_string(&storage.storage_file).unwrap();
        let expected: Vec<String> = (1..=20).map(|id: u64| id.to_string()).collect();
        assert_eq!(top_level_keys(&json), expected);
        fs::remove_dir_all(&storage.main_app_dir).unwrap();
    }

    #[test]
    fn same_data_writes_identical_bytes() {
        let storage = storage_with_keys(&permuted_ids());

        storage.set(&storage.get().unwrap()).unwrap();
        let first = fs::read(&storage.storage_file).unwrap();
        storage.set(&storage.get().unwrap()).unwrap();
        let second = fs::read(&storage.storage_file).unwrap();

        assert_eq!(first, second);
        fs::remove_dir_all(&storage.main_app_dir).unwrap();
    }
}
