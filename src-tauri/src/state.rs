//! Application state and scan orchestration.
//!
//! Locking model:
//! - `data` guards settings and the current `Library` snapshot. It is only
//!   held long enough to clone or swap them, so commands and the photo
//!   protocol never wait on a scan.
//! - `scan_lock` serializes scans and every settings change that affects
//!   what a scan sees. It is held for a scan's full duration, but nothing
//!   else takes it.
//! - `index_write_lock` keeps library-index.json writes from interleaving.

use crate::library::{
    load_library, merge_thumbnail_warnings, scan_library, write_library_index, Library,
    ProgressEmitter, ScanOutcome,
};
use crate::model::{
    PhotoAsset, ScanPhase, ScanProgress, Settings, SettingsPayload, ThumbnailCacheSummary,
};
use crate::render::{ensure_cached_render, image_render_job_limit, PhotoRenderVariant};
use crate::settings::{get_photo_directories, read_settings, root_summary_label, write_settings};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard,
    },
};
use tauri::{AppHandle, Emitter, Manager};

pub struct StateData {
    pub settings: Settings,
    pub library: Library,
}

pub struct AppState {
    data: Mutex<StateData>,
    scan_lock: Mutex<()>,
    index_write_lock: Mutex<()>,
    thumbnail_build_active: AtomicBool,
    pub data_dir: PathBuf,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while holding one of these locks leaves plain data behind, so
    // recovering the guard is always safe.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    /// User-visible scan with progress events and a full thumbnail check.
    Interactive,
    /// Folder-watcher rescan: no progress events, thumbnails only for new photos.
    Background,
}

impl AppState {
    pub fn load(data_dir: PathBuf) -> Self {
        let settings = read_settings(&data_dir);
        let root_dirs = get_photo_directories(&settings);
        let library = load_library(&data_dir, &root_dirs, &settings.excluded_directories);
        Self {
            data: Mutex::new(StateData { settings, library }),
            scan_lock: Mutex::new(()),
            index_write_lock: Mutex::new(()),
            thumbnail_build_active: AtomicBool::new(false),
            data_dir,
        }
    }

    pub fn settings(&self) -> Settings {
        lock(&self.data).settings.clone()
    }

    pub fn snapshot(&self) -> (Settings, Library) {
        let data = lock(&self.data);
        (data.settings.clone(), data.library.clone())
    }

    /// Looks up one indexed photo without cloning the rest of the library.
    pub fn find_photo(&self, path: &str) -> Option<PhotoAsset> {
        lock(&self.data).library.photo(path).cloned()
    }

    pub fn update<R>(&self, change: impl FnOnce(&mut StateData) -> R) -> R {
        change(&mut lock(&self.data))
    }

    pub fn settings_payload(&self) -> SettingsPayload {
        let data = lock(&self.data);
        SettingsPayload {
            settings: data.settings.clone(),
            summary: data.library.summary.clone(),
        }
    }

    pub fn persist_index(&self) -> Result<(), String> {
        let _writing = lock(&self.index_write_lock);
        let (settings, library) = self.snapshot();
        if !library.has_index {
            return Ok(());
        }
        let root_dirs = get_photo_directories(&settings);
        write_library_index(
            &self.data_dir,
            &library,
            &root_dirs,
            &settings.excluded_directories,
        )
    }

    pub fn is_data_path(&self, path: &Path) -> bool {
        path.starts_with(&self.data_dir)
    }

    fn try_begin_thumbnail_build(&self) -> bool {
        !self.thumbnail_build_active.swap(true, Ordering::AcqRel)
    }

    fn finish_thumbnail_build(&self) {
        self.thumbnail_build_active.store(false, Ordering::Release);
    }
}

/// Runs a scan, waiting for any scan already in progress.
pub fn run_scan(app: &AppHandle, force: bool, mode: ScanMode) -> Result<ScanOutcome, String> {
    let state = app.state::<AppState>();
    let _scanning = lock(&state.scan_lock);
    scan_while_locked(app, &state, force, mode)
}

