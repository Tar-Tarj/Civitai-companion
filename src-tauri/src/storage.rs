use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use atomic_write_file::AtomicWriteFile;
use tauri::{AppHandle, Manager};
use tokio::sync::{Mutex, Notify, RwLock, Semaphore};

use crate::{
    credentials,
    error::{AppError, AppResult},
    model::{PersistedData, STORAGE_VERSION},
};

const STATE_FILE_NAME: &str = "state-v1.json";
const MAX_STATE_BYTES: u64 = 8 * 1024 * 1024;

pub struct AppRuntime {
    pub data: RwLock<PersistedData>,
    pub sync_guard: Mutex<()>,
    pub scheduler_wakeup: Notify,
    pub image_fetch_guard: Semaphore,
    credential_configured: AtomicBool,
    close_to_tray: AtomicBool,
    path: PathBuf,
}

impl AppRuntime {
    pub fn load(app: &AppHandle) -> AppResult<Self> {
        let directory = app
            .path()
            .app_local_data_dir()
            .map_err(|_| AppError::StateRead)?;
        fs::create_dir_all(&directory).map_err(|_| AppError::StateWrite)?;
        let path = directory.join(STATE_FILE_NAME);
        let data = load_data(&path)?;
        let close_to_tray = data.settings.close_to_tray;
        let credential_configured = credentials::is_configured().unwrap_or(false);
        Ok(Self {
            data: RwLock::new(data),
            sync_guard: Mutex::new(()),
            scheduler_wakeup: Notify::new(),
            image_fetch_guard: Semaphore::new(4),
            credential_configured: AtomicBool::new(credential_configured),
            close_to_tray: AtomicBool::new(close_to_tray),
            path,
        })
    }

    pub fn persist(&self, data: &PersistedData) -> AppResult<()> {
        save_data(&self.path, data)
    }

    pub fn credential_configured(&self) -> bool {
        self.credential_configured.load(Ordering::Relaxed)
    }

    pub fn set_credential_configured(&self, configured: bool) {
        self.credential_configured
            .store(configured, Ordering::Relaxed);
    }

    pub fn close_to_tray(&self) -> bool {
        self.close_to_tray.load(Ordering::Relaxed)
    }

    pub fn set_close_to_tray(&self, enabled: bool) {
        self.close_to_tray.store(enabled, Ordering::Relaxed);
    }
}

fn load_data(path: &Path) -> AppResult<PersistedData> {
    if !path.exists() {
        return Ok(PersistedData::default());
    }
    let metadata = fs::metadata(path).map_err(|_| AppError::StateRead)?;
    if metadata.len() > MAX_STATE_BYTES {
        quarantine_corrupt_state(path)?;
        return Ok(PersistedData::default());
    }
    let bytes = fs::read(path).map_err(|_| AppError::StateRead)?;
    let (data, removed_creator_pulse) = match deserialize_state(&bytes) {
        Ok(data) => data,
        Err(_) => {
            quarantine_corrupt_state(path)?;
            return Ok(PersistedData::default());
        }
    };
    if data.version != STORAGE_VERSION {
        return Err(AppError::UnsupportedState);
    }
    if data.settings.clone().validate().is_none() {
        quarantine_corrupt_state(path)?;
        return Ok(PersistedData::default());
    }
    if removed_creator_pulse {
        save_data(path, &data)?;
    }
    Ok(data)
}

fn deserialize_state(bytes: &[u8]) -> Result<(PersistedData, bool), serde_json::Error> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
    let removed_creator_pulse = value
        .as_object_mut()
        .is_some_and(|state| state.remove("creatorPulse").is_some());
    serde_json::from_value(value).map(|data| (data, removed_creator_pulse))
}

fn quarantine_corrupt_state(path: &Path) -> AppResult<()> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let backup_name = format!("state-v1.corrupt-{timestamp}-{}.json", std::process::id());
    let backup_path = path.with_file_name(backup_name);
    fs::rename(path, backup_path).map_err(|_| AppError::StateRead)
}

fn save_data(path: &Path, data: &PersistedData) -> AppResult<()> {
    let bytes = serde_json::to_vec(data).map_err(|_| AppError::StateWrite)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(AppError::StateWrite);
    }
    let mut file = AtomicWriteFile::open(path).map_err(|_| AppError::StateWrite)?;
    file.write_all(&bytes).map_err(|_| AppError::StateWrite)?;
    file.commit().map_err(|_| AppError::StateWrite)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_serialization_has_no_auth_container() {
        let json = serde_json::to_string(&PersistedData::default()).unwrap();
        let lowercase = json.to_ascii_lowercase();
        assert!(!lowercase.contains("authorization"));
        assert!(!lowercase.contains("apikey"));
        assert!(!lowercase.contains("api_key"));
        assert!(!lowercase.contains("password"));
    }

    #[test]
    fn corrupt_state_is_quarantined_and_defaults_are_loaded() {
        let directory = std::env::temp_dir().join(format!(
            "civitai-companion-corrupt-state-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(STATE_FILE_NAME);
        fs::write(&path, b"not-json").unwrap();

        let loaded = load_data(&path).unwrap();

        assert_eq!(loaded.version, PersistedData::default().version);
        assert!(!path.exists());
        assert!(fs::read_dir(&directory).unwrap().flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("state-v1.corrupt-")
        }));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn legacy_creator_pulse_cache_is_removed_without_resetting_state() {
        let directory = std::env::temp_dir().join(format!(
            "civitai-companion-pulse-migration-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(STATE_FILE_NAME);
        let mut value = serde_json::to_value(PersistedData::default()).unwrap();
        value["settings"]["pollingMinutes"] = serde_json::json!(7);
        value["creatorPulse"] = serde_json::json!({ "legacy": true });
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

        let loaded = load_data(&path).unwrap();

        assert_eq!(loaded.settings.polling_minutes, 7.0);
        let migrated: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(migrated.get("creatorPulse").is_none());
        fs::remove_dir_all(directory).unwrap();
    }
}
