//! Tauri commands invoked by the renderer.
//!
//! Every command runs on the blocking thread pool: scans, EXIF reads, and
//! folder dialogs must never run on the main thread, where they would freeze
//! the window.

use crate::library::{
    directory_breadcrumbs, directory_payload, folders_payload, month_payload, on_this_day,
    random_photos, search, time_seed, today_local, year_payload, HOME_PHOTO_COUNT,
    ON_THIS_DAY_LIMIT, SEARCH_RESULT_LIMIT,
};
use crate::metadata::{detail_rows, read_exif};
use crate::model::{
    DirectoryPayload, FoldersPayload, HomePayload, LibrarySummary, MonthPayload, PhotoAsset,
    PhotoDetails, SearchPayload, SearchQuery, SettingsPayload, ThumbnailRebuildPayload,
    YearPayload,
};
use crate::paths::canonicalize_path;
use crate::settings::{
    add_excluded_directory, get_photo_directories, is_valid_excluded_directory,
    remove_excluded_directory,
};
use crate::state::{change_settings_and_rescan, ensure_library, run_scan, AppState, ScanMode};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

async fn blocking<T: Send + 'static>(
    app: AppHandle,
    task: impl FnOnce(&AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || task(&app))
        .await
        .map_err(|error| format!("Background task stopped unexpectedly: {error}"))?
}

fn pick_folder(app: &AppHandle, title: &str) -> Option<String> {
    app.dialog()
        .file()
        .set_title(title)
        .blocking_pick_folder()
        .map(|selected| canonicalize_path(&selected.to_string()))
}

fn library_photo(app: &AppHandle, photo_path: &str) -> Result<PhotoAsset, String> {
    ensure_library(app)?;
    app.state::<AppState>()
        .find_photo(photo_path)
        .ok_or_else(|| "Photo is not part of the current library.".to_string())
}

#[tauri::command]
pub async fn settings_get(app: AppHandle) -> Result<SettingsPayload, String> {
    blocking(app, |app| Ok(app.state::<AppState>().settings_payload())).await
}

#[tauri::command]
pub async fn settings_choose_root(app: AppHandle) -> Result<SettingsPayload, String> {
    blocking(app, |app| {
        let Some(selected) = pick_folder(app, "Choose a photo folder") else {
            return Ok(app.state::<AppState>().settings_payload());
        };
        change_settings_and_rescan(app, |settings| {
            let previous = get_photo_directories(settings);
            let is_first_root = previous.is_empty();
            let mut next = vec![selected];
            next.extend(previous.into_iter().skip(1));
            settings.photo_directory = None;
            settings.photo_directories = next;
            if is_first_root {
                settings.excluded_directories.clear();
            }
            is_first_root
        })
    })
    .await
}

