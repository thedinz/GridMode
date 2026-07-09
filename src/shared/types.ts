export interface Settings {
  photoDirectory?: string;
  photoDirectories?: string[];
  excludedDirectories?: string[];
  lastScanAt?: string;
}

export interface SettingsPayload {
  settings: Settings;
  summary: LibrarySummary;
}

export interface PhotoLocation {
  latitude: number;
  longitude: number;
}

export interface PhotoAsset {
  id: string;
  name: string;
  path: string;
  directory: string;
  extension: string;
  size: number;
  url: string;
  thumbnailUrl: string;
  capturedAt: string;
  dateSource: "exif" | "file";
  year: number;
  month: number;
  monthName: string;
  location?: PhotoLocation;
  width?: number;
  height?: number;
}

export interface YearSummary {
  year: number;
  count: number;
  sample: PhotoAsset[];
}

export interface MonthSummary {
  year: number;
  month: number;
  monthName: string;
  count: number;
  sample: PhotoAsset[];
}

export interface LibrarySummary {
  rootDir?: string;
  rootDirs?: string[];
  photoCount: number;
  years: YearSummary[];
  lastScanAt?: string;
  warnings: string[];
}

export type ScanPhase = "idle" | "discovering" | "reading-metadata" | "complete" | "error";

export interface ScanProgress {
  phase: ScanPhase;
  rootDir?: string;
  foldersScanned?: number;
  photosFound?: number;
  photosProcessed?: number;
  photosReused?: number;
  photosChanged?: number;
  photosRemoved?: number;
  foldersExcluded?: number;
  totalPhotos?: number;
  currentPath?: string;
  message?: string;
}

export interface HomePayload {
  summary: LibrarySummary;
  photos: PhotoAsset[];
}

export interface YearPayload {
  year: number;
  months: MonthSummary[];
}

export interface MonthPayload {
  year: number;
  month: number;
  monthName: string;
  photos: PhotoAsset[];
}

export interface ExifRow {
  label: string;
  value: string;
}

export interface PhotoDetails {
  photo: PhotoAsset;
  exif: ExifRow[];
}

export interface GridModeApi {
  settings: {
    get: () => Promise<SettingsPayload>;
    chooseRoot: () => Promise<SettingsPayload>;
    addRoot: () => Promise<SettingsPayload>;
    removeRoot: (rootPath: string) => Promise<SettingsPayload>;
    clearCache: () => Promise<SettingsPayload>;
    chooseExclusion: () => Promise<SettingsPayload>;
    removeExclusion: (excludedPath: string) => Promise<SettingsPayload>;
  };
  library: {
    scan: (force?: boolean) => Promise<LibrarySummary>;
    getHome: () => Promise<HomePayload>;
    getYears: () => Promise<LibrarySummary>;
    getYear: (year: number) => Promise<YearPayload>;
    getMonth: (year: number, month: number) => Promise<MonthPayload>;
    onProgress: (callback: (progress: ScanProgress) => void) => () => void;
  };
  photo: {
    getDetails: (photoPath: string) => Promise<PhotoDetails>;
  };
  updates: {
    check: () => Promise<UpdateStatus>;
    download: () => Promise<UpdateStatus>;
    install: () => void;
    onStatus: (callback: (status: UpdateStatus) => void) => () => void;
  };
}

export type UpdateState =
  | "idle"
  | "checking"
  | "available"
  | "not-available"
  | "downloading"
  | "downloaded"
  | "error";

export interface UpdateStatus {
  state: UpdateState;
  version?: string;
  message?: string;
  percent?: number;
  downloadUrl?: string;
  manualDownload?: boolean;
}