fn scan_while_locked(
    app: &AppHandle,
    state: &AppState,
    force: bool,
    mode: ScanMode,
) -> Result<ScanOutcome, String> {
    let (settings, previous) = state.snapshot();
    let root_dirs = get_photo_directories(&settings);
    if root_dirs.is_empty() {
        let library = Library::empty(&root_dirs);
        state.update(|data| data.library = library.clone());
        return Ok(ScanOutcome {
            library,
            changed: 0,
            removed: 0,
        });
    }

    let label = root_summary_label(&root_dirs);
    let emitter = ProgressEmitter::new(
        Some(app.clone()),
        label.clone(),
        mode == ScanMode::Background,
    );
    let ignored = vec![crate::paths::path_to_string(&state.data_dir)];
    let outcome = scan_library(
        &root_dirs,
        &settings.excluded_directories,
        &ignored,
        &previous,
        force,
        &emitter,
    );

    let last_scan_at = outcome.library.summary.last_scan_at.clone();
    let settings = state.update(|data| {
        data.library = outcome.library.clone();
        data.settings.last_scan_at = last_scan_at;
        data.settings.clone()
    });
    write_settings(&state.data_dir, &settings)?;
    state.persist_index()?;

    let to_prebuild = match mode {
        ScanMode::Interactive => outcome.library.photos.clone(),
        ScanMode::Background if outcome.changed > 0 => {
            // Only newly indexed photos can be missing thumbnails.
            let known: std::collections::HashSet<&str> = previous
                .photos
                .iter()
                .map(|photo| photo.path.as_str())
                .collect();
            Arc::new(
                outcome
                    .library
                    .photos
                    .iter()
                    .filter(|photo| !known.contains(photo.path.as_str()))
                    .cloned()
                    .collect(),
            )
        }
        ScanMode::Background => Arc::new(Vec::new()),
    };
    schedule_thumbnail_prebuild(app, to_prebuild, label, mode == ScanMode::Background);
    Ok(outcome)
}

/// Returns the current library, running the first scan if nothing is indexed yet.
pub fn ensure_library(app: &AppHandle) -> Result<(Settings, Library), String> {
    let state = app.state::<AppState>();
    let (settings, library) = state.snapshot();
    if library.has_index || get_photo_directories(&settings).is_empty() {
        return Ok((settings, library));
    }

    let _scanning = lock(&state.scan_lock);
    // Another caller may have finished the first scan while we waited.
    let (settings, library) = state.snapshot();
    if library.has_index {
        return Ok((settings, library));
    }
    scan_while_locked(app, &state, false, ScanMode::Interactive)?;
    Ok(state.snapshot())
}

/// Applies a settings change and rescans, holding the scan lock throughout so
/// no scan can observe a half-applied change. `change` returns true when the
/// existing index no longer applies (the primary root was replaced).
pub fn change_settings_and_rescan(
    app: &AppHandle,
    change: impl FnOnce(&mut Settings) -> bool,
) -> Result<SettingsPayload, String> {
    let state = app.state::<AppState>();
    let _scanning = lock(&state.scan_lock);
    let (settings, reset) = state.update(|data| {
        let reset = change(&mut data.settings);
        crate::settings::normalize_settings_in_place(&mut data.settings);
        let root_dirs = get_photo_directories(&data.settings);
        if reset || root_dirs.is_empty() {
            data.library = Library::empty(&root_dirs);
        }
        (data.settings.clone(), reset)
    });
    write_settings(&state.data_dir, &settings)?;

    if !get_photo_directories(&settings).is_empty() {
        scan_while_locked(app, &state, reset, ScanMode::Interactive)?;
    }
    crate::watcher::refresh(app);
    Ok(state.settings_payload())
}

// ---------------------------------------------------------------------------
// Thumbnail cache

fn schedule_thumbnail_prebuild(
    app: &AppHandle,
    photos: Arc<Vec<PhotoAsset>>,
    label: String,
    quiet: bool,
) {
    if photos.is_empty() || !app.state::<AppState>().try_begin_thumbnail_build() {
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let build = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (thumbnails, warnings) =
                pregenerate_thumbnails(&app, &state.data_dir, &photos, &label, quiet);
            store_thumbnail_warnings(&state, warnings);
            if !quiet {
                emit_thumbnail_complete(&app, &label, &thumbnails);
            }
        }));
        if build.is_err() {
            log::error!("Thumbnail prebuild stopped unexpectedly.");
        }
        state.finish_thumbnail_build();
    });
}

fn store_thumbnail_warnings(state: &AppState, warnings: Vec<String>) {
    let changed = state.update(|data| {
        let had_warnings = data
            .library
            .summary
            .warnings
            .iter()
            .any(|warning| warning.starts_with("Thumbnail "));
        let has_warnings = !warnings.is_empty();
        merge_thumbnail_warnings(&mut data.library.summary, warnings);
        had_warnings || has_warnings
    });
    if changed {
        if let Err(error) = state.persist_index() {
            log::warn!("Could not persist thumbnail warnings: {error}");
        }
    }
}

pub fn clear_image_cache(app: &AppHandle) -> Result<SettingsPayload, String> {
    let state = app.state::<AppState>();
    if !state.try_begin_thumbnail_build() {
        return Err("Thumbnail generation is already in progress.".to_string());
    }
    let result = crate::render::clear_image_cache(&state.data_dir);
    state.finish_thumbnail_build();
    result?;
    Ok(state.settings_payload())
}

