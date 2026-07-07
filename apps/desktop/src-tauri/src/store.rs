//! Application-level persistence for the AirDrop *experience* layer.
//!
//! This is deliberately separate from the transfer storage engine (`storage`
//! crate) and the certificate trust anchor (`receiver.peer`), neither of which
//! is modified. It adds product state *around* them:
//!
//! - a trusted-devices registry (friendly name, certificate fingerprint, when
//!   first trusted) — UI metadata layered over the unchanged cert trust, and
//! - a transfer-history log.
//!
//! Both are plain JSON files under the app state dir, guarded by a mutex so
//! concurrent commands serialize their read-modify-write.

use engine::chunker::sha256_hex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const TRUSTED_FILE: &str = "trusted-devices.json";
const HISTORY_FILE: &str = "transfer-history.json";
const SETTINGS_FILE: &str = "background-settings.json";
const ONBOARDING_FILE: &str = "onboarding.json";
const PREFERENCES_FILE: &str = "preferences.json";
const HISTORY_CAP: usize = 500;

/// Seconds since the Unix epoch (0 if the clock is before the epoch).
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Persisted background-mode preferences (Feature 2 / 5). Defaults make Nexo
/// behave like AirDrop: available in the background unless the user opts out.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundSettings {
    /// Keep the receiver + advertisement alive when the window is closed.
    pub background_receiving: bool,
    /// Launch Nexo on system startup (honored by the OS-level autostart layer;
    /// stored here so the UI reflects it).
    pub start_on_login: bool,
}

impl Default for BackgroundSettings {
    fn default() -> Self {
        Self {
            background_receiving: true,
            start_on_login: false,
        }
    }
}

/// Onboarding completion state (Feature 3). Once `completed` is true the
/// welcome flow is never shown again.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingState {
    pub completed: bool,
    pub completed_at: u64,
}

/// User-facing application preferences surfaced in the polished Settings screen
/// (Feature 4). All are product-level metadata; none alter the transfer engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppPreferences {
    pub device_name: String,
    pub theme: String,
    pub download_dir: String,
    pub auto_accept_trusted: bool,
    pub notifications_enabled: bool,
    pub discoverable: bool,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            device_name: String::new(),
            theme: "midnight".to_owned(),
            download_dir: String::new(),
            auto_accept_trusted: false,
            notifications_enabled: true,
            discoverable: true,
        }
    }
}

/// A device the user has explicitly trusted, with UI metadata layered over the
/// unchanged certificate trust. `fingerprint` is derived from the peer's
/// certificate DER, so it changes if the certificate changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrustedDevice {
    pub id: String,
    pub display_name: String,
    pub address: String,
    pub platform: String,
    pub fingerprint: String,
    pub first_trusted: u64,
    pub last_seen: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TrustedDevicesFile {
    devices: Vec<TrustedDevice>,
}

/// One completed/cancelled/failed/interrupted transfer, for the history view.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TransferRecord {
    pub id: String,
    pub filename: String,
    pub size: u64,
    pub direction: String,
    pub peer: String,
    pub timestamp: u64,
    pub status: String,
    pub duration_ms: u64,
    pub checksum_ok: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct HistoryFile {
    transfers: Vec<TransferRecord>,
}

/// Short, human-comparable certificate fingerprint (uppercase hex, grouped).
pub fn certificate_fingerprint(certificate_der: &[u8]) -> String {
    let digest = sha256_hex(certificate_der).to_uppercase();
    digest
        .as_bytes()
        .chunks(4)
        .take(8)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join(":")
}

/// JSON-backed application store. All methods serialize through `guard`.
#[derive(Debug)]
pub struct AppStore {
    state_dir: PathBuf,
    guard: Mutex<()>,
}

impl AppStore {
    pub fn new(state_dir: PathBuf) -> Self {
        Self {
            state_dir,
            guard: Mutex::new(()),
        }
    }

    fn trusted_path(&self) -> PathBuf {
        self.state_dir.join(TRUSTED_FILE)
    }

