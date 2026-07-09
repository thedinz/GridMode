use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Datelike, SecondsFormat, Utc};
use image::{
    imageops::FilterType, io::Reader as ImageReader, ColorType, DynamicImage, GenericImageView,
};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::BufWriter,
    path::{Path, PathBuf},
    sync::{Condvar, Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{http, AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

const LIBRARY_INDEX_VERSION: u32 = 1;
const IMAGE_CACHE_VERSION: &str = "v2-rust-jpeg";
const THUMBNAIL_SIZE: u32 = 420;
const DISPLAY_MAX_DIMENSION: u32 = 3840;
const IMAGE_RENDER_JOB_LIMIT: usize = 1;

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const SUPPORTED_EXTENSIONS: [&str; 13] = [
    "jpg", "jpeg", "jpe", "jfif", "png", "webp", "gif", "bmp", "tif", "tiff", "heic", "heif",
    "avif",
];

const BROWSER_NATIVE_EXTENSIONS: [&str; 9] = [
    "jpg", "jpeg", "jpe", "jfif", "png", "webp", "gif", "bmp", "avif",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PhotoRenderVariant {
    Image,
    Display,
    Thumb,
}

struct ImageRenderQueue {
    state: Mutex<ImageRenderQueueState>,
    available: Condvar,
}

#[derive(Default)]
struct ImageRenderQueueState {
    active: usize,
}

struct ImageRenderPermit<'a> {
    queue: &'a ImageRenderQueue,
}

impl ImageRenderQueue {
    fn acquire(&self) -> ImageRenderPermit<'_> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        while state.active >= IMAGE_RENDER_JOB_LIMIT {
            state = self
                .available
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
        state.active += 1;
        ImageRenderPermit { queue: self }
    }
}

impl Drop for ImageRenderPermit<'_> {
    fn drop(&mut self) {
        let mut state = self
            .queue
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.active = state.active.saturating_sub(1);
        self.queue.available.notify_one();
    }
}

static IMAGE_RENDER_QUEUE: OnceLock<ImageRenderQueue> = OnceLock::new();

struct GridModeState {
    inner: Mutex<StateData>,
}

