//! Persistence for trial and license state.
//!
//! The record lives in two places:
//! - the OS credential store (Windows Credential Manager, macOS Keychain),
//!   the platform's protected storage and the primary copy;
//! - an encrypted, tamper-evident mirror in the app data folder, used when
//!   the credential store is unavailable.
//!
//! On load the two are merged so that deleting either one alone neither
//! resets the trial nor loses a license. The mirror's key ships with the app,
//! so it deters casual editing rather than a determined attacker; that is
//! proportional for a $10 app, and license validity is always confirmed with
//! Lemon Squeezy rather than trusted from local state.

use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Key, Nonce,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf};

const RECORD_VERSION: u32 = 1;
const KEYRING_SERVICE: &str = "com.thedinz.gridmode";
pub const KEYRING_ACCOUNT: &str = "license";
/// Used by debug builds running with GRIDMODE_DATA_DIR, so development never
/// touches an installed copy's license.
pub const KEYRING_DEV_ACCOUNT: &str = "license-dev";
const MIRROR_FILE: &str = "license.dat";
const MIRROR_AAD: &[u8] = b"gridmode-license-record-v1";
// Obfuscation key for the mirror file (see the module docs).
const MIRROR_KEY: [u8; 32] = [
    0x5b, 0x1e, 0xc4, 0x93, 0x27, 0x6a, 0xf0, 0x0d, 0x88, 0x3c, 0x51, 0xe2, 0x9f, 0x14, 0xb7, 0x6d,
    0x02, 0xa9, 0x4e, 0xd3, 0x71, 0xc8, 0x35, 0x9a, 0xef, 0x60, 0x1b, 0x87, 0x4c, 0xf5, 0x2a, 0xd6,
];

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrialState {
    Active,
    Expired,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrialRecord {
    /// Unix seconds.
    pub started_at: i64,
    pub expires_at: i64,
    pub state: TrialState,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredLicense {
    pub key: String,
    pub instance_id: String,
    pub instance_name: String,
    /// Lemon Squeezy's key status at the last successful check.
    pub status: String,
    pub store_id: u64,
    pub product_id: u64,
    pub variant_id: u64,
    #[serde(default)]
    pub product_name: Option<String>,
    #[serde(default)]
    pub variant_name: Option<String>,
    pub activated_at: i64,
    /// Last time Lemon Squeezy confirmed this activation.
    pub last_validated_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LicenseRecord {
    pub version: u32,
    /// Random per-installation identifier; no hardware information.
    pub install_id: String,
    /// Latest time this installation has observed, so turning the clock back
    /// cannot extend a trial.
    pub last_seen_at: i64,
    /// Trials keyed by major version, so a future 2.x gets its own trial.
    #[serde(default)]
    pub trials: BTreeMap<u32, TrialRecord>,
    #[serde(default)]
    pub license: Option<StoredLicense>,
    /// Incremented on every license change so a stale copy cannot bring back
    /// a deactivated license when the two stores are merged.
    #[serde(default)]
    pub license_revision: u64,
    /// Message to show once, e.g. after Lemon Squeezy revoked an activation.
    #[serde(default)]
    pub notice: Option<String>,
}

impl LicenseRecord {
    pub fn new(now: i64) -> Self {
        Self {
            version: RECORD_VERSION,
            install_id: random_id(),
            last_seen_at: now,
            trials: BTreeMap::new(),
            license: None,
            license_revision: 0,
            notice: None,
        }
    }

    /// Combines the credential-store copy with the mirror copy.
    pub fn merge(primary: Option<Self>, mirror: Option<Self>) -> Option<Self> {
        let (primary, mirror) = match (primary, mirror) {
            (Some(primary), Some(mirror)) => (primary, mirror),
            (one, other) => return one.or(other),
        };
        let mut merged = primary.clone();
        merged.last_seen_at = primary.last_seen_at.max(mirror.last_seen_at);
        for (major, trial) in mirror.trials {
            merged
                .trials
                .entry(major)
                .and_modify(|existing| {
                    // The earliest start wins; a trial never gets longer.
                    if trial.started_at < existing.started_at {
                        *existing = trial.clone();
                    }
                    if trial.state == TrialState::Expired {
                        existing.state = TrialState::Expired;
                    }
                })
                .or_insert(trial);
        }
        if mirror.license_revision > primary.license_revision {
            merged.license = mirror.license;
            merged.license_revision = mirror.license_revision;
            merged.notice = mirror.notice;
        }
        Some(merged)
    }
}

pub fn random_id() -> String {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).expect("the OS random number generator is available");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Abstracts the credential store so tests can use memory instead.
pub trait SecretStore: Send + Sync {
    fn load(&self) -> Result<Option<String>, String>;
    fn save(&self, value: &str) -> Result<(), String>;
}

pub struct KeyringStore {
    account: &'static str,
}

impl KeyringStore {
    pub fn new(account: &'static str) -> Self {
        Self { account }
    }

    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYRING_SERVICE, self.account).map_err(|error| error.to_string())
    }
}

impl SecretStore for KeyringStore {
    fn load(&self) -> Result<Option<String>, String> {
        match self.entry()?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn save(&self, value: &str) -> Result<(), String> {
        self.entry()?
            .set_password(value)
            .map_err(|error| error.to_string())
    }
}

pub struct LicenseStorage {
    secrets: Box<dyn SecretStore>,
    mirror_path: PathBuf,
}

impl LicenseStorage {
    pub fn new(secrets: Box<dyn SecretStore>, data_dir: PathBuf) -> Self {
        Self {
            secrets,
            mirror_path: data_dir.join(MIRROR_FILE),
        }
    }

    pub fn load(&self, now: i64) -> LicenseRecord {
        let primary = match self.secrets.load() {
            Ok(value) => value.and_then(|text| serde_json::from_str(&text).ok()),
            Err(error) => {
                log::warn!("Credential store unavailable, using the local license mirror: {error}");
                None
            }
        };
        let mirror = self.load_mirror();
        LicenseRecord::merge(primary, mirror).unwrap_or_else(|| LicenseRecord::new(now))
    }

    /// Writes both copies; succeeds if at least one was written.
    pub fn save(&self, record: &LicenseRecord) -> Result<(), String> {
        let text = serde_json::to_string(record).map_err(|error| error.to_string())?;
        let secret_result = self.secrets.save(&text);
        let mirror_result = self.save_mirror(&text);
        match (secret_result, mirror_result) {
            (Err(secret), Err(mirror)) => {
                Err(format!("Could not save license state ({secret}; {mirror})"))
            }
            (Err(error), Ok(())) => {
                log::warn!("Could not update the credential store: {error}");
                Ok(())
            }
            (Ok(()), Err(error)) => {
                log::warn!("Could not update the license mirror: {error}");
                Ok(())
            }
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    fn load_mirror(&self) -> Option<LicenseRecord> {
        let bytes = fs::read(&self.mirror_path).ok()?;
        if bytes.len() < 12 {
            return None;
        }
        let (nonce, ciphertext) = bytes.split_at(12);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&MIRROR_KEY));
        // Decryption fails if the file was edited, so a tampered mirror is ignored.
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: MIRROR_AAD,
                },
            )
            .ok()?;
        serde_json::from_slice(&plaintext).ok()
    }

    fn save_mirror(&self, text: &str) -> Result<(), String> {
        let mut nonce = [0_u8; 12];
        getrandom::getrandom(&mut nonce).map_err(|error| error.to_string())?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&MIRROR_KEY));
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: text.as_bytes(),
                    aad: MIRROR_AAD,
                },
            )
            .map_err(|_| "Could not encrypt license state".to_string())?;
        if let Some(parent) = self.mirror_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut bytes = nonce.to_vec();
        bytes.extend_from_slice(&ciphertext);
        let temp = self.mirror_path.with_extension("tmp");
        fs::write(&temp, bytes).map_err(|error| error.to_string())?;
        fs::rename(&temp, &self.mirror_path).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// In-memory credential store; `available: false` simulates a locked or
    /// missing keychain.
    #[derive(Clone, Default)]
    pub struct MemoryStore {
        pub value: Arc<Mutex<Option<String>>>,
        pub unavailable: bool,
    }

    impl SecretStore for MemoryStore {
        fn load(&self) -> Result<Option<String>, String> {
            if self.unavailable {
                return Err("locked".into());
            }
            Ok(self.value.lock().unwrap().clone())
        }

        fn save(&self, value: &str) -> Result<(), String> {
            if self.unavailable {
                return Err("locked".into());
            }
            *self.value.lock().unwrap() = Some(value.to_string());
            Ok(())
        }
    }

    pub fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gridmode-license-test-{}", random_id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn trial(started_at: i64) -> TrialRecord {
        TrialRecord {
            started_at,
            expires_at: started_at + 10,
            state: TrialState::Active,
        }
    }

    #[test]
    fn round_trips_through_both_stores() {
        let dir = temp_dir();
        let memory = MemoryStore::default();
        let storage = LicenseStorage::new(Box::new(memory.clone()), dir.clone());
        let mut record = LicenseRecord::new(100);
        record.trials.insert(1, trial(100));
        storage.save(&record).unwrap();
        assert_eq!(storage.load(200), record);

        // Only the mirror left (credential entry deleted): still the same trial.
        *memory.value.lock().unwrap() = None;
        assert_eq!(storage.load(200).trials[&1].started_at, 100);

        // Only the credential entry left (app data folder deleted).
        storage.save(&record).unwrap();
        fs::remove_file(dir.join(MIRROR_FILE)).unwrap();
        assert_eq!(storage.load(200).trials[&1].started_at, 100);
        fs::remove_dir_all(dir).unwrap();
    }

    /// Touches the real OS credential store, so it only runs on request:
    /// `cargo test real_credential_store -- --ignored`.
    #[test]
    #[ignore]
    fn real_credential_store_round_trip() {
        let account: &'static str = Box::leak(format!("test-{}", random_id()).into_boxed_str());
        let store = KeyringStore::new(account);
        assert_eq!(store.load().unwrap(), None);
        store.save("{\"hello\":1}").unwrap();
        assert_eq!(store.load().unwrap().as_deref(), Some("{\"hello\":1}"));
        keyring::Entry::new(KEYRING_SERVICE, account)
            .unwrap()
            .delete_credential()
            .unwrap();
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn works_without_a_credential_store() {
        let dir = temp_dir();
        let storage = LicenseStorage::new(
            Box::new(MemoryStore {
                unavailable: true,
                ..MemoryStore::default()
            }),
            dir.clone(),
        );
        let mut record = LicenseRecord::new(5);
        record.trials.insert(1, trial(5));
        storage.save(&record).unwrap();
        assert_eq!(storage.load(9).trials[&1].started_at, 5);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_tampered_mirror_is_ignored() {
        let dir = temp_dir();
        let storage = LicenseStorage::new(Box::new(MemoryStore::default()), dir.clone());
        let mut record = LicenseRecord::new(1);
        record.trials.insert(1, trial(1));
        storage.save(&record).unwrap();
        let path = dir.join(MIRROR_FILE);
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        fs::write(&path, bytes).unwrap();
        assert!(storage.load_mirror().is_none());
        // The credential-store copy still carries the trial.
        assert_eq!(storage.load(2).trials[&1].started_at, 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn merging_keeps_the_earliest_trial_and_newest_license_change() {
        let mut primary = LicenseRecord::new(50);
        primary.trials.insert(1, trial(40));
        let mut mirror = primary.clone();
        mirror.trials.insert(1, trial(10));
        mirror.last_seen_at = 90;
        mirror.license_revision = 3;
        mirror.license = None;
        primary.license_revision = 2;
        primary.license = Some(StoredLicense {
            key: "OLD".into(),
            instance_id: "i".into(),
            instance_name: "n".into(),
            status: "active".into(),
            store_id: 1,
            product_id: 2,
            variant_id: 3,
            product_name: None,
            variant_name: None,
            activated_at: 0,
            last_validated_at: 0,
        });

        let merged = LicenseRecord::merge(Some(primary), Some(mirror)).unwrap();
        assert_eq!(merged.trials[&1].started_at, 10);
        assert_eq!(merged.last_seen_at, 90);
        // Revision 3 (a deactivation) beats the stale revision 2 license.
        assert!(merged.license.is_none());
    }
}