pub fn rebuild_thumbnails(
    app: &AppHandle,
) -> Result<(SettingsPayload, ThumbnailCacheSummary), String> {
    let state = app.state::<AppState>();
    if !state.try_begin_thumbnail_build() {
        return Err("Thumbnail generation is already in progress.".to_string());
    }
    let result = (|| {
        let (settings, library) = ensure_library(app)?;
        let label = root_summary_label(&get_photo_directories(&settings));
        crate::render::clear_thumbnail_cache(&state.data_dir)?;
        let (thumbnails, warnings) =
            pregenerate_thumbnails(app, &state.data_dir, &library.photos, &label, false);
        store_thumbnail_warnings(&state, warnings);
        emit_thumbnail_complete(app, &label, &thumbnails);
        Ok((state.settings_payload(), thumbnails))
    })();
    state.finish_thumbnail_build();
    result
}

fn emit_thumbnail_complete(app: &AppHandle, label: &str, thumbnails: &ThumbnailCacheSummary) {
    let mut progress = ScanProgress::new(ScanPhase::Complete);
    progress.root_dir = Some(label.to_string());
    progress.photos_found = Some(thumbnails.total);
    progress.photos_processed = Some(thumbnails.total);
    progress.thumbnails_generated = Some(thumbnails.generated);
    progress.thumbnails_reused = Some(thumbnails.reused);
    progress.thumbnail_failures = Some(thumbnails.failed);
    progress.total_photos = Some(thumbnails.total);
    progress.message = Some(if thumbnails.failed > 0 {
        format!(
            "Thumbnail rebuild finished - {} ready - {} failed",
            thumbnails.generated + thumbnails.reused,
            thumbnails.failed
        )
    } else {
        format!("{} thumbnails ready", thumbnails.total)
    });
    let _ = app.emit("scan:progress", progress);
}

fn pregenerate_thumbnails(
    app: &AppHandle,
    data_dir: &Path,
    photos: &[PhotoAsset],
    label: &str,
    quiet: bool,
) -> (ThumbnailCacheSummary, Vec<String>) {
    let total = photos.len();
    let progress_event = |processed, generated, reused, failed, current_path: Option<String>| {
        let mut progress = ScanProgress::new(ScanPhase::GeneratingThumbnails);
        progress.root_dir = Some(label.to_string());
        progress.photos_found = Some(total);
        progress.photos_processed = Some(processed);
        progress.thumbnails_generated = Some(generated);
        progress.thumbnails_reused = Some(reused);
        progress.thumbnail_failures = Some(failed);
        progress.total_photos = Some(total);
        progress.current_path = current_path;
        progress.message = Some(if processed == 0 {
            "Preparing thumbnail cache".to_string()
        } else {
            "Building thumbnail cache".to_string()
        });
        progress
    };
    if !quiet {
        let _ = app.emit("scan:progress", progress_event(0, 0, 0, 0, None));
    }
    if total == 0 {
        return (ThumbnailCacheSummary::default(), Vec::new());
    }

    let next_index = AtomicUsize::new(0);
    let processed = AtomicUsize::new(0);
    let generated = AtomicUsize::new(0);
    let reused = AtomicUsize::new(0);
    let failures = AtomicUsize::new(0);
    let warnings = Mutex::new(Vec::new());
    let last_reported = Mutex::new(0usize);
    let report_interval = total.div_ceil(100).max(1);
    // Leave one render slot free so on-demand requests from the grid stay responsive.
    let worker_count = image_render_job_limit().saturating_sub(1).max(1).min(total);

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| loop {
                let index = next_index.fetch_add(1, Ordering::Relaxed);
                let Some(photo) = photos.get(index) else {
                    break;
                };

                match ensure_cached_render(data_dir, &photo.path, PhotoRenderVariant::Thumb) {
                    Ok((_, true)) => {
                        generated.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok((_, false)) => {
                        reused.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(error) => {
                        failures.fetch_add(1, Ordering::Relaxed);
                        let mut list = lock(&warnings);
                        if list.len() < 20 {
                            list.push(format!("Thumbnail for {}: {}", photo.path, error));
                        }
                    }
                }

                let completed = processed.fetch_add(1, Ordering::Relaxed) + 1;
                if quiet || (completed != total && completed % report_interval != 0) {
                    continue;
                }
                let mut reported = lock(&last_reported);
                if completed <= *reported {
                    continue;
                }
                *reported = completed;
                let _ = app.emit(
                    "scan:progress",
                    progress_event(
                        completed,
                        generated.load(Ordering::Relaxed),
                        reused.load(Ordering::Relaxed),
                        failures.load(Ordering::Relaxed),
                        Some(photo.path.clone()),
                    ),
                );
            });
        }
    });

    let summary = ThumbnailCacheSummary {
        total,
        generated: generated.load(Ordering::Relaxed),
        reused: reused.load(Ordering::Relaxed),
        failed: failures.load(Ordering::Relaxed),
    };
    let warnings = warnings
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    (summary, warnings)
}
