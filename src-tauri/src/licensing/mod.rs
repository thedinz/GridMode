//! Licensing: a 10-day trial and Lemon Squeezy license keys, entitled per
//! major version. See docs/licensing.md for the full design.
//!
//! - `config`: every Lemon Squeezy identifier and timing constant.
//! - `entitlement`: pure rules for what a license grants.
//! - `api`: the Lemon Squeezy License API client.
//! - `store`: protected persistence of trial and license state.
//! - this module: the service the app and renderer talk to.

pub mod api;
pub mod config;
pub mod entitlement;
pub mod store;

use api::{ApiError, LicenseApi, LicenseResponse};
use chrono::{DateTime, SecondsFormat, Utc};
use config::LemonSqueezyConfig;
use entitlement::{evaluate, Entitlement, KeyStatus, LicenseFacts};
use serde::Serialize;
use std::sync::{Mutex, MutexGuard};
use store::{LicenseRecord, LicenseStorage, StoredLicense, TrialRecord, TrialState};
use tauri::{AppHandle, Emitter, Manager};
use ts_rs::TS;

const DAY_SECONDS: i64 = 24 * 60 * 60;

/// Returned by gated commands when the app is locked; the renderer shows the
/// license screen instead of surfacing this as an error.
pub const LICENSE_REQUIRED: &str = "GridMode needs a license or an active trial.";

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub enum LicenseState {
    /// No license and the trial has not been started.
    TrialAvailable,
    Trial,
    TrialExpired,
    Licensed,
    /// Licensed, but offline for longer than the grace period.
    ValidationRequired,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct LicenseSummary {
    /// Only the last four characters are shown, e.g. `••••-••••-••••-ABCD`.
    pub masked_key: String,
    /// Major version this license covers, when it is a GridMode license.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub major: Option<u32>,
    /// Lemon Squeezy's key status at the last check.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub variant_name: Option<String>,
    pub last_validated_at: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct LicenseStatus {
    pub state: LicenseState,
    pub can_use_app: bool,
    /// Major version of this build, e.g. 1 for GridMode 1.x.
    pub app_major: u32,
    /// False until Lemon Squeezy identifiers are filled in for this build.
    pub configured: bool,
    pub mode_label: String,
    pub price_label: String,
    pub trial_length_days: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub trial_days_remaining: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub trial_ends_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub license: Option<LicenseSummary>,
    /// Set when this installation holds a genuine GridMode license for a
    /// different major version (e.g. a 1.x license in a 2.x build).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub other_major_license: Option<u32>,
    /// Revalidation is overdue but within the offline grace period. For quiet
    /// display only; the app stays fully usable.
    pub offline: bool,
    /// A one-time message, e.g. after Lemon Squeezy revoked the license.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub notice: Option<String>,
    pub checkout_available: bool,
}

pub fn unix_now() -> i64 {
    Utc::now().timestamp()
}

fn iso(timestamp: i64) -> String {
    DateTime::<Utc>::from_timestamp(timestamp, 0)
        .unwrap_or_default()
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub fn mask_key(key: &str) -> String {
    let tail: String = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("••••-••••-••••-{}", tail.to_uppercase())
}

/// A privacy-conscious Lemon Squeezy instance name: product, OS, and a short
/// random installation ID. No hostname or user name.
pub fn instance_name(install_id: &str) -> String {
    let os = match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    };
    let short_id: String = install_id.chars().take(6).collect();
    format!("GridMode · {os} · {short_id}")
}

fn stored_facts(license: &StoredLicense) -> LicenseFacts {
    LicenseFacts {
        status: KeyStatus::parse(&license.status),
        store_id: license.store_id,
        product_id: license.product_id,
        variant_id: license.variant_id,
    }
}

/// Derives what the UI shows from the stored record. Pure, for testing.
pub fn compute_status(
    record: &LicenseRecord,
    config: &LemonSqueezyConfig,
    app_major: u32,
    now: i64,
) -> LicenseStatus {
    // Never let a clock turned backwards extend a trial.
    let now = now.max(record.last_seen_at);
    let mut status = LicenseStatus {
        state: LicenseState::TrialAvailable,
        can_use_app: false,
        app_major,
        configured: config.is_configured(),
        mode_label: config.mode_label.to_string(),
        price_label: config::PRICE_LABEL.to_string(),
        trial_length_days: config::TRIAL_DAYS as u32,
        trial_days_remaining: None,
        trial_ends_at: None,
        license: None,
        other_major_license: None,
        offline: false,
        notice: record.notice.clone(),
        checkout_available: config.checkout_url.starts_with("https://"),
    };

    if let Some(license) = &record.license {
        let entitlement = evaluate(&stored_facts(license), config, app_major);
        status.license = Some(LicenseSummary {
            masked_key: mask_key(&license.key),
            major: entitlement::major_for_variant(config, license.variant_id),
            status: license.status.clone(),
            variant_name: license.variant_name.clone(),
            last_validated_at: iso(license.last_validated_at),
        });
        match entitlement {
            Entitlement::Entitled { .. } => {
                let since_validated = now - license.last_validated_at;
                if since_validated <= config::OFFLINE_GRACE_PERIOD.as_secs() as i64 {
                    status.state = LicenseState::Licensed;
                    status.can_use_app = true;
                    status.offline = since_validated > config::VALIDATION_INTERVAL.as_secs() as i64;
                } else {
                    status.state = LicenseState::ValidationRequired;
                }
                return status;
            }
            Entitlement::OtherMajor { license_major } => {
                status.other_major_license = Some(license_major)
            }
            // Expired, disabled, or no longer matching this build's
            // configuration: fall back to the trial.
            Entitlement::Expired
            | Entitlement::Disabled
            | Entitlement::WrongProduct
            | Entitlement::NotConfigured => {}
        }
    }

    if let Some(trial) = record.trials.get(&app_major) {
        let remaining = trial.expires_at - now;
        status.trial_ends_at = Some(iso(trial.expires_at));
        if trial.state == TrialState::Active && remaining > 0 {
            status.state = LicenseState::Trial;
            status.can_use_app = true;
            status.trial_days_remaining =
                Some(((remaining + DAY_SECONDS - 1) / DAY_SECONDS) as u32);
        } else {
            status.state = LicenseState::TrialExpired;
        }
    }
    status
}

pub struct LicenseService {
    storage: LicenseStorage,
    record: Mutex<LicenseRecord>,
    api: LicenseApi,
    config: LemonSqueezyConfig,
    app_major: u32,
    /// Serializes activate / deactivate / revalidate so they never interleave.
    operation: tauri::async_runtime::Mutex<()>,
}

impl LicenseService {
    pub fn new(
        storage: LicenseStorage,
        api: LicenseApi,
        config: LemonSqueezyConfig,
        app_major: u32,
        now: i64,
    ) -> Self {
        let record = storage.load(now);
        let service = Self {
            storage,
            record: Mutex::new(record),
            api,
            config,
            app_major,
            operation: tauri::async_runtime::Mutex::new(()),
        };
        service.touch(now);
        service
    }

    fn record(&self) -> MutexGuard<'_, LicenseRecord> {
        self.record
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn update(&self, change: impl FnOnce(&mut LicenseRecord)) -> Result<(), String> {
        let snapshot = {
            let mut record = self.record();
            change(&mut record);
            record.clone()
        };
        self.storage.save(&snapshot)
    }

    pub fn status(&self, now: i64) -> LicenseStatus {
        compute_status(&self.record(), &self.config, self.app_major, now)
    }

    pub fn can_use_app(&self, now: i64) -> bool {
        self.status(now).can_use_app
    }

    /// Records the current time and persists an ended trial as expired.
    pub fn touch(&self, now: i64) {
        let app_major = self.app_major;
        let result = self.update(|record| {
            record.last_seen_at = record.last_seen_at.max(now);
            let effective_now = record.last_seen_at;
            if let Some(trial) = record.trials.get_mut(&app_major) {
                if trial.state == TrialState::Active && effective_now >= trial.expires_at {
                    trial.state = TrialState::Expired;
                }
            }
        });
        if let Err(error) = result {
            log::warn!("Could not save license state: {error}");
        }
    }

    pub fn start_trial(&self, now: i64) -> Result<LicenseStatus, String> {
        let app_major = self.app_major;
        self.update(|record| {
            // Starting again never resets an existing trial.
            record.trials.entry(app_major).or_insert(TrialRecord {
                started_at: now,
                expires_at: now + config::TRIAL_DAYS * DAY_SECONDS,
                state: TrialState::Active,
            });
            record.notice = None;
        })?;
        Ok(self.status(now))
    }

    pub fn dismiss_notice(&self, now: i64) -> Result<LicenseStatus, String> {
        self.update(|record| record.notice = None)?;
        Ok(self.status(now))
    }

    /// Checks a key with Lemon Squeezy, then activates this installation.
    /// The key is validated first so a key for another product or major
    /// version is refused without using up one of its activations.
    pub async fn activate(&self, license_key: &str, now: i64) -> Result<LicenseStatus, String> {
        let _operation = self.operation.lock().await;
        let license_key = license_key.trim();
        if license_key.is_empty() {
            return Err("Enter your GridMode license key.".to_string());
        }
        if !self.config.is_configured() {
            return Err("Licensing isn't set up in this build of GridMode yet.".to_string());
        }

        let precheck = match self.api.validate(license_key, None).await {
            Ok(response) => response,
            Err(ApiError::Rejected(response)) if response.facts().is_some() => *response,
            Err(error) => return Err(activation_error(&error)),
        };
        self.require_entitlement(&precheck)?;

        let existing = self.record().license.clone();
        if let Some(existing) = &existing {
            if existing.key == license_key {
                // Already active here; confirm the activation instead of adding another.
                drop(_operation);
                return self
                    .revalidate(now, true)
                    .await
                    .map_err(|error| error.to_string());
            }
            // Switching keys: release the old activation if Lemon Squeezy is reachable.
            let _ = self
                .api
                .deactivate(&existing.key, &existing.instance_id)
                .await;
        }

        let install_id = self.record().install_id.clone();
        let name = instance_name(&install_id);
        let activation = self
            .api
            .activate(license_key, &name)
            .await
            .map_err(|error| activation_error(&error))?;
        let instance_id = activation
            .instance
            .as_ref()
            .map(|instance| instance.id.clone())
            .ok_or_else(|| {
                "Lemon Squeezy did not return an activation for this computer.".to_string()
            })?;
        // Defense in depth: re-check what the activation itself reports.
        if let Err(error) = self.require_entitlement(&activation) {
            let _ = self.api.deactivate(license_key, &instance_id).await;
            return Err(error);
        }

        let meta = activation
            .meta
            .clone()
            .expect("checked by require_entitlement");
        let status = activation
            .facts()
            .map(|facts| facts.status.as_str())
            .unwrap_or("active")
            .to_string();
        self.update(|record| {
            record.license = Some(StoredLicense {
                key: license_key.to_string(),
                instance_id,
                instance_name: name,
                status,
                store_id: meta.store_id,
                product_id: meta.product_id,
                variant_id: meta.variant_id,
                product_name: meta.product_name,
                variant_name: meta.variant_name,
                activated_at: now,
                last_validated_at: now,
            });
            record.license_revision += 1;
            record.notice = None;
        })?;
        Ok(self.status(now))
    }

    fn require_entitlement(&self, response: &LicenseResponse) -> Result<(), String> {
        let facts = response
            .facts()
            .ok_or_else(|| invalid_key_message().to_string())?;
        match evaluate(&facts, &self.config, self.app_major) {
            Entitlement::Entitled { .. } => Ok(()),
            Entitlement::OtherMajor { license_major } => Err(format!(
                "This license is for GridMode {license_major}.x and does not include GridMode {}.",
                self.app_major
            )),
            Entitlement::WrongProduct => Err("This license key isn't for GridMode.".to_string()),
            Entitlement::Expired => Err("This license key has expired.".to_string()),
            Entitlement::Disabled => {
                Err("This license key has been disabled. Contact support if you think this is a mistake.".to_string())
            }
            Entitlement::NotConfigured => Err("Licensing isn't set up in this build of GridMode yet.".to_string()),
        }
    }

    /// Releases this installation's activation so the key can be used elsewhere.
    pub async fn deactivate(&self, now: i64) -> Result<LicenseStatus, String> {
        let _operation = self.operation.lock().await;
        let license = self
            .record()
            .license
            .clone()
            .ok_or_else(|| "This computer isn't activated.".to_string())?;
        match self.api.deactivate(&license.key, &license.instance_id).await {
            Ok(_) => {}
            // Already gone on Lemon Squeezy's side (e.g. removed in the dashboard).
            Err(ApiError::Rejected(response)) if response.is_instance_missing() => {}
            Err(ApiError::Rejected(response)) => {
                return Err(format!(
                    "Lemon Squeezy couldn't deactivate this computer: {}",
                    response.error.clone().unwrap_or_else(|| "unknown error".to_string())
                ))
            }
            Err(ApiError::Unreachable(_)) => {
                return Err(
                    "Couldn't reach Lemon Squeezy. Connect to the internet to deactivate, so the activation is released for another computer."
                        .to_string(),
                )
            }
        }
        self.update(|record| {
            record.license = None;
            record.license_revision += 1;
        })?;
        Ok(self.status(now))
    }

    /// Confirms the stored activation with Lemon Squeezy when it is due (or
    /// always, with `force`). Network failures change nothing; only an
    /// explicit answer from Lemon Squeezy can revoke a license.
    pub async fn revalidate(&self, now: i64, force: bool) -> Result<LicenseStatus, String> {
        let _operation = self.operation.lock().await;
        let Some(license) = self.record().license.clone() else {
            return Ok(self.status(now));
        };
        let due = now - license.last_validated_at >= config::VALIDATION_INTERVAL.as_secs() as i64;
        if !force && !due {
            return Ok(self.status(now));
        }

        let result = self
            .api
            .validate(&license.key, Some(&license.instance_id))
            .await;
        let response = match result {
            Ok(response) => response,
            Err(ApiError::Unreachable(error)) => {
                log::info!("License check skipped, Lemon Squeezy unreachable: {error}");
                if force {
                    return Err(
                        "Couldn't reach Lemon Squeezy. GridMode will try again later.".to_string(),
                    );
                }
                return Ok(self.status(now));
            }
            Err(ApiError::Rejected(response)) => {
                let revoked = response
                    .facts()
                    .map(|facts| matches!(facts.status, KeyStatus::Expired | KeyStatus::Disabled))
                    .unwrap_or(false);
                if !revoked {
                    let notice = if response.is_instance_missing() {
                        "This computer was deactivated from your GridMode license. Enter your license key to activate it again."
                    } else {
                        "Lemon Squeezy no longer recognizes this license key."
                    };
                    self.update(|record| {
                        record.license = None;
                        record.license_revision += 1;
                        record.notice = Some(notice.to_string());
                    })?;
                    return Ok(self.status(now));
                }
                *response
            }
        };

        let facts = response.facts();
        let entitlement = facts
            .map(|facts| evaluate(&facts, &self.config, self.app_major))
            .unwrap_or(Entitlement::WrongProduct);
        self.update(|record| {
            let Some(stored) = record.license.as_mut() else {
                return;
            };
            if let Some(facts) = facts {
                stored.status = facts.status.as_str().to_string();
            }
            if let Some(meta) = &response.meta {
                stored.variant_name = meta.variant_name.clone().or(stored.variant_name.take());
            }
            match entitlement {
                Entitlement::Entitled { .. } | Entitlement::OtherMajor { .. } => {
                    stored.last_validated_at = now
                }
                Entitlement::Expired => {
                    record.notice =
                        Some("Lemon Squeezy reports that this license key has expired.".to_string())
                }
                Entitlement::Disabled => {
                    record.notice = Some(
                        "Lemon Squeezy reports that this license key has been disabled."
                            .to_string(),
                    )
                }
                Entitlement::WrongProduct | Entitlement::NotConfigured => {
                    record.license = None;
                    record.license_revision += 1;
                    record.notice = Some("This license key isn't for GridMode.".to_string());
                }
            }
        })?;
        Ok(self.status(now))
    }
}

fn invalid_key_message() -> &'static str {
    "We couldn't validate that license key. Check the key and try again."
}

fn activation_error(error: &ApiError) -> String {
    match error {
        ApiError::Unreachable(_) => {
            "Couldn't reach Lemon Squeezy. Check your internet connection and try again.".to_string()
        }
        ApiError::Rejected(response) if response.is_activation_limit() => format!(
            "This license is already active on {} computers. Deactivate GridMode on one of them (Settings → License), then try again.",
            config::ACTIVATION_LIMIT
        ),
        ApiError::Rejected(_) => invalid_key_message().to_string(),
    }
}

// ---------------------------------------------------------------------------
// App integration

/// Fails gated commands when there is neither a license nor an active trial.
pub fn require_access(app: &AppHandle) -> Result<(), String> {
    if app.state::<LicenseService>().can_use_app(unix_now()) {
        Ok(())
    } else {
        Err(LICENSE_REQUIRED.to_string())
    }
}

/// Revalidates in the background: shortly after launch, then periodically.
/// Emits `license:changed` whenever the status changes.
pub fn start_background_validation(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(20));
        loop {
            let service = app.state::<LicenseService>();
            let before = service.status(unix_now());
            service.touch(unix_now());
            let _ = tauri::async_runtime::block_on(service.revalidate(unix_now(), false));
            let after = service.status(unix_now());
            if after != before {
                let _ = app.emit("license:changed", after);
            }
            std::thread::sleep(config::REVALIDATION_POLL);
        }
    });
}

