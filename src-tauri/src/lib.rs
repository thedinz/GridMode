mod commands;
mod library;
mod licensing;
mod metadata;
mod model;
mod paths;
mod protocol;
mod render;
mod settings;
mod state;
mod updates;
mod watcher;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("gridmode-photo", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let uri = request.uri().to_string();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(protocol::handle_photo_request(&app, &uri));
            });
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // Debug builds can point at a scratch data folder so development never
            // touches the settings and index of an installed copy.
            let dev_data_dir =
                std::env::var_os("GRIDMODE_DATA_DIR").filter(|_| cfg!(debug_assertions));
            let keyring_account = if dev_data_dir.is_some() {
                licensing::store::KEYRING_DEV_ACCOUNT
            } else {
                licensing::store::KEYRING_ACCOUNT
            };
            let data_dir = match dev_data_dir {
                Some(dir) => std::path::PathBuf::from(dir),
                None => app.path().app_data_dir()?,
            };
            app.manage(licensing::LicenseService::new(
                licensing::store::LicenseStorage::new(
                    Box::new(licensing::store::KeyringStore::new(keyring_account)),
                    data_dir.clone(),
                ),
                licensing::api::LicenseApi::new(licensing::config::LICENSE_API_BASE),
                licensing::config::ACTIVE,
                licensing::config::GRIDMODE_MAJOR_VERSION,
                licensing::unix_now(),
            ));
            licensing::start_background_validation(app.handle());
            app.manage(state::AppState::load(data_dir));
            app.manage(watcher::WatcherState::default());
            app.manage(updates::PendingUpdateState::default());
            watcher::refresh(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings_get,
            commands::settings_choose_root,
            commands::settings_add_root,
            commands::settings_remove_root,
            commands::settings_clear_cache,
            commands::settings_rebuild_thumbnails,
            commands::settings_choose_exclusion,
            commands::settings_remove_exclusion,
            commands::library_scan,
            commands::library_get_home,
            commands::library_get_years,
            commands::library_get_year,
            commands::library_get_month,
            commands::library_get_folders,
            commands::library_get_directory,
            commands::library_search,
            commands::photo_get_details,
            commands::photo_reveal,
            commands::photo_open,
            commands::photo_open_map,
            updates::updates_check,
            updates::updates_download,
            updates::updates_open_download,
            updates::updates_install,
            licensing::license_get_status,
            licensing::license_start_trial,
            licensing::license_activate,
            licensing::license_deactivate,
            licensing::license_refresh,
            licensing::license_dismiss_notice,
            licensing::license_open_checkout
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
