//! Data shared with the renderer. `cargo test` exports these types to
//! `src/shared/generated/bindings.ts`, so the TypeScript side cannot drift.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub photo_directory: Option<String>,
    #[serde(default)]
    pub photo_directories: Vec<String>,
    #[serde(default)]
    pub excluded_directories: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_scan_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct SettingsPayload {
    pub settings: Settings,
    pub summary: LibrarySummary,
}

#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct ThumbnailCacheSummary {
    pub total: usize,
    pub generated: usize,
    pub reused: usize,
    pub failed: usize,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct ThumbnailRebuildPayload {
    pub settings: Settings,
    pub summary: LibrarySummary,
    pub thumbnails: ThumbnailCacheSummary,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct PhotoLocation {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "bindings.ts")]
pub enum DateSource {
    Exif,
    File,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct PhotoAsset {
    pub id: String,
    pub name: String,
    pub path: String,
    pub directory: String,
    pub extension: String,
    #[ts(type = "number")]
    pub size: u64,
    #[serde(default)]
    pub cache_key: String,
    /// Protocol URLs are derived from the path and rebuilt on load, never
    /// trusted from disk; they are blanked before the library index is written.
    #[serde(default, skip_deserializing, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_deserializing, skip_serializing_if = "String::is_empty")]
    pub thumbnail_url: String,
    /// Local wall-clock time without an offset, e.g. `2024-05-01T14:03:00.000`.
    pub captured_at: String,
    pub date_source: DateSource,
    pub year: i32,
    pub month: u32,
    pub month_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub camera: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub location: Option<PhotoLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub height: Option<u32>,
}

#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct LibrarySummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub root_dir: Option<String>,
    pub root_dirs: Vec<String>,
    pub photo_count: usize,
    pub years: Vec<YearSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_scan_at: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct YearSummary {
    pub year: i32,
    pub count: usize,
    pub sample: Vec<PhotoAsset>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct MonthSummary {
    pub year: i32,
    pub month: u32,
    pub month_name: String,
    pub count: usize,
    pub sample: Vec<PhotoAsset>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct HomePayload {
    pub summary: LibrarySummary,
    pub photos: Vec<PhotoAsset>,
    /// Photos taken on today's month and day in earlier years.
    pub on_this_day: Vec<PhotoAsset>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct YearPayload {
    pub year: i32,
    pub months: Vec<MonthSummary>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct MonthPayload {
    pub year: i32,
    pub month: u32,
    pub month_name: String,
    pub photos: Vec<PhotoAsset>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct DirectoryBreadcrumb {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct FolderSummary {
    pub name: String,
    pub path: String,
    pub photo_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cover: Option<PhotoAsset>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct FoldersPayload {
    pub roots: Vec<FolderSummary>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct DirectoryPayload {
    pub path: String,
    pub name: String,
    pub photo_count: usize,
    pub breadcrumbs: Vec<DirectoryBreadcrumb>,
    pub subfolders: Vec<FolderSummary>,
    pub photos: Vec<PhotoAsset>,
}

#[derive(Clone, Debug, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct SearchQuery {
    #[serde(default)]
    #[ts(optional)]
    pub text: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub camera: Option<String>,
    /// Inclusive `YYYY-MM-DD` bounds on the capture date.
    #[serde(default)]
    #[ts(optional)]
    pub from: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub to: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct SearchPayload {
    pub photos: Vec<PhotoAsset>,
    pub total: usize,
    pub truncated: bool,
    pub cameras: Vec<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct ExifRow {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct PhotoDetails {
    pub photo: PhotoAsset,
    pub exif: Vec<ExifRow>,
    pub directory_breadcrumbs: Vec<DirectoryBreadcrumb>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export, export_to = "bindings.ts")]
pub enum ScanPhase {
    Discovering,
    ReadingMetadata,
    GeneratingThumbnails,
    Complete,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct ScanProgress {
    pub phase: ScanPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub root_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub folders_scanned: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub photos_found: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub photos_processed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub photos_reused: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub photos_changed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub photos_removed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub thumbnails_generated: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub thumbnails_reused: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub thumbnail_failures: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub folders_excluded: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub total_photos: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub current_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message: Option<String>,
}

impl ScanProgress {
    pub fn new(phase: ScanPhase) -> Self {
        Self {
            phase,
            root_dir: None,
            folders_scanned: None,
            photos_found: None,
            photos_processed: None,
            photos_reused: None,
            photos_changed: None,
            photos_removed: None,
            thumbnails_generated: None,
            thumbnails_reused: None,
            thumbnail_failures: None,
            folders_excluded: None,
            total_photos: None,
            current_path: None,
            message: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export, export_to = "bindings.ts")]
pub enum UpdateState {
    Idle,
    Checking,
    Available,
    NotAvailable,
    Downloading,
    Downloaded,
    Error,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "bindings.ts")]
pub struct UpdateStatus {
    pub state: UpdateState,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub manual_download: Option<bool>,
}

impl UpdateStatus {
    pub fn new(state: UpdateState, message: Option<&str>) -> Self {
        Self {
            state,
            version: None,
            message: message.map(str::to_string),
            percent: None,
            download_url: None,
            manual_download: None,
        }
    }
}