#[derive(Default)]
struct StateData {
    settings: Settings,
    cached_photos: Vec<PhotoAsset>,
    cached_summary: LibrarySummary,
    cached_photo_stats: HashMap<String, PhotoFileSnapshot>,
    has_library_index: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    #[serde(skip_serializing_if = "Option::is_none")]
    photo_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    photo_directories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    excluded_directories: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_scan_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsPayload {
    settings: Settings,
    summary: LibrarySummary,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PhotoAsset {
    id: String,
    name: String,
    path: String,
    directory: String,
    extension: String,
    size: u64,
    url: String,
    thumbnail_url: String,
    captured_at: String,
    date_source: String,
    year: i32,
    month: u32,
    month_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LibrarySummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    root_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    root_dirs: Vec<String>,
    photo_count: usize,
    years: Vec<YearSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_scan_at: Option<String>,
    warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct YearSummary {
    year: i32,
    count: usize,
    sample: Vec<PhotoAsset>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct MonthSummary {
    year: i32,
    month: u32,
    month_name: String,
    count: usize,
    sample: Vec<PhotoAsset>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HomePayload {
    summary: LibrarySummary,
    photos: Vec<PhotoAsset>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct YearPayload {
    year: i32,
    months: Vec<MonthSummary>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MonthPayload {
    year: i32,
    month: u32,
    month_name: String,
    photos: Vec<PhotoAsset>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExifRow {
    label: String,
    value: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PhotoDetails {
    photo: PhotoAsset,
    exif: Vec<ExifRow>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanProgress {
    phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    root_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    folders_scanned: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    photos_found: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    photos_processed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    photos_reused: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    photos_changed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    photos_removed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    folders_excluded: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_photos: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateStatus {
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    manual_download: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PhotoFileSnapshot {
    path: String,
    size: u64,
    mtime_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryIndexEntry {
    path: String,
    size: u64,
    mtime_ms: i64,
    photo: PhotoAsset,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryIndexFile {
    version: u32,
    #[serde(default)]
    root_dir: Option<String>,
    #[serde(default)]
    root_dirs: Vec<String>,
    #[serde(default)]
    excluded_directories: Vec<String>,
    scanned_at: String,
    #[serde(default)]
    warnings: Vec<String>,
    photos: Vec<LibraryIndexEntry>,
}

struct ProgressEmitter<'a> {
    app: &'a AppHandle,
    root_dir: String,
    last_sent_at: Instant,
}

impl<'a> ProgressEmitter<'a> {
    fn new(app: &'a AppHandle, root_dir: String) -> Self {
        Self {
            app,
            root_dir,
            last_sent_at: Instant::now(),
        }
    }

    fn send(&mut self, mut progress: ScanProgress, immediate: bool) {
        if !immediate && self.last_sent_at.elapsed().as_millis() < 250 {
            return;
        }

        self.last_sent_at = Instant::now();
        progress.root_dir = Some(self.root_dir.clone());
        let _ = self.app.emit("scan:progress", progress);
    }
}

impl StateData {
    fn load(app: &AppHandle) -> Result<Self, String> {
        let data_dir = app_data_dir(app)?;
        let settings = read_settings(&data_dir);
        let root_dirs = get_photo_directories(&settings);
        let excluded = normalize_excluded_directories(&root_dirs, &settings.excluded_directories);
        let mut data = Self {
            settings,
            cached_summary: empty_summary(&root_dirs),
            ..Self::default()
        };
        load_cached_library(&data_dir, &mut data, &root_dirs, &excluded);
        Ok(data)
    }
}

#[tauri::command]
fn settings_get(state: State<'_, GridModeState>) -> Result<SettingsPayload, String> {
    let data = state.inner.lock().map_err(lock_error)?;
    Ok(settings_payload(&data))
}

#[tauri::command]
async fn settings_choose_root(
    app: AppHandle,
    state: State<'_, GridModeState>,
) -> Result<SettingsPayload, String> {
    let selected = app
        .dialog()
        .file()
        .set_title("Choose a photo folder")
        .blocking_pick_folder();

    let Some(selected) = selected else {
        let data = state.inner.lock().map_err(lock_error)?;
        return Ok(settings_payload(&data));
    };

    let selected_root = normalize_path_string(&selected.to_string());
    let mut data = state.inner.lock().map_err(lock_error)?;
    let previous = get_photo_directories(&data.settings);
    let next = if previous.is_empty() {
        vec![selected_root.clone()]
    } else {
        let mut merged = vec![selected_root.clone()];
        merged.extend(
            previous
                .iter()
                .skip(1)
                .filter(|item| !same_directory(item, &selected_root))
                .cloned(),
        );
        normalize_photo_directories(&merged)
    };

    data.settings.photo_directory = next.first().cloned();
    data.settings.photo_directories = next;
    if previous.is_empty() {
        data.settings.excluded_directories = Vec::new();
        reset_library(&mut data);
    }
    normalize_settings_in_place(&mut data.settings);
    write_settings(&app_data_dir(&app)?, &data.settings)?;
    scan_library_locked(&app, &mut data, previous.is_empty())?;
    Ok(settings_payload(&data))
}

#[tauri::command]
async fn settings_add_root(
    app: AppHandle,
    state: State<'_, GridModeState>,
) -> Result<SettingsPayload, String> {
    let selected = app
        .dialog()
        .file()
        .set_title("Add a photo folder")
        .blocking_pick_folder();

    let Some(selected) = selected else {
        let data = state.inner.lock().map_err(lock_error)?;
        return Ok(settings_payload(&data));
    };

    let selected_root = normalize_path_string(&selected.to_string());
    let mut data = state.inner.lock().map_err(lock_error)?;
    let mut next = get_photo_directories(&data.settings);
    next.push(selected_root);
    next = normalize_photo_directories(&next);
    data.settings.photo_directory = next.first().cloned();
    data.settings.photo_directories = next;
    normalize_settings_in_place(&mut data.settings);
    write_settings(&app_data_dir(&app)?, &data.settings)?;
    scan_library_locked(&app, &mut data, false)?;
    Ok(settings_payload(&data))
}

#[tauri::command(rename_all = "camelCase")]
fn settings_remove_root(
    app: AppHandle,
    state: State<'_, GridModeState>,
    root_path: String,
) -> Result<SettingsPayload, String> {
    let mut data = state.inner.lock().map_err(lock_error)?;
    let next: Vec<String> = get_photo_directories(&data.settings)
        .into_iter()
        .filter(|item| !same_directory(item, &root_path))
        .collect();
    data.settings.photo_directory = next.first().cloned();
    data.settings.photo_directories = next;
    normalize_settings_in_place(&mut data.settings);
    write_settings(&app_data_dir(&app)?, &data.settings)?;

    if get_photo_directories(&data.settings).is_empty() {
        reset_library(&mut data);
    } else {
        scan_library_locked(&app, &mut data, false)?;
    }

    Ok(settings_payload(&data))
}

#[tauri::command]
fn settings_clear_cache(
    app: AppHandle,
    state: State<'_, GridModeState>,
) -> Result<SettingsPayload, String> {
    let mut data = state.inner.lock().map_err(lock_error)?;
    clear_library_cache(&app, &mut data)?;
    scan_library_locked(&app, &mut data, true)?;
    Ok(settings_payload(&data))
}

#[tauri::command]
async fn settings_choose_exclusion(
    app: AppHandle,
    state: State<'_, GridModeState>,
) -> Result<SettingsPayload, String> {
    let selected = app
        .dialog()
        .file()
        .set_title("Exclude a folder")
        .blocking_pick_folder();

    let Some(selected) = selected else {
        let data = state.inner.lock().map_err(lock_error)?;
        return Ok(settings_payload(&data));
    };

    let selected_path = normalize_path_string(&selected.to_string());
    let mut data = state.inner.lock().map_err(lock_error)?;
    let root_dirs = get_photo_directories(&data.settings);
    if !is_valid_excluded_directory(&root_dirs, &selected_path) {
        return Ok(settings_payload(&data));
    }

    data.settings.excluded_directories = add_excluded_directory(
        &root_dirs,
        &data.settings.excluded_directories,
        &selected_path,
    );
    write_settings(&app_data_dir(&app)?, &data.settings)?;
    scan_library_locked(&app, &mut data, false)?;
    Ok(settings_payload(&data))
}

#[tauri::command(rename_all = "camelCase")]
fn settings_remove_exclusion(
    app: AppHandle,
    state: State<'_, GridModeState>,
    excluded_path: String,
) -> Result<SettingsPayload, String> {
    let mut data = state.inner.lock().map_err(lock_error)?;
    data.settings.excluded_directories =
        remove_excluded_directory(&data.settings.excluded_directories, &excluded_path);
    write_settings(&app_data_dir(&app)?, &data.settings)?;
    scan_library_locked(&app, &mut data, false)?;
    Ok(settings_payload(&data))
}

#[tauri::command]
fn library_scan(
    app: AppHandle,
    state: State<'_, GridModeState>,
    force: bool,
) -> Result<LibrarySummary, String> {
    let mut data = state.inner.lock().map_err(lock_error)?;
    scan_library_locked(&app, &mut data, force)
}

#[tauri::command]
fn library_get_home(
    app: AppHandle,
    state: State<'_, GridModeState>,
) -> Result<HomePayload, String> {
    let mut data = state.inner.lock().map_err(lock_error)?;
    ensure_library_locked(&app, &mut data)?;
    Ok(HomePayload {
        summary: data.cached_summary.clone(),
        photos: sample_photos(&data.cached_photos, 260),
    })
}

#[tauri::command]
fn library_get_years(
    app: AppHandle,
    state: State<'_, GridModeState>,
) -> Result<LibrarySummary, String> {
    let mut data = state.inner.lock().map_err(lock_error)?;
    ensure_library_locked(&app, &mut data)
}

#[tauri::command]
fn library_get_year(
    app: AppHandle,
    state: State<'_, GridModeState>,
    year: i32,
) -> Result<YearPayload, String> {
    let mut data = state.inner.lock().map_err(lock_error)?;
    ensure_library_locked(&app, &mut data)?;
    let photos: Vec<PhotoAsset> = data
        .cached_photos
        .iter()
        .filter(|photo| photo.year == year)
        .cloned()
        .collect();
    Ok(YearPayload {
        year,
        months: group_months(&photos),
    })
}

#[tauri::command]
fn library_get_month(
    app: AppHandle,
    state: State<'_, GridModeState>,
    year: i32,
    month: u32,
) -> Result<MonthPayload, String> {
    let mut data = state.inner.lock().map_err(lock_error)?;
    ensure_library_locked(&app, &mut data)?;
    let mut photos: Vec<PhotoAsset> = data
        .cached_photos
        .iter()
        .filter(|photo| photo.year == year && photo.month == month)
        .cloned()
        .collect();
    photos.sort_by(|left, right| right.captured_at.cmp(&left.captured_at));
    Ok(MonthPayload {
        year,
        month,
        month_name: month_name(month),
        photos,
    })
}

#[tauri::command(rename_all = "camelCase")]
fn photo_get_details(
    app: AppHandle,
    state: State<'_, GridModeState>,
    photo_path: String,
) -> Result<PhotoDetails, String> {
    let mut data = state.inner.lock().map_err(lock_error)?;
    ensure_library_locked(&app, &mut data)?;
    let photo = data
        .cached_photos
        .iter()
        .find(|item| item.path == photo_path)
        .cloned()
        .ok_or_else(|| "Photo is not part of the current library.".to_string())?;
    Ok(PhotoDetails {
        photo,
        exif: Vec::new(),
    })
}

#[tauri::command]
fn updates_check(app: AppHandle) -> Result<UpdateStatus, String> {
    let status = updates_disabled_status();
    let _ = app.emit("updates:status", status.clone());
    Ok(status)
}

#[tauri::command]
fn updates_download(app: AppHandle) -> Result<UpdateStatus, String> {
    let status = updates_disabled_status();
    let _ = app.emit("updates:status", status.clone());
    Ok(status)
}

#[tauri::command]
fn updates_install() {}

fn ensure_library_locked(app: &AppHandle, data: &mut StateData) -> Result<LibrarySummary, String> {
    let root_dirs = get_photo_directories(&data.settings);
    if root_dirs.is_empty() {
        reset_library(data);
        return Ok(data.cached_summary.clone());
    }

    if !data.has_library_index && data.cached_photos.is_empty() {
        return scan_library_locked(app, data, false);
    }

    Ok(data.cached_summary.clone())
}

fn scan_library_locked(
    app: &AppHandle,
    data: &mut StateData,
    force: bool,
) -> Result<LibrarySummary, String> {
    let root_dirs = get_photo_directories(&data.settings);
    if root_dirs.is_empty() {
        reset_library(data);
        return Ok(data.cached_summary.clone());
    }

    let summary = do_scan(app, data, &root_dirs, force)?;
    data.cached_summary = summary.clone();
    data.settings.last_scan_at = summary.last_scan_at.clone();
    write_settings(&app_data_dir(app)?, &data.settings)?;
    Ok(summary)
}

fn do_scan(
    app: &AppHandle,
    data: &mut StateData,
    root_dirs: &[String],
    force: bool,
) -> Result<LibrarySummary, String> {
    let mut warnings = Vec::new();
    let excluded = normalize_excluded_directories(root_dirs, &data.settings.excluded_directories);
    let root_summary =
        format_root_summary(root_dirs).unwrap_or_else(|| "Photo library".to_string());
    let mut emitter = ProgressEmitter::new(app, root_summary);

    emitter.send(
        ScanProgress {
            phase: "discovering".to_string(),
            folders_scanned: Some(0),
            photos_found: Some(0),
            folders_excluded: Some(0),
            message: Some(if data.has_library_index && !force {
                "Checking folders for changes".to_string()
            } else {
                "Finding photos".to_string()
            }),
            ..empty_progress()
        },
        true,
    );

    let files = find_photo_files(root_dirs, &excluded, &mut warnings, &mut emitter);
    let previous_photos: HashMap<String, PhotoAsset> = if force {
        HashMap::new()
    } else {
        data.cached_photos
            .iter()
            .map(|photo| (photo.path.clone(), photo.clone()))
            .collect()
    };
    let previous_stats: HashMap<String, PhotoFileSnapshot> = if force {
        HashMap::new()
    } else {
        data.cached_photo_stats.clone()
    };
    let found_paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
    let found_lookup: HashMap<String, ()> =
        found_paths.iter().map(|path| (path.clone(), ())).collect();

    let mut files_to_index = Vec::new();
    let mut next_photos = Vec::new();
    let mut next_stats = HashMap::new();
    let mut reused = 0usize;

    for file in &files {
        if let (Some(photo), Some(previous)) = (
            previous_photos.get(&file.path),
            previous_stats.get(&file.path),
        ) {
            if is_unchanged_file(previous, file) {
                next_photos.push(refresh_cached_photo_asset(photo, file));
                next_stats.insert(file.path.clone(), file.clone());
                reused += 1;
                continue;
            }
        }
        files_to_index.push(file.clone());
    }

    let removed = if force {
        0
    } else {
        previous_photos
            .keys()
            .filter(|path| !found_lookup.contains_key(*path))
            .count()
    };

    emitter.send(
        ScanProgress {
            phase: "reading-metadata".to_string(),
            photos_found: Some(files.len()),
            photos_processed: Some(0),
            photos_reused: Some(reused),
            photos_changed: Some(files_to_index.len()),
            photos_removed: Some(removed),
            total_photos: Some(files_to_index.len()),
            message: Some(if files_to_index.is_empty() {
                "No photo metadata changes found".to_string()
            } else {
                "Reading metadata for new and changed photos".to_string()
            }),
            ..empty_progress()
        },
        true,
    );

    let total_to_index = files_to_index.len();
    for (index, file) in files_to_index.iter().enumerate() {
        match build_photo_asset(&file.path, Some(file)) {
            Ok(photo) => {
                next_photos.push(photo);
                next_stats.insert(file.path.clone(), file.clone());
            }
            Err(error) => warnings.push(format!("{}: {}", file.path, error)),
        }

        emitter.send(
            ScanProgress {
                phase: "reading-metadata".to_string(),
                photos_found: Some(files.len()),
                photos_processed: Some(index + 1),
                photos_reused: Some(reused),
                photos_changed: Some(total_to_index),
                photos_removed: Some(removed),
                total_photos: Some(total_to_index),
                current_path: Some(file.path.clone()),
                message: Some("Reading metadata for new and changed photos".to_string()),
                ..empty_progress()
            },
            false,
        );
    }

    next_photos.sort_by(|left, right| right.captured_at.cmp(&left.captured_at));
    data.cached_photos = next_photos;
    data.cached_photo_stats = next_stats;
    data.has_library_index = true;

    let summary = build_summary(root_dirs, &data.cached_photos, &warnings, None);
    data.cached_summary = summary.clone();
    write_library_index(&app_data_dir(app)?, data, root_dirs, &summary)?;

    emitter.send(
        ScanProgress {
            phase: "complete".to_string(),
            photos_found: Some(files.len()),
            photos_processed: Some(total_to_index),
            photos_reused: Some(reused),
            photos_changed: Some(total_to_index),
            photos_removed: Some(removed),
            total_photos: Some(total_to_index),
            message: Some(format_scan_complete_message(
                data.cached_photos.len(),
                reused,
                total_to_index,
                removed,
            )),
            ..empty_progress()
        },
        true,
    );

    Ok(summary)
}

fn find_photo_files(
    root_dirs: &[String],
    excluded_directories: &[String],
    warnings: &mut Vec<String>,
    emitter: &mut ProgressEmitter<'_>,
) -> Vec<PhotoFileSnapshot> {
    let mut found = Vec::new();
    let mut pending: Vec<String> = root_dirs.iter().rev().cloned().collect();
    let mut folders_scanned = 0usize;
    let mut folders_excluded = 0usize;

    while let Some(current) = pending.pop() {
        if !is_configured_root_directory(root_dirs, &current)
            && is_path_excluded(&current, excluded_directories)
        {
            folders_excluded += 1;
            emitter.send(
                ScanProgress {
                    phase: "discovering".to_string(),
                    folders_scanned: Some(folders_scanned),
                    photos_found: Some(found.len()),
                    folders_excluded: Some(folders_excluded),
                    current_path: Some(current),
                    message: Some("Finding photos".to_string()),
                    ..empty_progress()
                },
                false,
            );
            continue;
        }

        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(error) => {
                warnings.push(format!("{}: {}", current, error));
                continue;
            }
        };
        folders_scanned += 1;

        for entry in entries.flatten() {
            let path = entry.path();
            let path_string = path_to_string(&path);
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    warnings.push(format!("{}: {}", path_string, error));
                    continue;
                }
            };

            if file_type.is_dir() {
                if is_path_excluded(&path_string, excluded_directories) {
                    folders_excluded += 1;
                } else {
                    pending.push(path_string);
                }
            } else if file_type.is_file() && is_supported_photo_path(&path) {
                match read_photo_file_snapshot(&path_string) {
                    Ok(snapshot) => found.push(snapshot),
                    Err(error) => warnings.push(format!("{}: {}", path_string, error)),
                }
            }
        }

        emitter.send(
            ScanProgress {
                phase: "discovering".to_string(),
                folders_scanned: Some(folders_scanned),
                photos_found: Some(found.len()),
                folders_excluded: Some(folders_excluded),
                current_path: Some(current),
                message: Some("Finding photos".to_string()),
                ..empty_progress()
            },
            false,
        );
    }

    found
}

fn read_photo_file_snapshot(file_path: &str) -> Result<PhotoFileSnapshot, String> {
    let metadata = fs::metadata(file_path).map_err(|error| error.to_string())?;
    Ok(PhotoFileSnapshot {
        path: file_path.to_string(),
        size: metadata.len(),
        mtime_ms: metadata_modified_ms(&metadata),
    })
}

fn build_photo_asset(
    file_path: &str,
    snapshot: Option<&PhotoFileSnapshot>,
) -> Result<PhotoAsset, String> {
    let stat = match snapshot {
        Some(snapshot) => snapshot.clone(),
        None => read_photo_file_snapshot(file_path)?,
    };
    let captured = datetime_from_mtime_ms(stat.mtime_ms);
    let extension = Path::new(file_path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let name = Path::new(file_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(file_path)
        .to_string();
    let directory = Path::new(file_path)
        .parent()
        .map(path_to_string)
        .unwrap_or_default();
    let month = captured.month();

    Ok(PhotoAsset {
        id: photo_id(file_path),
        name,
        path: file_path.to_string(),
        directory,
        extension,
        size: stat.size,
        url: make_photo_url(file_path, PhotoRenderVariant::Display),
        thumbnail_url: make_photo_url(file_path, PhotoRenderVariant::Thumb),
        captured_at: captured.to_rfc3339_opts(SecondsFormat::Millis, true),
        date_source: "file".to_string(),
        year: captured.year(),
        month,
        month_name: month_name(month),
        width: None,
        height: None,
    })
}

fn is_unchanged_file(previous: &PhotoFileSnapshot, current: &PhotoFileSnapshot) -> bool {
    previous.size == current.size && previous.mtime_ms == current.mtime_ms
}

fn refresh_cached_photo_asset(photo: &PhotoAsset, snapshot: &PhotoFileSnapshot) -> PhotoAsset {
    let mut next = photo.clone();
    next.name = Path::new(&snapshot.path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&snapshot.path)
        .to_string();
    next.path = snapshot.path.clone();
    next.directory = Path::new(&snapshot.path)
        .parent()
        .map(path_to_string)
        .unwrap_or_default();
    next.extension = Path::new(&snapshot.path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_lowercase();
    next.size = snapshot.size;
    next.url = make_photo_url(&snapshot.path, PhotoRenderVariant::Display);
    next.thumbnail_url = make_photo_url(&snapshot.path, PhotoRenderVariant::Thumb);
    next
}

fn build_summary(
    root_dirs: &[String],
    photos: &[PhotoAsset],
    warnings: &[String],
    last_scan_at: Option<String>,
) -> LibrarySummary {
    LibrarySummary {
        root_dir: format_root_summary(root_dirs),
        root_dirs: root_dirs.to_vec(),
        photo_count: photos.len(),
        years: group_years(photos),
        last_scan_at: Some(last_scan_at.unwrap_or_else(now_iso)),
        warnings: warnings.iter().take(20).cloned().collect(),
    }
}

fn empty_summary(root_dirs: &[String]) -> LibrarySummary {
    LibrarySummary {
        root_dir: format_root_summary(root_dirs),
        root_dirs: root_dirs.to_vec(),
        photo_count: 0,
        years: Vec::new(),
        last_scan_at: None,
        warnings: Vec::new(),
    }
}

fn group_years(photos: &[PhotoAsset]) -> Vec<YearSummary> {
    let mut groups: BTreeMap<i32, Vec<PhotoAsset>> = BTreeMap::new();
    for photo in photos {
        groups.entry(photo.year).or_default().push(photo.clone());
    }
    groups
        .into_iter()
        .rev()
        .map(|(year, year_photos)| YearSummary {
            year,
            count: year_photos.len(),
            sample: sample_photos(&year_photos, 20),
        })
        .collect()
}

fn group_months(photos: &[PhotoAsset]) -> Vec<MonthSummary> {
    let mut groups: BTreeMap<u32, Vec<PhotoAsset>> = BTreeMap::new();
    for photo in photos {
        groups.entry(photo.month).or_default().push(photo.clone());
    }
    groups
        .into_iter()
        .rev()
        .map(|(month, month_photos)| MonthSummary {
            year: month_photos
                .first()
                .map(|photo| photo.year)
                .unwrap_or_else(|| Utc::now().year()),
            month,
            month_name: month_name(month),
            count: month_photos.len(),
            sample: sample_photos(&month_photos, 20),
        })
        .collect()
}

fn sample_photos(photos: &[PhotoAsset], count: usize) -> Vec<PhotoAsset> {
    if photos.len() <= count {
        return photos.to_vec();
    }

    let step = (photos.len() as f64 / count as f64).ceil() as usize;
    photos
        .iter()
        .step_by(step.max(1))
        .take(count)
        .cloned()
        .collect()
}

fn format_scan_complete_message(
    total: usize,
    reused: usize,
    changed: usize,
    removed: usize,
) -> String {
    if changed == 0 && removed == 0 && reused > 0 {
        return format!("Library up to date - reused {} cached photos", reused);
    }

    let mut parts = vec![format!("Indexed {} photos", total)];
    if reused > 0 {
        parts.push(format!("{} cached", reused));
    }
    if changed > 0 {
        parts.push(format!("{} new or changed", changed));
    }
    if removed > 0 {
        parts.push(format!("{} removed", removed));
    }
    parts.join(" - ")
}

fn handle_photo_protocol_request(app: &AppHandle, uri_text: &str) -> http::Response<Vec<u8>> {
    let (variant, file_path) = match parse_photo_protocol_request(uri_text) {
        Ok(request) => request,
        Err(error) => return text_response(http::StatusCode::BAD_REQUEST, &error),
    };

    if let Err((status, message)) = validate_photo_request(app, &file_path) {
        return text_response(status, &message);
    }

    match read_photo_response(app, &file_path, variant) {
        Ok((bytes, content_type, immutable)) => binary_response(bytes, &content_type, immutable),
        Err(error) => text_response(http::StatusCode::INTERNAL_SERVER_ERROR, &error),
    }
}

fn parse_photo_protocol_request(uri_text: &str) -> Result<(PhotoRenderVariant, String), String> {
    let uri: http::Uri = uri_text
        .parse()
        .map_err(|error| format!("Unsupported photo URL: {}", error))?;
    let authority = uri
        .authority()
        .map(|value| value.host())
        .unwrap_or_default();
    let path = percent_decode(uri.path().trim_start_matches('/'))?;

    let (variant_token, path_token) = if matches!(authority, "image" | "display" | "thumb") {
        (authority, path.as_str())
    } else {
        let mut parts = path.splitn(2, '/');
        let variant = parts.next().unwrap_or_default();
        let token = parts.next().unwrap_or_default();
        (variant, token)
    };

    let variant = match variant_token {
        "image" => PhotoRenderVariant::Image,
        "display" => PhotoRenderVariant::Display,
        "thumb" => PhotoRenderVariant::Thumb,
        _ => return Err("Unsupported photo request variant.".to_string()),
    };

    if path_token.is_empty() {
        return Err("Photo request did not include a file path.".to_string());
    }

    Ok((variant, decode_path_token(path_token)?))
}

fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes
                .get(index + 1)
                .copied()
                .and_then(hex_value)
                .ok_or_else(|| "Photo URL contained an invalid percent escape.".to_string())?;
            let low = bytes
                .get(index + 2)
                .copied()
                .and_then(hex_value)
                .ok_or_else(|| "Photo URL contained an invalid percent escape.".to_string())?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    String::from_utf8(decoded).map_err(|error| format!("Photo URL path was not UTF-8: {}", error))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn validate_photo_request(
    app: &AppHandle,
    file_path: &str,
) -> Result<(), (http::StatusCode, String)> {
    let state = app.state::<GridModeState>();
    let data = state.inner.lock().map_err(|_| {
        (
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "GridMode state lock was poisoned.".to_string(),
        )
    })?;
    let root_dirs = get_photo_directories(&data.settings);
    if root_dirs.is_empty() || !is_inside_any_directory(&root_dirs, file_path) {
        return Err((
            http::StatusCode::FORBIDDEN,
            "Photo is outside the configured library.".to_string(),
        ));
    }

    if is_path_excluded(file_path, &data.settings.excluded_directories) {
        return Err((
            http::StatusCode::FORBIDDEN,
            "Photo is in an excluded folder.".to_string(),
        ));
    }

    if !is_supported_photo_path(Path::new(file_path)) {
        return Err((
            http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Photo format is not supported.".to_string(),
        ));
    }

    Ok(())
}

fn read_photo_response(
    app: &AppHandle,
    file_path: &str,
    variant: PhotoRenderVariant,
) -> Result<(Vec<u8>, String, bool), String> {
    if variant == PhotoRenderVariant::Thumb {
        let cached_path = get_cached_render_path(app, file_path, variant)?;
        let bytes = fs::read(cached_path).map_err(|error| error.to_string())?;
        return Ok((bytes, "image/jpeg".to_string(), true));
    }

    if variant == PhotoRenderVariant::Display && needs_rendered_display(file_path) {
        let cached_path = get_cached_render_path(app, file_path, variant)?;
        let bytes = fs::read(cached_path).map_err(|error| error.to_string())?;
        return Ok((bytes, "image/jpeg".to_string(), true));
    }

    let bytes = fs::read(file_path).map_err(|error| error.to_string())?;
    Ok((bytes, mime_type_for_path(file_path).to_string(), false))
}

fn get_cached_render_path(
    app: &AppHandle,
    file_path: &str,
    variant: PhotoRenderVariant,
) -> Result<PathBuf, String> {
    let metadata = fs::metadata(file_path).map_err(|error| error.to_string())?;
    let data_dir = app_data_dir(app)?;
    let output_path = image_cache_file_path(&data_dir, file_path, &metadata, variant);

    if output_path.exists() {
        return Ok(output_path);
    }

    let _permit = image_render_queue().acquire();
    if output_path.exists() {
        return Ok(output_path);
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let temp_path = temp_image_cache_file_path(&output_path);
    render_image(file_path, &temp_path, variant).inspect_err(|_| {
        let _ = fs::remove_file(&temp_path);
    })?;
    fs::rename(&temp_path, &output_path).map_err(|error| error.to_string())?;
    Ok(output_path)
}

fn image_render_queue() -> &'static ImageRenderQueue {
    IMAGE_RENDER_QUEUE.get_or_init(|| ImageRenderQueue {
        state: Mutex::new(ImageRenderQueueState::default()),
        available: Condvar::new(),
    })
}

fn image_cache_file_path(
    data_dir: &Path,
    file_path: &str,
    metadata: &fs::Metadata,
    variant: PhotoRenderVariant,
) -> PathBuf {
    let mut hasher = Sha1::new();
    hasher.update(IMAGE_CACHE_VERSION.as_bytes());
    hasher.update([0]);
    hasher.update(photo_render_variant_token(variant).as_bytes());
    hasher.update([0]);
    hasher.update(file_path.as_bytes());
    hasher.update([0]);
    hasher.update(metadata.len().to_string().as_bytes());
    hasher.update([0]);
    hasher.update(metadata_modified_ms(metadata).to_string().as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    image_cache_path(data_dir)
        .join(photo_render_variant_token(variant))
        .join(&hash[..2])
        .join(format!("{}.jpg", hash))
}

fn temp_image_cache_file_path(output_path: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let file_name = output_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("render.jpg");
    output_path.with_file_name(format!("{}.{}.tmp", file_name, stamp))
}

fn render_image(
    file_path: &str,
    output_path: &Path,
    variant: PhotoRenderVariant,
) -> Result<(), String> {
    let image = ImageReader::open(file_path)
        .map_err(|error| error.to_string())?
        .with_guessed_format()
        .map_err(|error| error.to_string())?
        .decode()
        .map_err(|error| error.to_string())?;
    let rendered = match variant {
        PhotoRenderVariant::Thumb => resize_to_cover(image, THUMBNAIL_SIZE),
        PhotoRenderVariant::Display | PhotoRenderVariant::Image => {
            resize_inside(image, DISPLAY_MAX_DIMENSION)
        }
    };
    write_jpeg(output_path, &rendered, jpeg_quality_for_variant(variant))
}

fn resize_to_cover(image: DynamicImage, size: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    if width <= size && height <= size {
        return image;
    }

    let scale = (size as f64 / width as f64).max(size as f64 / height as f64);
    let resized_width = ((width as f64 * scale).round() as u32).max(1);
    let resized_height = ((height as f64 * scale).round() as u32).max(1);
    let resized = image.resize(resized_width, resized_height, FilterType::Lanczos3);
    let crop_width = resized_width.min(size);
    let crop_height = resized_height.min(size);
    let x = resized_width.saturating_sub(crop_width) / 2;
    let y = resized_height.saturating_sub(crop_height) / 2;
    resized.crop_imm(x, y, crop_width, crop_height)
}

fn resize_inside(image: DynamicImage, max_dimension: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    if width <= max_dimension && height <= max_dimension {
        return image;
    }
    image.resize(max_dimension, max_dimension, FilterType::Lanczos3)
}

fn write_jpeg(output_path: &Path, image: &DynamicImage, quality: u8) -> Result<(), String> {
    let file = fs::File::create(output_path).map_err(|error| error.to_string())?;
    let mut writer = BufWriter::new(file);
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, quality);
    encoder
        .encode(&rgb, width, height, ColorType::Rgb8)
        .map_err(|error| error.to_string())
}

fn binary_response(bytes: Vec<u8>, content_type: &str, immutable: bool) -> http::Response<Vec<u8>> {
    let cache_control = if immutable {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    http::Response::builder()
        .status(http::StatusCode::OK)
        .header(http::header::CONTENT_TYPE, content_type)
        .header(http::header::CACHE_CONTROL, cache_control)
        .body(bytes)
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

fn text_response(status: http::StatusCode, message: &str) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(message.as_bytes().to_vec())
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

fn needs_rendered_display(file_path: &str) -> bool {
    Path::new(file_path)
        .extension()
        .and_then(|value| value.to_str())
        .map(|extension| {
            let normalized = extension.to_lowercase();
            !BROWSER_NATIVE_EXTENSIONS.contains(&normalized.as_str())
        })
        .unwrap_or(true)
}

fn mime_type_for_path(file_path: &str) -> &'static str {
    match Path::new(file_path)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg" | "jpe" | "jfif") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("tif" | "tiff") => "image/tiff",
        Some("heic") => "image/heic",
        Some("heif") => "image/heif",
        Some("avif") => "image/avif",
        _ => "application/octet-stream",
    }
}

fn make_photo_url(file_path: &str, variant: PhotoRenderVariant) -> String {
    format!(
        "gridmode-photo://{}/{}",
        photo_render_variant_token(variant),
        encode_path_token(file_path)
    )
}

fn photo_render_variant_token(variant: PhotoRenderVariant) -> &'static str {
    match variant {
        PhotoRenderVariant::Image => "image",
        PhotoRenderVariant::Display => "display",
        PhotoRenderVariant::Thumb => "thumb",
    }
}

fn jpeg_quality_for_variant(variant: PhotoRenderVariant) -> u8 {
    match variant {
        PhotoRenderVariant::Thumb => 78,
        PhotoRenderVariant::Display | PhotoRenderVariant::Image => 88,
    }
}

fn encode_path_token(file_path: &str) -> String {
    URL_SAFE_NO_PAD.encode(file_path.as_bytes())
}

fn decode_path_token(token: &str) -> Result<String, String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|error| format!("Photo path token could not be decoded: {}", error))?;
    String::from_utf8(bytes).map_err(|error| format!("Photo path was not UTF-8: {}", error))
}

fn clear_library_cache(app: &AppHandle, data: &mut StateData) -> Result<(), String> {
    reset_library(data);
    let data_dir = app_data_dir(app)?;
    let _ = fs::remove_dir_all(image_cache_path(&data_dir));
    let _ = fs::remove_file(library_index_path(&data_dir));
    Ok(())
}

fn reset_library(data: &mut StateData) {
    let root_dirs = get_photo_directories(&data.settings);
    data.cached_photos = Vec::new();
    data.cached_photo_stats = HashMap::new();
    data.cached_summary = empty_summary(&root_dirs);
    data.has_library_index = false;
}

fn load_cached_library(
    data_dir: &Path,
    data: &mut StateData,
    root_dirs: &[String],
    excluded_directories: &[String],
) {
    if root_dirs.is_empty() {
        data.cached_summary = empty_summary(root_dirs);
        return;
    }

    let Ok(text) = fs::read_to_string(library_index_path(data_dir)) else {
        data.cached_summary = empty_summary(root_dirs);
        return;
    };
    let Ok(index) = serde_json::from_str::<LibraryIndexFile>(&text) else {
        data.cached_summary = empty_summary(root_dirs);
        return;
    };
    if !is_usable_library_index(&index, root_dirs, excluded_directories) {
        data.cached_summary = empty_summary(root_dirs);
        return;
    }

    let entries: Vec<LibraryIndexEntry> = index
        .photos
        .into_iter()
        .filter(|entry| is_usable_library_index_entry(entry, root_dirs))
        .collect();
    data.cached_photos = entries
        .iter()
        .map(|entry| refresh_cached_photo_asset(&entry.photo, &entry_snapshot(entry)))
        .collect();
    data.cached_photos
        .sort_by(|left, right| right.captured_at.cmp(&left.captured_at));
    data.cached_photo_stats = entries
        .iter()
        .map(|entry| (entry.path.clone(), entry_snapshot(entry)))
        .collect();
    data.cached_summary = build_summary(
        root_dirs,
        &data.cached_photos,
        &index.warnings,
        Some(index.scanned_at),
    );
    data.has_library_index = true;
}

fn write_library_index(
    data_dir: &Path,
    data: &StateData,
    root_dirs: &[String],
    summary: &LibrarySummary,
) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    let entries: Vec<LibraryIndexEntry> = data
        .cached_photos
        .iter()
        .filter_map(|photo| {
            data.cached_photo_stats
                .get(&photo.path)
                .map(|stat| LibraryIndexEntry {
                    path: photo.path.clone(),
                    size: stat.size,
                    mtime_ms: stat.mtime_ms,
                    photo: photo.clone(),
                })
        })
        .collect();
    let index = LibraryIndexFile {
        version: LIBRARY_INDEX_VERSION,
        root_dir: root_dirs.first().cloned(),
        root_dirs: root_dirs.to_vec(),
        excluded_directories: normalize_excluded_directories(
            root_dirs,
            &data.settings.excluded_directories,
        ),
        scanned_at: summary.last_scan_at.clone().unwrap_or_else(now_iso),
        warnings: summary.warnings.clone(),
        photos: entries,
    };
    let target = library_index_path(data_dir);
    let temp = target.with_extension("json.tmp");
    let text = serde_json::to_string(&index).map_err(|error| error.to_string())?;
    fs::write(&temp, text).map_err(|error| error.to_string())?;
    fs::rename(temp, target).map_err(|error| error.to_string())?;
    Ok(())
}

fn is_usable_library_index(
    index: &LibraryIndexFile,
    root_dirs: &[String],
    excluded_directories: &[String],
) -> bool {
    let mut index_root_dirs = Vec::new();
    if let Some(root_dir) = &index.root_dir {
        index_root_dirs.push(root_dir.clone());
    }
    index_root_dirs.extend(index.root_dirs.clone());
    let index_root_dirs = normalize_photo_directories(&index_root_dirs);

    index.version == LIBRARY_INDEX_VERSION
        && same_directories(&index_root_dirs, root_dirs)
        && same_directories(
            &normalize_excluded_directories(root_dirs, &index.excluded_directories),
            &normalize_excluded_directories(root_dirs, excluded_directories),
        )
}

fn is_usable_library_index_entry(entry: &LibraryIndexEntry, root_dirs: &[String]) -> bool {
    is_supported_photo_path(Path::new(&entry.path))
        && is_inside_any_directory(root_dirs, &entry.path)
}

fn entry_snapshot(entry: &LibraryIndexEntry) -> PhotoFileSnapshot {
    PhotoFileSnapshot {
        path: entry.path.clone(),
        size: entry.size,
        mtime_ms: entry.mtime_ms,
    }
}

fn settings_payload(data: &StateData) -> SettingsPayload {
    SettingsPayload {
        settings: data.settings.clone(),
        summary: data.cached_summary.clone(),
    }
}

fn read_settings(data_dir: &Path) -> Settings {
    let Ok(text) = fs::read_to_string(settings_path(data_dir)) else {
        return Settings::default();
    };
    let cleaned = text.trim_start_matches('\u{feff}');
    let mut settings = serde_json::from_str::<Settings>(cleaned).unwrap_or_default();
    normalize_settings_in_place(&mut settings);
    settings
}

fn write_settings(data_dir: &Path, settings: &Settings) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    let mut normalized = settings.clone();
    normalize_settings_in_place(&mut normalized);
    let text = serde_json::to_string_pretty(&normalized).map_err(|error| error.to_string())?;
    fs::write(settings_path(data_dir), text).map_err(|error| error.to_string())
}

fn normalize_settings_in_place(settings: &mut Settings) {
    let mut dirs = Vec::new();
    if let Some(photo_directory) = &settings.photo_directory {
        dirs.push(photo_directory.clone());
    }
    dirs.extend(settings.photo_directories.clone());
    let photo_directories = normalize_photo_directories(&dirs);
    settings.photo_directory = photo_directories.first().cloned();
    settings.photo_directories = photo_directories.clone();
    settings.excluded_directories =
        normalize_excluded_directories(&photo_directories, &settings.excluded_directories);
}

fn normalize_photo_directories(directories: &[String]) -> Vec<String> {
    let normalized: Vec<String> = directories
        .iter()
        .filter(|item| !item.trim().is_empty())
        .map(|item| normalize_path_string(item))
        .collect();

    let mut compacted: Vec<String> = Vec::new();
    for item in normalized {
        if compacted
            .iter()
            .any(|existing| same_directory(existing, &item) || is_inside_directory(existing, &item))
        {
            continue;
        }

        compacted.retain(|existing| !is_inside_directory(&item, existing));
        compacted.push(item);
    }
    compacted
}

fn get_photo_directories(settings: &Settings) -> Vec<String> {
    let mut dirs = Vec::new();
    if let Some(photo_directory) = &settings.photo_directory {
        dirs.push(photo_directory.clone());
    }
    dirs.extend(settings.photo_directories.clone());
    normalize_photo_directories(&dirs)
}

fn normalize_excluded_directories(root_dirs: &[String], directories: &[String]) -> Vec<String> {
    if root_dirs.is_empty() {
        return Vec::new();
    }

    let mut normalized: Vec<String> = directories
        .iter()
        .filter(|item| !item.trim().is_empty())
        .map(|item| normalize_path_string(item))
        .filter(|item| is_valid_excluded_directory(root_dirs, item))
        .collect();
    normalized.sort_by(|left, right| left.len().cmp(&right.len()).then_with(|| left.cmp(right)));

    let mut compacted: Vec<String> = Vec::new();
    for item in normalized {
        if !compacted
            .iter()
            .any(|existing| same_directory(existing, &item) || is_inside_directory(existing, &item))
        {
            compacted.push(item);
        }
    }
    compacted
}

fn add_excluded_directory(
    root_dirs: &[String],
    existing: &[String],
    candidate: &str,
) -> Vec<String> {
    let mut next = existing.to_vec();
    next.push(candidate.to_string());
    normalize_excluded_directories(root_dirs, &next)
}

fn remove_excluded_directory(existing: &[String], candidate: &str) -> Vec<String> {
    let normalized_candidate = normalize_path_string(candidate);
    existing
        .iter()
        .filter(|item| !same_directory(&normalize_path_string(item), &normalized_candidate))
        .cloned()
        .collect()
}

fn is_path_excluded(file_path: &str, excluded_directories: &[String]) -> bool {
    excluded_directories
        .iter()
        .any(|excluded| is_inside_directory(excluded, file_path))
}

fn is_valid_excluded_directory(root_dirs: &[String], candidate: &str) -> bool {
    is_inside_any_directory(root_dirs, candidate)
        && !is_configured_root_directory(root_dirs, candidate)
}

fn is_configured_root_directory(root_dirs: &[String], candidate: &str) -> bool {
    root_dirs.iter().any(|root| same_directory(root, candidate))
}

fn is_inside_any_directory(root_dirs: &[String], file_path: &str) -> bool {
    root_dirs
        .iter()
        .any(|root_dir| is_inside_directory(root_dir, file_path))
}

fn is_inside_directory(root_dir: &str, file_path: &str) -> bool {
    let root = normalize_path_buf(root_dir);
    let file = normalize_path_buf(file_path);
    file == root || file.starts_with(root)
}

fn same_directory(left: &str, right: &str) -> bool {
    let left = normalize_path_string(left);
    let right = normalize_path_string(right);
    if cfg!(target_os = "windows") {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

fn same_directories(left: &[String], right: &[String]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|(left, right)| same_directory(left, right))
}

fn normalize_path_string(path: &str) -> String {
    path_to_string(&normalize_path_buf(path))
}

fn normalize_path_buf(path: &str) -> PathBuf {
    let raw = PathBuf::from(path);
    raw.canonicalize().unwrap_or(raw)
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn format_root_summary(root_dirs: &[String]) -> Option<String> {
    match root_dirs.len() {
        0 => None,
        1 => root_dirs.first().cloned(),
        count => Some(format!("{} photo locations", count)),
    }
}

fn is_supported_photo_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|extension| {
            let normalized = extension.to_lowercase();
            SUPPORTED_EXTENSIONS.contains(&normalized.as_str())
        })
        .unwrap_or(false)
}

fn metadata_modified_ms(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn datetime_from_mtime_ms(mtime_ms: i64) -> DateTime<Utc> {
    let seconds = mtime_ms.div_euclid(1000);
    let nanos = (mtime_ms.rem_euclid(1000) * 1_000_000) as u32;
    DateTime::<Utc>::from_timestamp(seconds, nanos).unwrap_or_else(Utc::now)
}

fn now_iso() -> String {
    DateTime::<Utc>::from(SystemTime::now()).to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn month_name(month: u32) -> String {
    MONTH_NAMES
        .get(month.saturating_sub(1) as usize)
        .unwrap_or(&"Unknown")
        .to_string()
}

fn photo_id(file_path: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(file_path.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    digest.chars().take(16).collect()
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|error| error.to_string())
}

fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

fn library_index_path(data_dir: &Path) -> PathBuf {
    data_dir.join("library-index.json")
}

fn image_cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join("image-cache")
}

fn empty_progress() -> ScanProgress {
    ScanProgress {
        phase: "idle".to_string(),
        root_dir: None,
        folders_scanned: None,
        photos_found: None,
        photos_processed: None,
        photos_reused: None,
        photos_changed: None,
        photos_removed: None,
        folders_excluded: None,
        total_photos: None,
        current_path: None,
        message: None,
    }
}

fn updates_disabled_status() -> UpdateStatus {
    UpdateStatus {
        state: "idle".to_string(),
        version: None,
        message: Some("Automatic updates are not configured in this build yet.".to_string()),
        percent: None,
        download_url: None,
        manual_download: None,
    }
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> String {
    "GridMode state lock was poisoned.".to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("gridmode-photo", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let uri = request.uri().to_string();
            std::thread::spawn(move || {
                responder.respond(handle_photo_protocol_request(&app, &uri));
            });
        })
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let data = StateData::load(app.handle()).map_err(std::io::Error::other)?;
            app.manage(GridModeState {
                inner: Mutex::new(data),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            settings_get,
            settings_choose_root,
            settings_add_root,
            settings_remove_root,
            settings_clear_cache,
            settings_choose_exclusion,
            settings_remove_exclusion,
            library_scan,
            library_get_home,
            library_get_years,
            library_get_year,
            library_get_month,
            photo_get_details,
            updates_check,
            updates_download,
            updates_install
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
