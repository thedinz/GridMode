export interface Settings {
  photoDirectory?: string;
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
}
