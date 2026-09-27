//! Auto-update commands built on the Tauri updater, with a manual GitHub
//! Releases download fallback for macOS.

use crate::model::{UpdateState, UpdateStatus};
use serde::Deserialize;
use std::sync::Mutex;
use tauri::{http, AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::{Update, UpdaterExt};

#[cfg(target_os = "macos")]
const GITHUB_LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/thedinz/GridMode/releases/latest";
const GITHUB_RELEASE_PATH_PREFIX: &str = "/thedinz/GridMode/";

#[derive(Default)]
pub struct PendingUpdateState {
    inner: Mutex<Option<PendingUpdate>>,
}

struct PendingUpdate {
    update: Update,
    bytes: Option<Vec<u8>>,
}

impl PendingUpdateState {
    fn set(&self, pending: Option<PendingUpdate>) {
        *self.inner.lock().unwrap_or_else(|error| error.into_inner()) = pending;
    }

    fn update(&self) -> Option<Update> {
        let pending = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        pending.as_ref().map(|pending| pending.update.clone())
    }

    fn downloaded(&self) -> Option<(Update, Vec<u8>)> {
        let pending = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        pending
            .as_ref()
            .and_then(|pending| Some((pending.update.clone(), pending.bytes.clone()?)))
    }
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    browser_download_url: String,
}

fn emit(app: &AppHandle, status: &UpdateStatus) {
    let _ = app.emit("updates:status", status.clone());
}

fn status_for(
    state: UpdateState,
    update: &Update,
    message: &str,
    percent: Option<f64>,
) -> UpdateStatus {
    UpdateStatus {
        version: Some(update.version.clone()),
        percent,
        ..UpdateStatus::new(state, Some(message))
    }
}

fn error_status(message: impl Into<String>) -> UpdateStatus {
    let message = message.into();
    UpdateStatus::new(UpdateState::Error, Some(&message))
}

#[tauri::command]
pub async fn updates_check(
    app: AppHandle,
    pending_update: State<'_, PendingUpdateState>,
    automatic: Option<bool>,
) -> Result<UpdateStatus, String> {
    let automatic = automatic.unwrap_or(false);
    if !automatic {
        emit(
            &app,
            &UpdateStatus::new(UpdateState::Checking, Some("Checking for updates...")),
        );
    }

    // Automatic checks stay silent unless an update is actually available.
    let finish = |status: UpdateStatus| {
        let silent = automatic && status.state != UpdateState::Available;
        if silent {
            return UpdateStatus::new(UpdateState::Idle, None);
        }
        emit(&app, &status);
        status
    };

    let checked = match app.updater() {
        Ok(updater) => updater
            .check()
            .await
            .map_err(|error| format!("Update check failed: {error}")),
        Err(error) => Err(format!("Could not start updater: {error}")),
    };

    match checked {
        Ok(Some(update)) => {
            let mut status = status_for(
                UpdateState::Available,
                &update,
                "A GridMode update is ready to download.",
                None,
            );
            status.download_url = Some(update.download_url.to_string());
            pending_update.set(Some(PendingUpdate {
                update,
                bytes: None,
            }));
            Ok(finish(status))
        }
        Ok(None) => {
            pending_update.set(None);
            match manual_download_status(automatic).await? {
                Some(status) => Ok(finish(status)),
                None => Ok(finish(UpdateStatus::new(
                    UpdateState::NotAvailable,
                    Some("GridMode is up to date."),
                ))),
            }
        }
        Err(error) => match manual_download_status(automatic).await? {
            Some(status) => Ok(finish(status)),
            None => Ok(finish(error_status(error))),
        },
    }
}

#[tauri::command(rename_all = "camelCase")]
pub fn updates_open_download(app: AppHandle, download_url: String) -> Result<UpdateStatus, String> {
    validate_external_download_url(&download_url)?;
    app.opener()
        .open_url(&download_url, None::<&str>)
        .map_err(|error| format!("Could not open download URL: {error}"))?;

    let status = UpdateStatus {
        download_url: Some(download_url),
        manual_download: Some(true),
        ..UpdateStatus::new(
            UpdateState::Available,
            Some("Opened the GridMode download in your browser."),
        )
    };
    emit(&app, &status);
    Ok(status)
}

#[tauri::command]
pub async fn updates_download(
    app: AppHandle,
    pending_update: State<'_, PendingUpdateState>,
) -> Result<UpdateStatus, String> {
    let update = match pending_update.update() {
        Some(update) => update,
        None => {
            emit(
                &app,
                &UpdateStatus::new(UpdateState::Checking, Some("Checking for updates...")),
            );
            let checked = match app.updater() {
                Ok(updater) => updater
                    .check()
                    .await
                    .map_err(|error| format!("Update check failed: {error}")),
                Err(error) => Err(format!("Could not start updater: {error}")),
            };
            match checked {
                Ok(Some(update)) => update,
                Ok(None) => {
                    pending_update.set(None);
                    let status = UpdateStatus::new(
                        UpdateState::NotAvailable,
                        Some("GridMode is up to date."),
                    );
                    emit(&app, &status);
                    return Ok(status);
                }
                Err(error) => {
                    let status = error_status(error);
                    emit(&app, &status);
                    return Ok(status);
                }
            }
        }
    };

    emit(
        &app,
        &status_for(
            UpdateState::Downloading,
            &update,
            "Downloading update...",
            Some(0.0),
        ),
    );

    let progress_app = app.clone();
    let progress_update = update.clone();
    let mut downloaded = 0_u64;
    let bytes = update
        .download(
            |chunk_length, content_length| {
                downloaded += chunk_length as u64;
                let percent = content_length
                    .filter(|total| *total > 0)
                    .map(|total| (downloaded as f64 / total as f64) * 100.0);
                emit(
                    &progress_app,
                    &status_for(
                        UpdateState::Downloading,
                        &progress_update,
                        "Downloading update...",
                        percent,
                    ),
                );
            },
            || {},
        )
        .await;

    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => {
            let status = error_status(format!("Update download failed: {error}"));
            emit(&app, &status);
            return Ok(status);
        }
    };

    let status = status_for(
        UpdateState::Downloaded,
        &update,
        "Update downloaded. Install it to finish updating GridMode.",
        Some(100.0),
    );
    pending_update.set(Some(PendingUpdate {
        update,
        bytes: Some(bytes),
    }));
    emit(&app, &status);
    Ok(status)
}

