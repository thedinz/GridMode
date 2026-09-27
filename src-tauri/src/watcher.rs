//! Watches the photo locations and rescans in the background when photos are
//! added, changed, or removed, so the library stays current without a manual refresh.

use crate::render::is_supported_photo_path;
use crate::settings::{get_photo_directories, is_path_excluded};
use crate::state::{run_scan, AppState, ScanMode};
use notify_debouncer_mini::{
    new_debouncer,
    notify::{RecommendedWatcher, RecursiveMode},
    DebounceEventResult, Debouncer,
};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};

/// Quiet period after the last file event before rescanning, so copying a
/// folder of photos triggers one scan instead of hundreds.
const DEBOUNCE: Duration = Duration::from_secs(3);

#[derive(Default)]
pub struct WatcherState {
    debouncer: Mutex<Option<Debouncer<RecommendedWatcher>>>,
    rescan_requested: AtomicBool,
    rescan_running: AtomicBool,
}

/// (Re)starts watching the configured photo locations.
pub fn refresh(app: &AppHandle) {
    let root_dirs = get_photo_directories(&app.state::<AppState>().settings());
    let watcher_state = app.state::<WatcherState>();
    let mut slot = watcher_state
        .debouncer
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    // Dropping the old debouncer stops its watches before new ones start.
    *slot = None;
    if root_dirs.is_empty() {
        return;
    }

    let handler_app = app.clone();
    let debouncer = new_debouncer(DEBOUNCE, move |result: DebounceEventResult| match result {
        Ok(events) => {
            if events
                .iter()
                .any(|event| is_relevant_change(&handler_app, &event.path))
            {
                request_rescan(&handler_app);
            }
        }
        Err(error) => log::warn!("Folder watcher error: {error}"),
    });

    let mut debouncer = match debouncer {
        Ok(debouncer) => debouncer,
        Err(error) => {
            log::warn!("Could not start the folder watcher: {error}");
            return;
        }
    };
    for root in &root_dirs {
        if let Err(error) = debouncer
            .watcher()
            .watch(Path::new(root), RecursiveMode::Recursive)
        {
            log::warn!("Could not watch {root}: {error}");
        }
    }
    *slot = Some(debouncer);
}

fn is_relevant_change(app: &AppHandle, path: &Path) -> bool {
    let state = app.state::<AppState>();
    if state.is_data_path(path) {
        return false;
    }
    // Directory events have no extension; a renamed or deleted folder can
    // change many photos at once.
    let could_affect_photos = is_supported_photo_path(path) || path.extension().is_none();
    could_affect_photos
        && !is_path_excluded(
            &crate::paths::path_to_string(path),
            &state.settings().excluded_directories,
        )
}

/// Coalesces bursts of changes: at most one background scan runs, and one
/// more follows if changes arrive while it is running.
fn request_rescan(app: &AppHandle) {
    let watcher_state = app.state::<WatcherState>();
    watcher_state
        .rescan_requested
        .store(true, Ordering::Release);
    if watcher_state.rescan_running.swap(true, Ordering::AcqRel) {
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || loop {
        let watcher_state = app.state::<WatcherState>();
        if !watcher_state.rescan_requested.swap(false, Ordering::AcqRel) {
            watcher_state.rescan_running.store(false, Ordering::Release);
            // A request may have landed between the swap and the store.
            if watcher_state.rescan_requested.load(Ordering::Acquire)
                && !watcher_state.rescan_running.swap(true, Ordering::AcqRel)
            {
                continue;
            }
            break;
        }

        match run_scan(&app, false, ScanMode::Background) {
            Ok(outcome) if outcome.has_changes() => {
                let _ = app.emit("library:changed", outcome.library.summary.clone());
            }
            Ok(_) => {}
            Err(error) => log::warn!("Background rescan failed: {error}"),
        }
    });
}