    fn history_path(&self) -> PathBuf {
        self.state_dir.join(HISTORY_FILE)
    }

    // ---- Trusted devices --------------------------------------------------

    pub fn trusted_devices(&self) -> Vec<TrustedDevice> {
        let _lock = self.guard.lock();
        read_json::<TrustedDevicesFile>(&self.trusted_path()).devices
    }

    /// Adds or updates a trusted device (keyed by `id`). Preserves the original
    /// `first_trusted` and any user-chosen `display_name` on update.
    pub fn trust_device(&self, mut device: TrustedDevice) -> TrustedDevice {
        let _lock = self.guard.lock();
        let mut file = read_json::<TrustedDevicesFile>(&self.trusted_path());
        if let Some(existing) = file.devices.iter_mut().find(|d| d.id == device.id) {
            device.first_trusted = existing.first_trusted;
            if device.display_name.trim().is_empty() {
                device.display_name = existing.display_name.clone();
            }
            *existing = device.clone();
        } else {
            if device.first_trusted == 0 {
                device.first_trusted = unix_now();
            }
            file.devices.push(device.clone());
        }
        write_json(&self.state_dir, &self.trusted_path(), &file);
        device
    }

    pub fn untrust_device(&self, id: &str) -> bool {
        let _lock = self.guard.lock();
        let mut file = read_json::<TrustedDevicesFile>(&self.trusted_path());
        let before = file.devices.len();
        file.devices.retain(|device| device.id != id);
        let removed = file.devices.len() != before;
        if removed {
            write_json(&self.state_dir, &self.trusted_path(), &file);
        }
        removed
    }

    pub fn rename_trusted_device(&self, id: &str, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let _lock = self.guard.lock();
        let mut file = read_json::<TrustedDevicesFile>(&self.trusted_path());
        let Some(device) = file.devices.iter_mut().find(|device| device.id == id) else {
            return false;
        };
        device.display_name = name.to_owned();
        write_json(&self.state_dir, &self.trusted_path(), &file);
        true
    }

    /// Refreshes `last_seen` for any trusted device whose address is currently
    /// visible. Returns the ids updated.
    pub fn touch_last_seen(&self, addresses: &[String], now: u64) {
        if addresses.is_empty() {
            return;
        }
        let _lock = self.guard.lock();
        let mut file = read_json::<TrustedDevicesFile>(&self.trusted_path());
        let mut changed = false;
        for device in file.devices.iter_mut() {
            if addresses.iter().any(|address| address == &device.address) {
                device.last_seen = now;
                changed = true;
            }
        }
        if changed {
            write_json(&self.state_dir, &self.trusted_path(), &file);
        }
    }

    // ---- Transfer history -------------------------------------------------

    pub fn history(&self) -> Vec<TransferRecord> {
        let _lock = self.guard.lock();
        let mut transfers = read_json::<HistoryFile>(&self.history_path()).transfers;
        transfers.sort_by_key(|record| std::cmp::Reverse(record.timestamp));
        transfers
    }

    pub fn record_transfer(&self, record: TransferRecord) {
        let _lock = self.guard.lock();
        let mut file = read_json::<HistoryFile>(&self.history_path());
        file.transfers.push(record);
        if file.transfers.len() > HISTORY_CAP {
            let overflow = file.transfers.len() - HISTORY_CAP;
            file.transfers.drain(0..overflow);
        }
        write_json(&self.state_dir, &self.history_path(), &file);
    }

    pub fn clear_history(&self) {
        let _lock = self.guard.lock();
        write_json(
            &self.state_dir,
            &self.history_path(),
            &HistoryFile::default(),
        );
    }

    // ---- Background settings ----------------------------------------------

    fn settings_path(&self) -> PathBuf {
        self.state_dir.join(SETTINGS_FILE)
    }

    pub fn background_settings(&self) -> BackgroundSettings {
        let _lock = self.guard.lock();
        read_json_or(&self.settings_path(), BackgroundSettings::default())
    }

