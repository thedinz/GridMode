import type { LibrarySummary, PhotoAsset, PhotoLocation, ScanProgress } from "../../shared/types";

export function monthName(month: number): string {
  return new Date(2024, month - 1, 1).toLocaleString(undefined, { month: "long" });
}

/** Capture times are local wall-clock strings without an offset, which `Date` parses as local time. */
export function formatDate(value: string): string {
  return new Date(value).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric"
  });
}

export function formatCaptureTime(value: string): string {
  return new Date(value).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit"
  });
}

export function formatDateTime(value: string): string {
  return new Date(value).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    second: "2-digit"
  });
}

export function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = bytes;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export function formatDimensions(photo: Pick<PhotoAsset, "width" | "height">): string | undefined {
  if (!photo.width || !photo.height) {
    return undefined;
  }
  const megapixels = (photo.width * photo.height) / 1_000_000;
  const megapixelText = megapixels >= 1 ? ` (${megapixels.toFixed(1)} MP)` : "";
  return `${photo.width.toLocaleString()} × ${photo.height.toLocaleString()}${megapixelText}`;
}

export function formatCoordinates(location: PhotoLocation): string {
  const latitude = `${Math.abs(location.latitude).toFixed(5)}° ${location.latitude >= 0 ? "N" : "S"}`;
  const longitude = `${Math.abs(location.longitude).toFixed(5)}° ${location.longitude >= 0 ? "E" : "W"}`;
  return `${latitude}, ${longitude}`;
}

export function formatLibraryActionComplete(label: string, summary: LibrarySummary): string {
  const scannedAt = formatDateTime(summary.lastScanAt ?? new Date().toISOString());
  return `${label} - ${summary.photoCount.toLocaleString()} photos indexed at ${scannedAt}`;
}

export function getScanProgressPercent(progress?: ScanProgress): number | undefined {
  if (
    !progress ||
    (progress.phase !== "reading-metadata" && progress.phase !== "generating-thumbnails") ||
    !progress.totalPhotos
  ) {
    return undefined;
  }
  return Math.min(100, Math.round(((progress.photosProcessed ?? 0) / progress.totalPhotos) * 100));
}

export function formatScanProgress(progress: ScanProgress, percent: number | undefined): string {
  if (progress.phase === "discovering") {
    const folders = progress.foldersScanned ?? 0;
    const photos = progress.photosFound ?? 0;
    const excluded = progress.foldersExcluded ?? 0;
    const excludedText = excluded > 0 ? ` - ${excluded.toLocaleString()} excluded` : "";
    return `${folders.toLocaleString()} folders checked - ${photos.toLocaleString()} photos found${excludedText}`;
  }

  if (progress.phase === "reading-metadata") {
    const processed = progress.photosProcessed ?? 0;
    const total = progress.totalPhotos ?? progress.photosFound ?? 0;
    const reused = progress.photosReused ?? 0;
    if (total === 0 && reused > 0) {
      return `${reused.toLocaleString()} cached photos reused - no metadata work`;
    }
    const suffix = percent === undefined ? "" : ` - ${percent}%`;
    const reusedText = reused > 0 ? ` - ${reused.toLocaleString()} cached` : "";
    return `${processed.toLocaleString()} / ${total.toLocaleString()} photos processed${suffix}${reusedText}`;
  }

  if (progress.phase === "generating-thumbnails") {
    const processed = progress.photosProcessed ?? 0;
    const total = progress.totalPhotos ?? progress.photosFound ?? 0;
    const generated = progress.thumbnailsGenerated ?? 0;
    const cached = progress.thumbnailsReused ?? 0;
    const failures = progress.thumbnailFailures ?? 0;
    const suffix = percent === undefined ? "" : ` - ${percent}%`;
    const cachedText = cached > 0 ? ` - ${cached.toLocaleString()} already cached` : "";
    const failureText = failures > 0 ? ` - ${failures.toLocaleString()} failed` : "";
    return `${processed.toLocaleString()} / ${total.toLocaleString()} thumbnails checked${suffix} - ${generated.toLocaleString()} generated${cachedText}${failureText}`;
  }

  return progress.message ?? "Scan complete";
}

export function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function formatTrialRemaining(days: number): string {
  return `${days.toLocaleString()} ${days === 1 ? "day" : "days"} remaining`;
}
