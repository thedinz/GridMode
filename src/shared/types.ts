// Data types are generated from the Rust models by `cargo test`; see
// src-tauri/src/model.rs. Only the renderer-side API surface lives here.
export type * from "./generated/bindings";

import type {
  DirectoryPayload,
  FoldersPayload,
  HomePayload,
  LibrarySummary,
  MonthPayload,
  PhotoDetails,
  ScanProgress,
  SearchPayload,
  SearchQuery,
  SettingsPayload,
  ThumbnailRebuildPayload,
  UpdateStatus,
  YearPayload
} from "./generated/bindings";

export type Unsubscribe = () => void;

export interface GridModeApi {
  settings: {
    get: () => Promise<SettingsPayload>;
    chooseRoot: () => Promise<SettingsPayload>;
    addRoot: () => Promise<SettingsPayload>;
    removeRoot: (rootPath: string) => Promise<SettingsPayload>;
    clearCache: () => Promise<SettingsPayload>;
    rebuildThumbnails: () => Promise<ThumbnailRebuildPayload>;
    chooseExclusion: () => Promise<SettingsPayload>;
    removeExclusion: (excludedPath: string) => Promise<SettingsPayload>;
  };
  library: {
    scan: (force?: boolean) => Promise<LibrarySummary>;
    getHome: () => Promise<HomePayload>;
    getYears: () => Promise<LibrarySummary>;
    getYear: (year: number) => Promise<YearPayload>;
    getMonth: (year: number, month: number) => Promise<MonthPayload>;
    getFolders: () => Promise<FoldersPayload>;
    getDirectory: (directoryPath: string) => Promise<DirectoryPayload>;
    search: (query: SearchQuery) => Promise<SearchPayload>;
    onProgress: (callback: (progress: ScanProgress) => void) => Unsubscribe;
    /** Fired after a background rescan picks up added, changed, or removed photos. */
    onChanged: (callback: (summary: LibrarySummary) => void) => Unsubscribe;
  };
  photo: {
    getDetails: (photoPath: string) => Promise<PhotoDetails>;
    reveal: (photoPath: string) => Promise<void>;
    open: (photoPath: string) => Promise<void>;
    openMap: (photoPath: string) => Promise<void>;
  };
  updates: {
    check: (options?: { automatic?: boolean }) => Promise<UpdateStatus>;
    download: () => Promise<UpdateStatus>;
    openDownload: (downloadUrl: string) => Promise<UpdateStatus>;
    install: () => Promise<UpdateStatus>;
    onStatus: (callback: (status: UpdateStatus) => void) => Unsubscribe;
  };
}