fn emit_status(app: &AppHandle, status: &LicenseStatus) {
    let _ = app.emit("license:changed", status.clone());
}

#[tauri::command]
pub async fn license_get_status(app: AppHandle) -> Result<LicenseStatus, String> {
    Ok(app.state::<LicenseService>().status(unix_now()))
}

#[tauri::command]
pub async fn license_start_trial(app: AppHandle) -> Result<LicenseStatus, String> {
    let status = app.state::<LicenseService>().start_trial(unix_now())?;
    emit_status(&app, &status);
    Ok(status)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn license_activate(
    app: AppHandle,
    license_key: String,
) -> Result<LicenseStatus, String> {
    let status = app
        .state::<LicenseService>()
        .activate(&license_key, unix_now())
        .await?;
    emit_status(&app, &status);
    Ok(status)
}

#[tauri::command]
pub async fn license_deactivate(app: AppHandle) -> Result<LicenseStatus, String> {
    let status = app.state::<LicenseService>().deactivate(unix_now()).await?;
    emit_status(&app, &status);
    Ok(status)
}

#[tauri::command]
pub async fn license_refresh(app: AppHandle) -> Result<LicenseStatus, String> {
    let status = app
        .state::<LicenseService>()
        .revalidate(unix_now(), true)
        .await?;
    emit_status(&app, &status);
    Ok(status)
}

#[tauri::command]
pub async fn license_dismiss_notice(app: AppHandle) -> Result<LicenseStatus, String> {
    let status = app.state::<LicenseService>().dismiss_notice(unix_now())?;
    emit_status(&app, &status);
    Ok(status)
}

#[tauri::command]
pub async fn license_open_checkout(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let url = app.state::<LicenseService>().config.checkout_url;
    if !url.starts_with("https://") {
        return Err("The GridMode checkout page isn't set up in this build yet.".to_string());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| format!("Could not open the checkout page: {error}"))
}

#[cfg(test)]
mod tests {
    use super::api::tests::{StubServer, VALID_V1};
    use super::store::tests::{temp_dir, MemoryStore};
    use super::*;

    const CONFIG: LemonSqueezyConfig = LemonSqueezyConfig {
        mode_label: "Test Mode",
        store_id: 10,
        product_id: 20,
        major_variants: &[(1, &[111])],
        checkout_url: "https://gridmode.lemonsqueezy.com/checkout/buy/test",
    };
    const DAY: i64 = DAY_SECONDS;
    const T0: i64 = 1_800_000_000;

    const ACTIVATED_V1: &str = r#"{"activated":true,"error":null,"license_key":{"status":"active"},"instance":{"id":"inst-1"},"meta":{"store_id":10,"product_id":20,"variant_id":111,"product_name":"GridMode","variant_name":"GridMode 1.x"}}"#;
    const VALID_OTHER_PRODUCT: &str = r#"{"valid":true,"license_key":{"status":"active"},"meta":{"store_id":10,"product_id":99,"variant_id":5}}"#;
    const VALID_V2: &str = r#"{"valid":true,"license_key":{"status":"active"},"meta":{"store_id":10,"product_id":20,"variant_id":222}}"#;
    const NOT_FOUND: &str = r#"{"valid":false,"error":"license_key not found."}"#;
    const LIMIT: &str = r#"{"activated":false,"error":"This license key has reached the activation limit.","license_key":{"status":"active"},"meta":{"store_id":10,"product_id":20,"variant_id":111}}"#;
    const DEACTIVATED: &str = r#"{"deactivated":true,"error":null}"#;
    const INSTANCE_GONE: &str = r#"{"valid":false,"error":"instance_id not found."}"#;
    const DISABLED: &str = r#"{"valid":false,"error":null,"license_key":{"status":"disabled"},"meta":{"store_id":10,"product_id":20,"variant_id":111}}"#;

    fn service(
        url: &str,
        memory: MemoryStore,
        dir: std::path::PathBuf,
        now: i64,
    ) -> LicenseService {
        LicenseService::new(
            LicenseStorage::new(Box::new(memory), dir),
            LicenseApi::new(url),
            CONFIG,
            1,
            now,
        )
    }

    fn run<T>(future: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(future)
    }

    #[test]
    fn new_installations_offer_a_trial_that_counts_down_and_ends() {
        let dir = temp_dir();
        let memory = MemoryStore::default();
        let svc = service("http://127.0.0.1:9", memory.clone(), dir.clone(), T0);
        let first = svc.status(T0);
        assert_eq!(first.state, LicenseState::TrialAvailable);
        assert!(!first.can_use_app);

        let started = svc.start_trial(T0).unwrap();
        assert_eq!(
            (started.state, started.trial_days_remaining),
            (LicenseState::Trial, Some(10))
        );
        assert_eq!(svc.status(T0 + 3 * DAY + 60).trial_days_remaining, Some(7));

        // A restart reads the same trial back; starting again does not reset it.
        let restarted = service("http://127.0.0.1:9", memory, dir.clone(), T0 + 3 * DAY);
        restarted.start_trial(T0 + 3 * DAY).unwrap();
        assert_eq!(restarted.status(T0 + 3 * DAY).trial_days_remaining, Some(7));

        let ended = restarted.status(T0 + 10 * DAY);
        assert_eq!(ended.state, LicenseState::TrialExpired);
        assert!(!ended.can_use_app);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn turning_the_clock_back_does_not_extend_the_trial() {
        let dir = temp_dir();
        let svc = service(
            "http://127.0.0.1:9",
            MemoryStore::default(),
            dir.clone(),
            T0,
        );
        svc.start_trial(T0).unwrap();
        svc.touch(T0 + 9 * DAY);
        assert_eq!(svc.status(T0).trial_days_remaining, Some(1));
        svc.touch(T0 + 11 * DAY);
        assert_eq!(svc.status(T0 + 2 * DAY).state, LicenseState::TrialExpired);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn activates_a_gridmode_1x_key_and_deactivates_it() {
        let dir = temp_dir();
        let server = StubServer::start(vec![
            (200, VALID_V1),
            (200, ACTIVATED_V1),
            (200, DEACTIVATED),
        ]);
        let svc = service(&server.url, MemoryStore::default(), dir.clone(), T0);

        let status = run(svc.activate("  38b1460a-5104-4067-a91d-77b872934d51 ", T0)).unwrap();
        assert_eq!(status.state, LicenseState::Licensed);
        let summary = status.license.unwrap();
        assert_eq!(summary.masked_key, "••••-••••-••••-4D51");
        assert_eq!(summary.major, Some(1));

        let requests = server.requests.lock().unwrap().clone();
        assert!(requests[0].starts_with("POST /validate"));
        assert!(
            requests[1].starts_with("POST /activate")
                && requests[1].contains("instance_name=GridMode")
        );
        assert!(
            !requests[1].contains(&whoami_hint()),
            "instance name must not include the user name"
        );

        let after = run(svc.deactivate(T0)).unwrap();
        assert_eq!(after.state, LicenseState::TrialAvailable);
        assert!(server.requests.lock().unwrap()[2].contains("instance_id=inst-1"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn whoami_hint() -> String {
        std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "\u{0}".into())
    }

    #[test]
    fn refuses_invalid_wrong_product_and_wrong_major_keys_without_activating() {
        let dir = temp_dir();
        let server = StubServer::start(vec![
            (404, NOT_FOUND),
            (200, VALID_OTHER_PRODUCT),
            (200, VALID_V2),
        ]);
        let svc = service(&server.url, MemoryStore::default(), dir.clone(), T0);

        let invalid = run(svc.activate("nope", T0)).unwrap_err();
        assert_eq!(
            invalid,
            "We couldn't validate that license key. Check the key and try again."
        );
        let other = run(svc.activate("other-product", T0)).unwrap_err();
        assert_eq!(other, "This license key isn't for GridMode.");
        // A 1.x build does not know the 2.x variant, so it is not GridMode 1.x.
        let v2 = run(svc.activate("v2-key", T0)).unwrap_err();
        assert_eq!(v2, "This license key isn't for GridMode.");

        let requests = server.requests.lock().unwrap();
        assert!(
            requests
                .iter()
                .all(|request| request.starts_with("POST /validate")),
            "{requests:?}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reports_the_activation_limit() {
        let dir = temp_dir();
        let server = StubServer::start(vec![(200, VALID_V1), (400, LIMIT)]);
        let svc = service(&server.url, MemoryStore::default(), dir.clone(), T0);
        let error = run(svc.activate("key", T0)).unwrap_err();
        assert!(
            error.starts_with("This license is already active on 2 computers."),
            "{error}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn licensed_service(
        responses: Vec<(u16, &'static str)>,
    ) -> (LicenseService, StubServer, std::path::PathBuf) {
        let dir = temp_dir();
        let mut all = vec![(200, VALID_V1), (200, ACTIVATED_V1)];
        all.extend(responses);
        let server = StubServer::start(all);
        let svc = service(&server.url, MemoryStore::default(), dir.clone(), T0);
        run(svc.activate("key", T0)).unwrap();
        (svc, server, dir)
    }

    #[test]
    fn offline_launches_keep_working_through_the_grace_period() {
        // No further stub responses: every revalidation finds nothing listening.
        let (svc, _server, dir) = licensed_service(vec![]);
        let later = T0 + 10 * DAY;
        let status = run(svc.revalidate(later, false)).unwrap();
        assert_eq!(status.state, LicenseState::Licensed);
        assert!(status.offline);
        assert!(status.can_use_app);

        let beyond_grace = T0 + 31 * DAY;
        let status = run(svc.revalidate(beyond_grace, false)).unwrap();
        assert_eq!(status.state, LicenseState::ValidationRequired);
        assert!(!status.can_use_app);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn successful_revalidation_renews_the_grace_period() {
        let (svc, _server, dir) = licensed_service(vec![(200, VALID_V1)]);
        let status = run(svc.revalidate(T0 + 20 * DAY, false)).unwrap();
        assert_eq!(
            (status.state, status.offline),
            (LicenseState::Licensed, false)
        );
        assert_eq!(svc.status(T0 + 45 * DAY).state, LicenseState::Licensed);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn revalidation_is_skipped_until_due() {
        let (svc, server, dir) = licensed_service(vec![]);
        run(svc.revalidate(T0 + DAY, false)).unwrap();
        assert_eq!(
            server.requests.lock().unwrap().len(),
            2,
            "no validate call before the interval"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn explicit_revocation_ends_the_license_but_outages_do_not() {
        let (svc, _server, dir) = licensed_service(vec![(503, "down"), (200, DISABLED)]);
        let outage = run(svc.revalidate(T0 + 4 * DAY, false)).unwrap();
        assert_eq!(outage.state, LicenseState::Licensed);

        let disabled = run(svc.revalidate(T0 + 4 * DAY, false)).unwrap();
        assert_ne!(disabled.state, LicenseState::Licensed);
        assert!(!disabled.can_use_app);
        assert!(disabled.notice.unwrap().contains("disabled"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_deactivated_instance_returns_to_the_trial_with_a_notice() {
        let (svc, _server, dir) = licensed_service(vec![(404, INSTANCE_GONE)]);
        svc.start_trial(T0).unwrap();
        let status = run(svc.revalidate(T0 + 4 * DAY, false)).unwrap();
        assert_eq!(status.state, LicenseState::Trial);
        assert!(status.license.is_none());
        assert!(status.notice.unwrap().contains("deactivated"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_future_2x_build_explains_a_valid_1x_license() {
        const V2_BUILD: LemonSqueezyConfig = LemonSqueezyConfig {
            major_variants: &[(1, &[111]), (2, &[222])],
            ..CONFIG
        };
        let mut record = LicenseRecord::new(T0);
        record.license = Some(StoredLicense {
            key: "key-abcd".into(),
            instance_id: "i".into(),
            instance_name: "n".into(),
            status: "active".into(),
            store_id: 10,
            product_id: 20,
            variant_id: 111,
            product_name: None,
            variant_name: None,
            activated_at: T0,
            last_validated_at: T0,
        });
        let status = compute_status(&record, &V2_BUILD, 2, T0);
        assert_eq!(status.other_major_license, Some(1));
        assert_eq!(status.state, LicenseState::TrialAvailable);
        assert_eq!(status.license.unwrap().major, Some(1));
    }

    #[test]
    fn instance_names_hold_no_personal_details() {
        let name = instance_name("a1b2c3d4e5");
        assert!(
            name.starts_with("GridMode · ") && name.ends_with(" · a1b2c3"),
            "{name}"
        );
    }
}
