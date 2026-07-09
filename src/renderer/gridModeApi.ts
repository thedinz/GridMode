import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  GridModeApi,
  HomePayload,
  LibrarySummary,
  MonthPayload,
  PhotoAsset,
  PhotoDetails,
  ScanProgress,
  SettingsPayload,
  UpdateStatus,
  YearPayload
} from "../shared/types";

type Unsubscribe = () => void;

function isTauriRuntime(): boolean {
  return Boolean(window.__TAURI_INTERNALS__);
}

function encodePathToken(filePath: string): string {
  const bytes = new TextEncoder().encode(filePath);
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary)
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

function makeTauriPhotoUrl(filePath: string, variant: "display" | "thumb"): string {
  return convertFileSrc(`${variant}/${encodePathToken(filePath)}`, "gridmode-photo");
}

function convertPhoto(photo: PhotoAsset): PhotoAsset {
  return {
    ...photo,
    url: makeTauriPhotoUrl(photo.path, "display"),
    thumbnailUrl: makeTauriPhotoUrl(photo.path, "thumb")
  };
}

function convertSummary(summary: LibrarySummary): LibrarySummary {
  return {
    ...summary,
    years: summary.years.map((year) => ({
      ...year,
      sample: year.sample.map(convertPhoto)
    }))
  };
}

function convertSettingsPayload(payload: SettingsPayload): SettingsPayload {
  return {
    settings: payload.settings,
    summary: convertSummary(payload.summary)
  };
}

function convertHomePayload(payload: HomePayload): HomePayload {
  return {
    summary: convertSummary(payload.summary),
    photos: payload.photos.map(convertPhoto)
  };
}

function convertYearPayload(payload: YearPayload): YearPayload {
  return {
    ...payload,
    months: payload.months.map((month) => ({
      ...month,
      sample: month.sample.map(convertPhoto)
    }))
  };
}

function convertMonthPayload(payload: MonthPayload): MonthPayload {
  return {
    ...payload,
    photos: payload.photos.map(convertPhoto)
  };
}

function convertPhotoDetails(payload: PhotoDetails): PhotoDetails {
  return {
    ...payload,
    photo: convertPhoto(payload.photo)
  };
}

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
    void unlisten.then((stop) => stop());
  };
}

function createTauriApi(): GridModeApi {
  return {
    settings: {
      get: async () => convertSettingsPayload(await invoke<SettingsPayload>("settings_get")),
      chooseRoot: async () => convertSettingsPayload(await invoke<SettingsPayload>("settings_choose_root")),
      addRoot: async () => convertSettingsPayload(await invoke<SettingsPayload>("settings_add_root")),
      removeRoot: async (rootPath: string) =>
        convertSettingsPayload(await invoke<SettingsPayload>("settings_remove_root", { rootPath })),
      clearCache: async () => convertSettingsPayload(await invoke<SettingsPayload>("settings_clear_cache")),
      chooseExclusion: async () =>
        convertSettingsPayload(await invoke<SettingsPayload>("settings_choose_exclusion")),
      removeExclusion: async (excludedPath: string) =>
        convertSettingsPayload(await invoke<SettingsPayload>("settings_remove_exclusion", { excludedPath }))
    },
    library: {
      scan: async (force = false) => convertSummary(await invoke<LibrarySummary>("library_scan", { force })),
      getHome: async () => convertHomePayload(await invoke<HomePayload>("library_get_home")),
      getYears: async () => convertSummary(await invoke<LibrarySummary>("library_get_years")),
      getYear: async (year: number) => convertYearPayload(await invoke<YearPayload>("library_get_year", { year })),
      getMonth: async (year: number, month: number) =>
        convertMonthPayload(await invoke<MonthPayload>("library_get_month", { year, month })),
      onProgress: (callback: (progress: ScanProgress) => void) => subscribe("scan:progress", callback)
    },
    photo: {
      getDetails: async (photoPath: string) =>
        convertPhotoDetails(await invoke<PhotoDetails>("photo_get_details", { photoPath }))
    },
    updates: {
      check: () => invoke<UpdateStatus>("updates_check"),
      download: () => invoke<UpdateStatus>("updates_download"),
      install: () => invoke<UpdateStatus>("updates_install"),
      onStatus: (callback: (status: UpdateStatus) => void) => subscribe("updates:status", callback)
    }
  };
}

if (!window.gridMode && !isTauriRuntime()) {
  throw new Error("GridMode desktop bridge is not available.");
}

export const gridModeApi: GridModeApi = window.gridMode ?? createTauriApi();

export type GridModeRendererApi = typeof gridModeApi;
