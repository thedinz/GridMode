import { describe, expect, it } from "vitest";
import {
  formatBytes,
  formatCoordinates,
  formatDimensions,
  formatScanProgress,
  formatTrialRemaining,
  getErrorMessage,
  getScanProgressPercent
} from "./format";

describe("formatBytes", () => {
  it("scales through binary units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MB");
  });
});

describe("formatDimensions", () => {
  it("includes megapixels for large images", () => {
    expect(formatDimensions({ width: 6000, height: 4000 })).toBe("6,000 × 4,000 (24.0 MP)");
  });

  it("omits megapixels below one and handles missing sizes", () => {
    expect(formatDimensions({ width: 640, height: 480 })).toBe("640 × 480");
    expect(formatDimensions({ width: 640 })).toBeUndefined();
  });
});

describe("formatCoordinates", () => {
  it("uses hemisphere letters instead of signs", () => {
    expect(formatCoordinates({ latitude: 40.4462, longitude: -79.9823 })).toBe("40.44620° N, 79.98230° W");
    expect(formatCoordinates({ latitude: -33.8688, longitude: 151.2093 })).toBe("33.86880° S, 151.20930° E");
  });
});

describe("scan progress", () => {
  it("reports a percentage only for countable phases", () => {
    expect(getScanProgressPercent({ phase: "discovering", totalPhotos: 10 })).toBeUndefined();
    expect(getScanProgressPercent({ phase: "reading-metadata", photosProcessed: 5, totalPhotos: 20 })).toBe(25);
    expect(getScanProgressPercent({ phase: "generating-thumbnails", photosProcessed: 30, totalPhotos: 20 })).toBe(100);
  });

  it("describes discovery and cached-only metadata passes", () => {
    expect(
      formatScanProgress({ phase: "discovering", foldersScanned: 3, photosFound: 12, foldersExcluded: 1 }, undefined)
    ).toBe("3 folders checked - 12 photos found - 1 excluded");
    expect(formatScanProgress({ phase: "reading-metadata", totalPhotos: 0, photosReused: 40 }, undefined)).toBe(
      "40 cached photos reused - no metadata work"
    );
  });

  it("falls back to the message once complete", () => {
    expect(formatScanProgress({ phase: "complete", message: "Indexed 4 photos" }, undefined)).toBe("Indexed 4 photos");
  });
});

describe("formatTrialRemaining", () => {
  it("uses singular day for one day left", () => {
    expect(formatTrialRemaining(1)).toBe("1 day remaining");
  });

  it("uses plural days otherwise", () => {
    expect(formatTrialRemaining(7)).toBe("7 days remaining");
    expect(formatTrialRemaining(0)).toBe("0 days remaining");
  });
});

describe("getErrorMessage", () => {
  it("accepts errors and the plain strings Tauri commands reject with", () => {
    expect(getErrorMessage(new Error("boom"))).toBe("boom");
    expect(getErrorMessage("Photo is not part of the current library.")).toBe(
      "Photo is not part of the current library."
    );
  });
});