    pub fn set_background_settings(&self, settings: BackgroundSettings) {
        let _lock = self.guard.lock();
        write_json(&self.state_dir, &self.settings_path(), &settings);
    }

    // ---- Onboarding -------------------------------------------------------

    fn onboarding_path(&self) -> PathBuf {
        self.state_dir.join(ONBOARDING_FILE)
    }

    pub fn onboarding(&self) -> OnboardingState {
        let _lock = self.guard.lock();
        read_json::<OnboardingState>(&self.onboarding_path())
    }

    pub fn complete_onboarding(&self) -> OnboardingState {
        let state = OnboardingState {
            completed: true,
            completed_at: unix_now(),
        };
        let _lock = self.guard.lock();
        write_json(&self.state_dir, &self.onboarding_path(), &state);
        state
    }

    // ---- Preferences ------------------------------------------------------

    fn preferences_path(&self) -> PathBuf {
        self.state_dir.join(PREFERENCES_FILE)
    }

    pub fn preferences(&self) -> AppPreferences {
        let _lock = self.guard.lock();
        read_json_or(&self.preferences_path(), AppPreferences::default())
    }

    pub fn set_preferences(&self, preferences: AppPreferences) {
        let _lock = self.guard.lock();
        write_json(&self.state_dir, &self.preferences_path(), &preferences);
    }

    // ---- Benchmark reports ------------------------------------------------

    /// Directory where benchmark JSON reports are written
    /// (`<state_dir>/reports/`).
    pub fn reports_dir(&self) -> PathBuf {
        self.state_dir.join("reports")
    }

    /// Writes a benchmark report as pretty JSON and returns the file path.
    /// `stamp` is a caller-supplied filename-safe timestamp (the store never
    /// reads the clock beyond `unix_now`).
    pub fn write_report(&self, stamp: &str, json: &str) -> std::io::Result<PathBuf> {
        let _lock = self.guard.lock();
        let dir = self.reports_dir();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("transfer-report-{stamp}.json"));
        std::fs::write(&path, json)?;
        Ok(path)
    }
}

fn read_json_or<T: for<'de> Deserialize<'de>>(path: &Path, fallback: T) -> T {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or(fallback),
        Err(_) => fallback,
    }
}

fn read_json<T: Default + for<'de> Deserialize<'de>>(path: &Path) -> T {
    // A missing or corrupt file is treated as empty: application metadata must
    // never block the app from starting.
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => T::default(),
    }
}