#[tauri::command]
pub fn updates_install(
    app: AppHandle,
    pending_update: State<'_, PendingUpdateState>,
) -> Result<UpdateStatus, String> {
    let Some((update, bytes)) = pending_update.downloaded() else {
        let status = error_status("No downloaded update is ready to install.");
        emit(&app, &status);
        return Ok(status);
    };

    emit(
        &app,
        &status_for(
            UpdateState::Downloaded,
            &update,
            "Installing update...",
            Some(100.0),
        ),
    );
    match update.install(bytes) {
        Ok(()) => {
            pending_update.set(None);
            emit(
                &app,
                &status_for(
                    UpdateState::Downloaded,
                    &update,
                    "Update installed. Restarting GridMode...",
                    Some(100.0),
                ),
            );
            app.restart()
        }
        Err(error) => {
            let status = error_status(format!("Update install failed: {error}"));
            emit(&app, &status);
            Ok(status)
        }
    }
}

async fn manual_download_status(automatic: bool) -> Result<Option<UpdateStatus>, String> {
    let status = manual_download_status_for_platform().await;
    if automatic {
        return Ok(status.unwrap_or(None));
    }
    status
}

#[cfg(target_os = "macos")]
async fn manual_download_status_for_platform() -> Result<Option<UpdateStatus>, String> {
    let response = reqwest::Client::new()
        .get(GITHUB_LATEST_RELEASE_API)
        .header(reqwest::header::USER_AGENT, "GridMode updater")
        .send()
        .await
        .map_err(|error| format!("Could not check GitHub Releases: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub Releases check failed with HTTP {}",
            response.status()
        ));
    }
    let release = response
        .json::<GitHubRelease>()
        .await
        .map_err(|error| format!("Could not read GitHub Releases response: {error}"))?;

    let latest_version = normalize_version_tag(&release.tag_name);
    if !is_newer_version(&latest_version, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let download_url = macos_release_download_url(&release).unwrap_or(release.html_url);
    validate_external_download_url(&download_url)?;

    Ok(Some(UpdateStatus {
        version: Some(latest_version),
        download_url: Some(download_url),
        manual_download: Some(true),
        ..UpdateStatus::new(
            UpdateState::Available,
            Some("A GridMode update is available for macOS."),
        )
    }))
}

