import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { GridModeApi, Unsubscribe } from "../shared/types";

function subscribe<T>(eventName: string, callback: (payload: T) => void): Unsubscribe {
  let disposed = false;
  const unlisten = listen<T>(eventName, (event) => {
    callback(event.payload);
  });

  unlisten.then((stop) => {
    if (disposed) {
      stop();
    }
  }).catch(() => {
    // Event subscriptions can reject while the Tauri bridge is still booting.
  });

  return () => {
    disposed = true;
    void unlisten.then((stop) => stop()).catch(() => undefined);
  };
}

// Photo URLs arrive ready to use: the backend builds them for the
// gridmode-photo scheme, so payloads pass straight through.
export const gridModeApi: GridModeApi = {
  settings: {
    get: () => invoke("settings_get"),
    chooseRoot: () => invoke("settings_choose_root"),
    addRoot: () => invoke("settings_add_root"),
    removeRoot: (rootPath) => invoke("settings_remove_root", { rootPath }),
    clearCache: () => invoke("settings_clear_cache"),
    rebuildThumbnails: () => invoke("settings_rebuild_thumbnails"),
    chooseExclusion: () => invoke("settings_choose_exclusion"),
    removeExclusion: (excludedPath) => invoke("settings_remove_exclusion", { excludedPath })
  },
  library: {
    scan: (force = false) => invoke("library_scan", { force }),
    getHome: () => invoke("library_get_home"),
    getYears: () => invoke("library_get_years"),
    getYear: (year) => invoke("library_get_year", { year }),
    getMonth: (year, month) => invoke("library_get_month", { year, month }),
    getFolders: () => invoke("library_get_folders"),
    getDirectory: (directoryPath) => invoke("library_get_directory", { directoryPath }),
    search: (query) => invoke("library_search", { query }),
    onProgress: (callback) => subscribe("scan:progress", callback),
    onChanged: (callback) => subscribe("library:changed", callback)
  },
  photo: {
    getDetails: (photoPath) => invoke("photo_get_details", { photoPath }),
    reveal: (photoPath) => invoke("photo_reveal", { photoPath }),
    open: (photoPath) => invoke("photo_open", { photoPath }),
    openMap: (photoPath) => invoke("photo_open_map", { photoPath })
  },
  updates: {
    check: (options = {}) => invoke("updates_check", options),
    download: () => invoke("updates_download"),
    openDownload: (downloadUrl) => invoke("updates_open_download", { downloadUrl }),
    install: () => invoke("updates_install"),
    onStatus: (callback) => subscribe("updates:status", callback)
  }
};