fn write_json<T: Serialize>(state_dir: &Path, path: &Path, value: &T) {
    let _ = std::fs::create_dir_all(state_dir);
    if let Ok(serialized) = serde_json::to_vec_pretty(value) {
        let _ = std::fs::write(path, serialized);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(label: &str) -> AppStore {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "nexo-store-{label}-{}-{unique}",
            std::process::id()
        ));
        AppStore::new(dir)
    }

    fn device(id: &str, name: &str, address: &str) -> TrustedDevice {
        TrustedDevice {
            id: id.to_owned(),
            display_name: name.to_owned(),
            address: address.to_owned(),
            platform: "linux".to_owned(),
            fingerprint: certificate_fingerprint(id.as_bytes()),
            first_trusted: 0,
            last_seen: 0,
        }
    }

    #[test]
    fn fingerprint_is_stable_grouped_hex() {
        let a = certificate_fingerprint(b"cert-a");
        assert_eq!(a, certificate_fingerprint(b"cert-a"));
        assert_ne!(a, certificate_fingerprint(b"cert-b"));
        // 8 groups of 4 hex chars joined by ':'.
        assert_eq!(a.split(':').count(), 8);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() || c == ':'));
    }

    #[test]
    fn trust_then_rename_untrust_roundtrips() {
        let store = temp_store("trust");
        let saved = store.trust_device(device("dev-1", "Laptop", "10.0.0.5:41000"));
        assert!(saved.first_trusted > 0, "first_trusted stamped on add");

        assert_eq!(store.trusted_devices().len(), 1);

        assert!(store.rename_trusted_device("dev-1", "Harsh Laptop"));
        assert_eq!(store.trusted_devices()[0].display_name, "Harsh Laptop");

        // Re-trusting must not reset first_trusted or blank an existing name.
        let again = store.trust_device(device("dev-1", "", "10.0.0.5:41000"));
        assert_eq!(again.first_trusted, saved.first_trusted);
        assert_eq!(again.display_name, "Harsh Laptop");

        assert!(store.untrust_device("dev-1"));
        assert!(!store.untrust_device("dev-1"));
        assert!(store.trusted_devices().is_empty());
    }

    #[test]
    fn touch_last_seen_updates_only_visible_devices() {
        let store = temp_store("seen");
        store.trust_device(device("a", "A", "10.0.0.1:1"));
        store.trust_device(device("b", "B", "10.0.0.2:2"));

        store.touch_last_seen(&["10.0.0.1:1".to_owned()], 12345);

        let devices = store.trusted_devices();
        let a = devices.iter().find(|d| d.id == "a").expect("a");
        let b = devices.iter().find(|d| d.id == "b").expect("b");
        assert_eq!(a.last_seen, 12345);
        assert_eq!(b.last_seen, 0);
    }

    #[test]
    fn history_records_are_returned_newest_first_and_capped() {
        let store = temp_store("history");
        for index in 0..(HISTORY_CAP + 25) {
            store.record_transfer(TransferRecord {
                id: format!("t-{index}"),
                filename: "file.bin".to_owned(),
                size: 1024,
                direction: "send".to_owned(),
                peer: "10.0.0.9:41000".to_owned(),
                timestamp: index as u64,
                status: "completed".to_owned(),
                duration_ms: 10,
                checksum_ok: true,
            });
        }

        let history = store.history();
        assert_eq!(history.len(), HISTORY_CAP, "history is capped");
        assert!(history[0].timestamp > history[1].timestamp, "newest first");

        store.clear_history();
        assert!(store.history().is_empty());
    }

    #[test]
    fn missing_or_corrupt_files_read_as_empty() {
        let store = temp_store("corrupt");
        assert!(store.trusted_devices().is_empty());
        assert!(store.history().is_empty());
        // Write garbage, then ensure it degrades to empty rather than panicking.
        std::fs::create_dir_all(&store.state_dir).expect("dir");
        std::fs::write(store.trusted_path(), b"{ not json").expect("write");
        assert!(store.trusted_devices().is_empty());
    }

    #[test]
    fn background_settings_default_on_and_roundtrip() {
        let store = temp_store("bg-settings");
        // Feature 2: default is background receiving ON (AirDrop-like).
        let defaults = store.background_settings();
        assert!(defaults.background_receiving);
        assert!(!defaults.start_on_login);

        store.set_background_settings(BackgroundSettings {
            background_receiving: false,
            start_on_login: true,
        });
        let loaded = store.background_settings();
        assert!(!loaded.background_receiving);
        assert!(loaded.start_on_login);
    }

    #[test]
    fn onboarding_starts_incomplete_and_persists_completion() {
        let store = temp_store("onboarding");
        assert!(!store.onboarding().completed);

        let done = store.complete_onboarding();
        assert!(done.completed);
        assert!(done.completed_at > 0);
        // Persisted: a fresh store over the same dir stays completed.
        assert!(store.onboarding().completed);
    }

    #[test]
    fn preferences_default_and_roundtrip() {
        let store = temp_store("prefs");
        let defaults = store.preferences();
        assert_eq!(defaults.theme, "midnight");
        assert!(defaults.discoverable);
        assert!(defaults.notifications_enabled);
        assert!(!defaults.auto_accept_trusted);

        let mut prefs = defaults;
        prefs.device_name = "Harsh Laptop".to_owned();
        prefs.auto_accept_trusted = true;
        prefs.discoverable = false;
        store.set_preferences(prefs);

        let loaded = store.preferences();
        assert_eq!(loaded.device_name, "Harsh Laptop");
        assert!(loaded.auto_accept_trusted);
        assert!(!loaded.discoverable);
    }
}