#[tauri::command]
pub async fn settings_add_root(app: AppHandle) -> Result<SettingsPayload, String> {
    blocking(app, |app| {
        let Some(selected) = pick_folder(app, "Add a photo folder") else {
            return Ok(app.state::<AppState>().settings_payload());
        };
        change_settings_and_rescan(app, |settings| {
            let mut next = get_photo_directories(settings);
            next.push(selected);
            settings.photo_directory = None;
            settings.photo_directories = next;
            false
        })
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn settings_remove_root(
    app: AppHandle,
    root_path: String,
) -> Result<SettingsPayload, String> {
    blocking(app, move |app| {
        change_settings_and_rescan(app, |settings| {
            let next = get_photo_directories(settings)
                .into_iter()
                .filter(|item| !crate::paths::same_directory(item, &root_path))
                .collect();
            settings.photo_directory = None;
            settings.photo_directories = next;
            false
        })
    })
    .await
}

#[tauri::command]
pub async fn settings_choose_exclusion(app: AppHandle) -> Result<SettingsPayload, String> {
    blocking(app, |app| {
        let Some(selected) = pick_folder(app, "Exclude a folder") else {
            return Ok(app.state::<AppState>().settings_payload());
        };
        let root_dirs = get_photo_directories(&app.state::<AppState>().settings());
        if !is_valid_excluded_directory(&root_dirs, &selected) {
            return Ok(app.state::<AppState>().settings_payload());
        }
        change_settings_and_rescan(app, |settings| {
            settings.excluded_directories =
                add_excluded_directory(&root_dirs, &settings.excluded_directories, &selected);
            false
        })
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn settings_remove_exclusion(
    app: AppHandle,
    excluded_path: String,
) -> Result<SettingsPayload, String> {
    blocking(app, move |app| {
        change_settings_and_rescan(app, |settings| {
            settings.excluded_directories =
                remove_excluded_directory(&settings.excluded_directories, &excluded_path);
            false
        })
    })
    .await
}

#[tauri::command]
pub async fn settings_clear_cache(app: AppHandle) -> Result<SettingsPayload, String> {
    blocking(app, crate::state::clear_image_cache).await
}

#[tauri::command]
pub async fn settings_rebuild_thumbnails(
    app: AppHandle,
) -> Result<ThumbnailRebuildPayload, String> {
    blocking(app, |app| {
        let (payload, thumbnails) = crate::state::rebuild_thumbnails(app)?;
        Ok(ThumbnailRebuildPayload {
            settings: payload.settings,
            summary: payload.summary,
            thumbnails,
        })
    })
    .await
}

#[tauri::command]
pub async fn library_scan(app: AppHandle, force: bool) -> Result<LibrarySummary, String> {
    blocking(app, move |app| {
        Ok(run_scan(app, force, ScanMode::Interactive)?.library.summary)
    })
    .await
}

#[tauri::command]
pub async fn library_get_home(app: AppHandle) -> Result<HomePayload, String> {
    blocking(app, |app| {
        let (_, library) = ensure_library(app)?;
        Ok(HomePayload {
            photos: random_photos(&library.photos, HOME_PHOTO_COUNT, time_seed()),
            on_this_day: on_this_day(&library.photos, today_local(), ON_THIS_DAY_LIMIT),
            summary: library.summary,
        })
    })
    .await
}

#[tauri::command]
pub async fn library_get_years(app: AppHandle) -> Result<LibrarySummary, String> {
    blocking(app, |app| Ok(ensure_library(app)?.1.summary)).await
}

#[tauri::command]
pub async fn library_get_year(app: AppHandle, year: i32) -> Result<YearPayload, String> {
    blocking(app, move |app| {
        Ok(year_payload(&ensure_library(app)?.1.photos, year))
    })
    .await
}

#[tauri::command]
pub async fn library_get_month(
    app: AppHandle,
    year: i32,
    month: u32,
) -> Result<MonthPayload, String> {
    blocking(app, move |app| {
        Ok(month_payload(&ensure_library(app)?.1.photos, year, month))
    })
    .await
}

#[tauri::command]
pub async fn library_get_folders(app: AppHandle) -> Result<FoldersPayload, String> {
    blocking(app, |app| {
        let (settings, library) = ensure_library(app)?;
        Ok(folders_payload(
            &library.photos,
            &get_photo_directories(&settings),
        ))
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn library_get_directory(
    app: AppHandle,
    directory_path: String,
) -> Result<DirectoryPayload, String> {
    blocking(app, move |app| {
        let (settings, library) = ensure_library(app)?;
        directory_payload(
            &library.photos,
            &get_photo_directories(&settings),
            &settings.excluded_directories,
            &directory_path,
        )
    })
    .await
}

#[tauri::command]
pub async fn library_search(app: AppHandle, query: SearchQuery) -> Result<SearchPayload, String> {
    blocking(app, move |app| {
        Ok(search(
            &ensure_library(app)?.1.photos,
            &query,
            SEARCH_RESULT_LIMIT,
        ))
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn photo_get_details(app: AppHandle, photo_path: String) -> Result<PhotoDetails, String> {
    blocking(app, move |app| {
        let photo = library_photo(app, &photo_path)?;
        let root_dirs = get_photo_directories(&app.state::<AppState>().settings());
        let directory_breadcrumbs = directory_breadcrumbs(&root_dirs, &photo.directory)
            .ok_or_else(|| "Photo directory is outside the configured library.".to_string())?;
        let exif = read_exif(&photo.path)
            .map(|exif| detail_rows(&exif))
            .unwrap_or_default();
        Ok(PhotoDetails {
            photo,
            exif,
            directory_breadcrumbs,
        })
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn photo_reveal(app: AppHandle, photo_path: String) -> Result<(), String> {
    blocking(app, move |app| {
        let photo = library_photo(app, &photo_path)?;
        app.opener()
            .reveal_item_in_dir(&photo.path)
            .map_err(|error| format!("Could not show the photo in its folder: {error}"))
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn photo_open(app: AppHandle, photo_path: String) -> Result<(), String> {
    blocking(app, move |app| {
        let photo = library_photo(app, &photo_path)?;
        app.opener()
            .open_path(&photo.path, None::<&str>)
            .map_err(|error| format!("Could not open the photo: {error}"))
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn photo_open_map(app: AppHandle, photo_path: String) -> Result<(), String> {
    blocking(app, move |app| {
        let photo = library_photo(app, &photo_path)?;
        let location = photo
            .location
            .ok_or_else(|| "This photo has no location.".to_string())?;
        // The URL is built only from stored coordinates, never from renderer input.
        let url = format!(
            "https://www.openstreetmap.org/?mlat={lat:.6}&mlon={lon:.6}#map=15/{lat:.6}/{lon:.6}",
            lat = location.latitude,
            lon = location.longitude
        );
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|error| format!("Could not open the map: {error}"))
    })
    .await
}