#[cfg(not(target_os = "macos"))]
async fn manual_download_status_for_platform() -> Result<Option<UpdateStatus>, String> {
    Ok(None)
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn macos_release_download_url(release: &GitHubRelease) -> Option<String> {
    let dmg_assets: Vec<&GitHubReleaseAsset> = release
        .assets
        .iter()
        .filter(|asset| asset.name.to_lowercase().ends_with(".dmg"))
        .collect();
    dmg_assets
        .iter()
        .find(|asset| asset.name.to_lowercase().contains("universal"))
        .or(dmg_assets.first())
        .map(|asset| asset.browser_download_url.clone())
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn normalize_version_tag(version: &str) -> String {
    version.trim().trim_start_matches(['v', 'V']).to_string()
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn is_newer_version(candidate: &str, current: &str) -> bool {
    match (version_triplet(candidate), version_triplet(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => candidate != current,
    }
}

fn version_triplet(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version
        .split(['.', '-', '+'])
        .take(3)
        .map(str::parse::<u64>);
    Some((
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    ))
}

fn validate_external_download_url(url: &str) -> Result<(), String> {
    let uri: http::Uri = url
        .parse()
        .map_err(|error| format!("Download URL could not be parsed: {error}"))?;
    let is_github_gridmode_url = uri.scheme_str() == Some("https")
        && uri
            .authority()
            .is_some_and(|authority| authority.host() == "github.com")
        && uri.path().starts_with(GITHUB_RELEASE_PATH_PREFIX);
    if is_github_gridmode_url {
        Ok(())
    } else {
        Err("Download URL is not a GridMode GitHub release.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_download_fallback_prefers_the_universal_dmg() {
        let release = GitHubRelease {
            tag_name: "v0.2.0".to_string(),
            html_url: "https://github.com/thedinz/GridMode/releases/tag/v0.2.0".to_string(),
            assets: vec![
                GitHubReleaseAsset {
                    name: "GridMode_0.2.0_x64.dmg".to_string(),
                    browser_download_url: "https://github.com/thedinz/GridMode/releases/download/v0.2.0/GridMode_0.2.0_x64.dmg".to_string(),
                },
                GitHubReleaseAsset {
                    name: "GridMode_0.2.0_universal.dmg".to_string(),
                    browser_download_url: "https://github.com/thedinz/GridMode/releases/download/v0.2.0/GridMode_0.2.0_universal.dmg".to_string(),
                },
            ],
        };
        assert_eq!(
            macos_release_download_url(&release).as_deref(),
            Some("https://github.com/thedinz/GridMode/releases/download/v0.2.0/GridMode_0.2.0_universal.dmg")
        );
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer_version("0.1.31", "0.1.9"));
        assert!(!is_newer_version("0.1.9", "0.1.30"));
        assert_eq!(normalize_version_tag("v0.2.0"), "0.2.0");
    }

    #[test]
    fn only_gridmode_release_urls_may_be_opened() {
        assert!(validate_external_download_url(
            "https://github.com/thedinz/GridMode/releases/download/v1/x.dmg"
        )
        .is_ok());
        assert!(validate_external_download_url("https://github.com/other/repo/releases").is_err());
        assert!(validate_external_download_url("http://github.com/thedinz/GridMode/").is_err());
        assert!(validate_external_download_url("https://evil.com/thedinz/GridMode/").is_err());
    }
}
