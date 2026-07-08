import { app, BrowserWindow, dialog, ipcMain, net, protocol, shell } from "electron";
import { autoUpdater } from "electron-updater";
import crypto from "node:crypto";
import type { Dirent } from "node:fs";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import exifr from "exifr";
import sharp from "sharp";
import type {
  ExifRow,
  HomePayload,
  LibrarySummary,
  MonthPayload,
  MonthSummary,
  PhotoAsset,
  PhotoDetails,
  PhotoLocation,
  ScanProgress,
  Settings,
  SettingsPayload,
  UpdateStatus,
  YearPayload,
  YearSummary
} from "../shared/types";

const supportedExtensions = new Set([
  ".jpg",
  ".jpeg",
  ".jpe",
  ".jfif",
  ".png",
  ".webp",
  ".gif",
  ".bmp",
  ".tif",
  ".tiff",
  ".heic",
  ".heif",
  ".avif"
]);

const browserNativeExtensions = new Set([
  ".jpg",
  ".jpeg",
  ".jpe",
  ".jfif",
  ".png",
  ".webp",
  ".gif",
  ".bmp",
  ".avif"
]);

const thumbnailSize = 420;
const displayMaxDimension = 3840;
const imageCacheVersion = "v1";
const libraryIndexVersion = 1;
const githubLatestReleaseUrl = "https://api.github.com/repos/thedinz/GridMode/releases/latest";

interface PhotoFileSnapshot {
  path: string;
  size: number;
  mtimeMs: number;
}

interface LibraryIndexEntry {
  path: string;
  size: number;
  mtimeMs: number;
  photo: PhotoAsset;
}

interface LibraryIndexFile {
  version: number;
  rootDir?: string;
  rootDirs?: string[];
  excludedDirectories?: string[];
  scannedAt: string;
  warnings?: string[];
  photos: LibraryIndexEntry[];
}

const monthNames = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December"
];

protocol.registerSchemesAsPrivileged([
  {
    scheme: "gridmode-photo",
    privileges: {
      standard: true,
      secure: true,
      supportFetchAPI: true,
      stream: true
    }
  }
]);

let mainWindow: BrowserWindow | null = null;
let settings: Settings = {};
let cachedPhotos: PhotoAsset[] = [];
let cachedSummary: LibrarySummary = emptySummary();
let cachedPhotoStats = new Map<string, PhotoFileSnapshot>();
let hasLibraryIndex = false;
let activeScan: Promise<LibrarySummary> | null = null;
let updateDownloadInProgress = false;
let promptedDownloadVersion: string | undefined;
let promptedInstallVersion: string | undefined;
let manualMacUpdateStatus: UpdateStatus | undefined;
let activeImageJobs = 0;
const imageJobLimit = Math.max(2, Math.min(os.cpus().length - 1, 4));
const queuedImageJobs: Array<() => void> = [];
const pendingImageRenders = new Map<string, Promise<string>>();

if (process.env.GRIDMODE_USER_DATA_DIR) {
  app.setPath("userData", process.env.GRIDMODE_USER_DATA_DIR);
}

const gotLock = app.requestSingleInstanceLock();

if (!gotLock) {
  app.quit();
}

app.on("second-instance", () => {
  if (!mainWindow) {
    return;
  }
  if (mainWindow.isMinimized()) {
    mainWindow.restore();
  }
  mainWindow.focus();
});

app.whenReady().then(async () => {
  settings = await readSettings();
  await loadCachedLibrary(getPhotoDirectories(settings), settings.excludedDirectories ?? []);
  registerPhotoProtocol();
  registerIpc();
  configureUpdates();
  await createWindow();
  if (app.isPackaged) {
    setTimeout(() => {
      void checkForUpdates();
    }, 4500);
  }
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    app.quit();
  }
});

app.on("activate", () => {
  if (BrowserWindow.getAllWindows().length === 0) {
    void createWindow();
  }
});

async function createWindow(): Promise<void> {
  mainWindow = new BrowserWindow({
    width: 1320,
    height: 920,
    minWidth: 840,
    minHeight: 620,
    backgroundColor: "#151512",
    title: "GridMode",
    webPreferences: {
      preload: path.join(__dirname, "../preload/preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false
    }
  });

  if (app.isPackaged) {
    await mainWindow.loadFile(path.join(__dirname, "../renderer/index.html"));
  } else {
    await mainWindow.loadURL("http://127.0.0.1:5173");
    mainWindow.webContents.openDevTools({ mode: "detach" });
  }
}

function registerPhotoProtocol(): void {
  protocol.handle("gridmode-photo", async (request) => {
    const url = new URL(request.url);
    const variant = normalizePhotoVariant(url.hostname);
    const encodedPath = url.pathname.replace(/^\//, "");
    const filePath = decodePathToken(encodedPath);

    if (!variant) {
      return new Response("Unsupported photo request.", { status: 400 });
    }

    const photoDirectories = getPhotoDirectories(settings);
    if (photoDirectories.length === 0 || !isInsideAnyDirectory(photoDirectories, filePath)) {
      return new Response("Photo is outside the configured library.", { status: 403 });
    }

    if (isPathExcluded(filePath, settings.excludedDirectories ?? [])) {
      return new Response("Photo is in an excluded folder.", { status: 403 });
    }

    try {
      await fs.access(filePath);
      if (variant === "thumb") {
        return serveRenderedPhoto(filePath, "thumb");
      }

      if (variant === "display" && needsRenderedDisplay(filePath)) {
        return serveRenderedPhoto(filePath, "display");
      }

      return net.fetch(pathToFileURL(filePath).toString());
    } catch (error) {
      if (error instanceof UnsupportedImageError) {
        return new Response(error.message, { status: 415 });
      }
      if (isMissingFileError(error)) {
        return new Response("Photo could not be read.", { status: 404 });
      }
      return new Response(`Photo could not be rendered: ${getErrorMessage(error)}`, { status: 500 });
    }
  });
}

function registerIpc(): void {
  ipcMain.handle("settings:get", async (): Promise<SettingsPayload> => ({
    settings,
    summary: cachedSummary
  }));

  ipcMain.handle("settings:choose-root", async (): Promise<SettingsPayload> => {
    const result = await dialog.showOpenDialog({
      title: "Choose a photo folder",
      properties: ["openDirectory"]
    });

    if (result.canceled || result.filePaths.length === 0) {
      return {
        settings,
        summary: cachedSummary
      };
    }

    const selectedRoot = path.resolve(result.filePaths[0]);
    const previousPhotoDirectories = getPhotoDirectories(settings);
    const nextPhotoDirectories = previousPhotoDirectories.length > 0
      ? normalizePhotoDirectories([
          selectedRoot,
          ...previousPhotoDirectories.slice(1).filter((photoDirectory) => !sameDirectory(photoDirectory, selectedRoot))
        ])
      : [selectedRoot];
    settings = normalizeSettings({
      ...settings,
      photoDirectory: selectedRoot,
      photoDirectories: nextPhotoDirectories,
      excludedDirectories: previousPhotoDirectories.length > 0 ? settings.excludedDirectories : []
    });
    if (previousPhotoDirectories.length === 0) {
      cachedPhotos = [];
      cachedPhotoStats = new Map();
      cachedSummary = emptySummary(getPhotoDirectories(settings));
      hasLibraryIndex = false;
    }
    await writeSettings(settings);
    const summary = await scanLibrary(previousPhotoDirectories.length === 0);

    return {
      settings,
      summary
    };
  });

  ipcMain.handle("settings:add-root", async (): Promise<SettingsPayload> => {
    const photoDirectories = getPhotoDirectories(settings);
    const result = await dialog.showOpenDialog({
      title: "Add a photo folder",
      defaultPath: photoDirectories[0],
      properties: ["openDirectory"]
    });

    if (result.canceled || result.filePaths.length === 0) {
      return {
        settings,
        summary: cachedSummary
      };
    }

    const nextPhotoDirectories = normalizePhotoDirectories([
      ...photoDirectories,
      path.resolve(result.filePaths[0])
    ]);
    settings = normalizeSettings({
      ...settings,
      photoDirectory: nextPhotoDirectories[0],
      photoDirectories: nextPhotoDirectories
    });
    await writeSettings(settings);
    const summary = await scanLibrary(false);

    return {
      settings,
      summary
    };
  });

  ipcMain.handle("settings:remove-root", async (_event, rootPath: string): Promise<SettingsPayload> => {
    const nextPhotoDirectories = getPhotoDirectories(settings)
      .filter((photoDirectory) => !sameDirectory(photoDirectory, rootPath));

    settings = normalizeSettings({
      ...settings,
      photoDirectory: nextPhotoDirectories[0],
      photoDirectories: nextPhotoDirectories
    });
    await writeSettings(settings);

    if (nextPhotoDirectories.length === 0) {
      cachedPhotos = [];
      cachedPhotoStats = new Map();
      cachedSummary = emptySummary();
      hasLibraryIndex = false;
      return {
        settings,
        summary: cachedSummary
      };
    }

    const summary = await scanLibrary(false);
    return {
      settings,
      summary
    };
  });

  ipcMain.handle("settings:clear-cache", async (): Promise<SettingsPayload> => {
    await clearLibraryCache();
    const summary = await scanLibrary(true);

    return {
      settings,
      summary
    };
  });

  ipcMain.handle("settings:choose-exclusion", async (): Promise<SettingsPayload> => {
    const photoDirectories = getPhotoDirectories(settings);
    if (photoDirectories.length === 0) {
      return {
        settings,
        summary: cachedSummary
      };
    }

    const result = await dialog.showOpenDialog({
      title: "Exclude a folder",
      defaultPath: photoDirectories[0],
      properties: ["openDirectory"]
    });

    if (result.canceled || result.filePaths.length === 0) {
      return {
        settings,
        summary: cachedSummary
      };
    }

    const selectedPath = path.resolve(result.filePaths[0]);
    if (!isValidExcludedDirectory(photoDirectories, selectedPath)) {
      const message = {
        type: "warning",
        title: "Folder cannot be excluded",
        message: "Choose a subfolder inside one of the selected photo locations."
      } as const;
      if (mainWindow) {
        await dialog.showMessageBox(mainWindow, message);
      } else {
        await dialog.showMessageBox(message);
      }
      return {
        settings,
        summary: cachedSummary
      };
    }

    settings = {
      ...settings,
      excludedDirectories: addExcludedDirectory(photoDirectories, settings.excludedDirectories ?? [], selectedPath)
    };
    await writeSettings(settings);
    const summary = await scanLibrary(false);

    return {
      settings,
      summary
    };
  });

  ipcMain.handle("settings:remove-exclusion", async (_event, excludedPath: string): Promise<SettingsPayload> => {
    if (getPhotoDirectories(settings).length === 0) {
      return {
        settings,
        summary: cachedSummary
      };
    }

    settings = {
      ...settings,
      excludedDirectories: removeExcludedDirectory(settings.excludedDirectories ?? [], excludedPath)
    };
    await writeSettings(settings);
    const summary = await scanLibrary(false);

    return {
      settings,
      summary
    };
  });

  ipcMain.handle("library:scan", async (_event, force: boolean = false) => scanLibrary(force));

  ipcMain.handle("library:get-home", async (): Promise<HomePayload> => {
    const summary = await ensureLibrary();
    return {
      summary,
      photos: samplePhotos(cachedPhotos, 260)
    };
  });

  ipcMain.handle("library:get-years", async (): Promise<LibrarySummary> => {
    return ensureLibrary();
  });

  ipcMain.handle("library:get-year", async (_event, year: number): Promise<YearPayload> => {
    await ensureLibrary();
    const months = groupMonths(cachedPhotos.filter((photo) => photo.year === year));
    return {
      year,
      months
    };
  });

  ipcMain.handle(
    "library:get-month",
    async (_event, year: number, month: number): Promise<MonthPayload> => {
      await ensureLibrary();
      const photos = cachedPhotos
        .filter((photo) => photo.year === year && photo.month === month)
        .sort((a, b) => Date.parse(b.capturedAt) - Date.parse(a.capturedAt));

      return {
        year,
        month,
        monthName: monthNames[month - 1],
        photos
      };
    }
  );

  ipcMain.handle("photo:get-details", async (_event, photoPath: string): Promise<PhotoDetails> => {
    await ensureLibrary();
    const photo = cachedPhotos.find((item) => item.path === photoPath);
    if (!photo) {
      throw new Error("Photo is not part of the current library.");
    }

    const exif = await readDetailedExif(photo.path);
    return {
      photo,
      exif
    };
  });

  ipcMain.handle("updates:check", async () => checkForUpdates());
  ipcMain.handle("updates:download", async () => {
    if (!app.isPackaged) {
      const status: UpdateStatus = {
        state: "idle",
        message: "Updates are only available in installed builds."
      };
      sendUpdateStatus(status);
      return status;
    }
    if (process.platform === "darwin") {
      return openManualMacUpdateDownload();
    }
    await downloadAvailableUpdate();
    return { state: "downloading" } satisfies UpdateStatus;
  });
  ipcMain.on("updates:install", () => {
    if (app.isPackaged && process.platform !== "darwin") {
      autoUpdater.quitAndInstall();
    }
  });
}

async function ensureLibrary(): Promise<LibrarySummary> {
  const photoDirectories = getPhotoDirectories(settings);
  if (photoDirectories.length === 0) {
    cachedPhotos = [];
    cachedPhotoStats = new Map();
    cachedSummary = emptySummary();
    hasLibraryIndex = false;
    return cachedSummary;
  }

  if (!hasLibraryIndex && cachedPhotos.length === 0) {
    return scanLibrary(false);
  }

  return cachedSummary;
}

async function scanLibrary(force: boolean): Promise<LibrarySummary> {
  const photoDirectories = getPhotoDirectories(settings);
  if (photoDirectories.length === 0) {
    cachedPhotos = [];
    cachedPhotoStats = new Map();
    cachedSummary = emptySummary();
    hasLibraryIndex = false;
    return cachedSummary;
  }

  if (activeScan && !force) {
    return activeScan;
  }

  activeScan = doScan(photoDirectories, force);
  try {
    cachedSummary = await activeScan;
    settings = {
      ...settings,
      lastScanAt: cachedSummary.lastScanAt
    };
    await writeSettings(settings);
    return cachedSummary;
  } catch (error) {
    sendScanProgress({
      phase: "error",
      rootDir: formatRootSummary(photoDirectories),
      message: getErrorMessage(error)
    });
    throw error;
  } finally {
    activeScan = null;
  }
}

async function doScan(rootDirs: string[], force: boolean): Promise<LibrarySummary> {
  const warnings: string[] = [];
  const excludedDirectories = normalizeExcludedDirectories(rootDirs, settings.excludedDirectories ?? []);
  const emitProgress = createScanProgressEmitter(formatRootSummary(rootDirs) ?? "Photo library");

  emitProgress({
    phase: "discovering",
    foldersScanned: 0,
    photosFound: 0,
    foldersExcluded: 0,
    message: hasLibraryIndex && !force ? "Checking folders for changes" : "Finding photos"
  });

  const files = await findPhotoFiles(rootDirs, warnings, excludedDirectories, (progress) => {
    emitProgress({
      phase: "discovering",
      ...progress,
      message: hasLibraryIndex && !force ? "Checking folders for changes" : "Finding photos"
    });
  });

  const previousPhotos = force ? new Map<string, PhotoAsset>() : new Map(cachedPhotos.map((photo) => [photo.path, photo]));
  const previousStats = force ? new Map<string, PhotoFileSnapshot>() : new Map(cachedPhotoStats);
  const foundPaths = new Set(files.map((file) => file.path));
  const filesToIndex: PhotoFileSnapshot[] = [];
  const nextPhotos: PhotoAsset[] = [];
  const nextStats = new Map<string, PhotoFileSnapshot>();
  let reused = 0;

  for (const file of files) {
    const cachedPhoto = previousPhotos.get(file.path);
    const cachedStat = previousStats.get(file.path);
    if (cachedPhoto && cachedStat && isUnchangedFile(cachedStat, file)) {
      nextPhotos.push(refreshCachedPhotoAsset(cachedPhoto, file));
      nextStats.set(file.path, file);
      reused += 1;
    } else {
      filesToIndex.push(file);
    }
  }

  const removed = force ? 0 : Array.from(previousPhotos.keys()).filter((filePath) => !foundPaths.has(filePath)).length;
  let processed = 0;
  emitProgress({
    phase: "reading-metadata",
    photosFound: files.length,
    photosProcessed: processed,
    photosReused: reused,
    photosChanged: filesToIndex.length,
    photosRemoved: removed,
    totalPhotos: filesToIndex.length,
    message:
      files.length === 0
        ? "No supported photos found"
        : filesToIndex.length === 0
          ? "No photo metadata changes found"
          : "Reading metadata for new and changed photos"
  });

  const photos = await mapLimit(filesToIndex, Math.max(2, Math.min(os.cpus().length, 6)), async (file) => {
    try {
      const photo = await buildPhotoAsset(file.path, file);
      return {
        photo,
        file
      };
    } catch (error) {
      warnings.push(`${path.basename(file.path)}: ${getErrorMessage(error)}`);
      return null;
    } finally {
      processed += 1;
      emitProgress({
        phase: "reading-metadata",
        photosFound: files.length,
        photosProcessed: processed,
        photosReused: reused,
        photosChanged: filesToIndex.length,
        photosRemoved: removed,
        totalPhotos: filesToIndex.length,
        currentPath: file.path,
        message: "Reading metadata for new and changed photos"
      });
    }
  });

  for (const item of photos) {
    if (item) {
      nextPhotos.push(item.photo);
      nextStats.set(item.file.path, item.file);
    }
  }

  cachedPhotos = nextPhotos
    .sort((a, b) => Date.parse(b.capturedAt) - Date.parse(a.capturedAt));
  cachedPhotoStats = nextStats;

  const summary = buildSummary(rootDirs, cachedPhotos, warnings);
  cachedSummary = summary;
  hasLibraryIndex = true;
  await writeLibraryIndex(rootDirs, summary);

  emitProgress({
    phase: "complete",
    photosFound: files.length,
    photosProcessed: processed,
    photosReused: reused,
    photosChanged: filesToIndex.length,
    photosRemoved: removed,
    totalPhotos: filesToIndex.length,
    message: formatScanCompleteMessage(cachedPhotos.length, reused, filesToIndex.length, removed)
  }, true);

  return summary;
}

async function findPhotoFiles(
  rootDirs: string[],
  warnings: string[],
  excludedDirectories: string[],
  onProgress: (progress: Pick<ScanProgress, "foldersScanned" | "photosFound" | "foldersExcluded" | "currentPath">) => void
): Promise<PhotoFileSnapshot[]> {
  const found: PhotoFileSnapshot[] = [];
  const pending = [...rootDirs].reverse();
  let foldersScanned = 0;
  let foldersExcluded = 0;

  while (pending.length > 0) {
    const current = pending.pop();
    if (!current) {
      continue;
    }

    if (!isConfiguredRootDirectory(rootDirs, current) && isPathExcluded(current, excludedDirectories)) {
      foldersExcluded += 1;
      onProgress({
        foldersScanned,
        photosFound: found.length,
        foldersExcluded,
        currentPath: current
      });
      continue;
    }

    let entries: Dirent<string>[];
    try {
      entries = await fs.readdir(current, { withFileTypes: true, encoding: "utf8" });
      foldersScanned += 1;
    } catch (error) {
      warnings.push(`${current}: ${getErrorMessage(error)}`);
      continue;
    }

    for (const entry of entries) {
      const fullPath = path.join(current, entry.name);
      if (entry.isDirectory()) {
        if (isPathExcluded(fullPath, excludedDirectories)) {
          foldersExcluded += 1;
        } else {
          pending.push(fullPath);
        }
      } else if (entry.isFile() && supportedExtensions.has(path.extname(entry.name).toLowerCase())) {
        try {
          found.push(await readPhotoFileSnapshot(fullPath));
        } catch (error) {
          warnings.push(`${fullPath}: ${getErrorMessage(error)}`);
        }
      }
    }

    onProgress({
      foldersScanned,
      photosFound: found.length,
      foldersExcluded,
      currentPath: current
    });
  }

  return found;
}

function createScanProgressEmitter(rootDir: string): (progress: ScanProgress, immediate?: boolean) => void {
  let lastSentAt = 0;

  return (progress: ScanProgress, immediate = false) => {
    const now = Date.now();
    if (!immediate && now - lastSentAt < 250) {
      return;
    }

    lastSentAt = now;
    sendScanProgress({
      rootDir,
      ...progress
    });
  };
}

async function readPhotoFileSnapshot(filePath: string): Promise<PhotoFileSnapshot> {
  const stats = await fs.stat(filePath);
  return {
    path: filePath,
    size: stats.size,
    mtimeMs: Math.trunc(stats.mtimeMs)
  };
}

async function buildPhotoAsset(filePath: string, snapshot?: PhotoFileSnapshot): Promise<PhotoAsset> {
  const stats = snapshot ?? await readPhotoFileSnapshot(filePath);
  const parsed = await readFastExif(filePath);
  const capturedAt = parsed.capturedAt ?? new Date(stats.mtimeMs);
  const year = capturedAt.getFullYear();
  const month = capturedAt.getMonth() + 1;

  return {
    id: crypto.createHash("sha1").update(filePath).digest("hex").slice(0, 16),
    name: path.basename(filePath),
    path: filePath,
    directory: path.dirname(filePath),
    extension: path.extname(filePath).toLowerCase().replace(".", ""),
    size: stats.size,
    url: makePhotoUrl(filePath, "display"),
    thumbnailUrl: makePhotoUrl(filePath, "thumb"),
    capturedAt: capturedAt.toISOString(),
    dateSource: parsed.capturedAt ? "exif" : "file",
    year,
    month,
    monthName: monthNames[month - 1],
    location: parsed.location,
    width: parsed.width,
    height: parsed.height
  };
}

function isUnchangedFile(previous: PhotoFileSnapshot, current: PhotoFileSnapshot): boolean {
  return previous.size === current.size && previous.mtimeMs === current.mtimeMs;
}

function refreshCachedPhotoAsset(photo: PhotoAsset, snapshot: PhotoFileSnapshot): PhotoAsset {
  return {
    ...photo,
    name: path.basename(snapshot.path),
    path: snapshot.path,
    directory: path.dirname(snapshot.path),
    extension: path.extname(snapshot.path).toLowerCase().replace(".", ""),
    size: snapshot.size,
    url: makePhotoUrl(snapshot.path, "display"),
    thumbnailUrl: makePhotoUrl(snapshot.path, "thumb")
  };
}

async function readFastExif(filePath: string): Promise<{
  capturedAt?: Date;
  location?: PhotoLocation;
  width?: number;
  height?: number;
}> {
  try {
    const metadata = await exifr.parse(filePath, {
      exif: true,
      gps: true,
      xmp: true,
      translateKeys: true,
      translateValues: true,
      reviveValues: true,
      pick: [
        "DateTimeOriginal",
        "CreateDate",
        "ModifyDate",
        "DateCreated",
        "GPSLatitude",
        "GPSLongitude",
        "latitude",
        "longitude",
        "ExifImageWidth",
        "ExifImageHeight",
        "ImageWidth",
        "ImageHeight"
      ]
    });

    return {
      capturedAt: normalizeDate(metadata?.DateTimeOriginal ?? metadata?.CreateDate ?? metadata?.DateCreated),
      location: normalizeLocation(metadata),
      width: normalizeNumber(metadata?.ExifImageWidth ?? metadata?.ImageWidth),
      height: normalizeNumber(metadata?.ExifImageHeight ?? metadata?.ImageHeight)
    };
  } catch {
    return {};
  }
}

async function readDetailedExif(filePath: string): Promise<ExifRow[]> {
  const rows: ExifRow[] = [];

  try {
    const metadata = await exifr.parse(filePath, {
      exif: true,
      gps: true,
      xmp: true,
      iptc: true,
      translateKeys: true,
      translateValues: true,
      reviveValues: true
    });

    const wanted: Array<[string, unknown]> = [
      ["Taken", metadata?.DateTimeOriginal ?? metadata?.CreateDate ?? metadata?.DateCreated],
      ["Camera", joinParts(metadata?.Make, metadata?.Model)],
      ["Lens", metadata?.LensModel],
      ["Exposure", metadata?.ExposureTime ? `${metadata.ExposureTime}s` : undefined],
      ["Aperture", metadata?.FNumber ? `f/${metadata.FNumber}` : undefined],
      ["ISO", metadata?.ISO],
      ["Focal Length", metadata?.FocalLength ? `${metadata.FocalLength} mm` : undefined],
      ["Dimensions", joinDimensions(metadata?.ExifImageWidth ?? metadata?.ImageWidth, metadata?.ExifImageHeight ?? metadata?.ImageHeight)],
      ["Location", formatLocation(normalizeLocation(metadata))],
      ["Software", metadata?.Software],
      ["Artist", metadata?.Artist],
      ["Copyright", metadata?.Copyright]
    ];

    for (const [label, value] of wanted) {
      const formatted = formatExifValue(value);
      if (formatted) {
        rows.push({ label, value: formatted });
      }
    }
  } catch {
    rows.push({
      label: "Metadata",
      value: "No readable EXIF metadata found."
    });
  }

  return rows;
}

function buildSummary(
  rootDirs: string[],
  photos: PhotoAsset[],
  warnings: string[],
  lastScanAt: string = new Date().toISOString()
): LibrarySummary {
  const years = groupYears(photos);
  return {
    rootDir: formatRootSummary(rootDirs),
    rootDirs,
    photoCount: photos.length,
    years,
    lastScanAt,
    warnings: warnings.slice(0, 20)
  };
}

function formatScanCompleteMessage(total: number, reused: number, changed: number, removed: number): string {
  if (changed === 0 && removed === 0 && reused > 0) {
    return `Library up to date - reused ${reused.toLocaleString()} cached photos`;
  }

  const parts = [`Indexed ${total.toLocaleString()} photos`];
  if (reused > 0) {
    parts.push(`${reused.toLocaleString()} cached`);
  }
  if (changed > 0) {
    parts.push(`${changed.toLocaleString()} new or changed`);
  }
  if (removed > 0) {
    parts.push(`${removed.toLocaleString()} removed`);
  }
  return parts.join(" - ");
}

function emptySummary(rootDirs: string[] = []): LibrarySummary {
  return {
    rootDir: formatRootSummary(rootDirs),
    rootDirs,
    photoCount: 0,
    years: [],
    warnings: []
  };
}

function groupYears(photos: PhotoAsset[]): YearSummary[] {
  const groups = new Map<number, PhotoAsset[]>();
  for (const photo of photos) {
    groups.set(photo.year, [...(groups.get(photo.year) ?? []), photo]);
  }

  return Array.from(groups.entries())
    .sort(([a], [b]) => b - a)
    .map(([year, yearPhotos]) => ({
      year,
      count: yearPhotos.length,
      sample: samplePhotos(yearPhotos, 20)
    }));
}

function groupMonths(photos: PhotoAsset[]): MonthSummary[] {
  const groups = new Map<number, PhotoAsset[]>();
  for (const photo of photos) {
    groups.set(photo.month, [...(groups.get(photo.month) ?? []), photo]);
  }

  return Array.from(groups.entries())
    .sort(([a], [b]) => b - a)
    .map(([month, monthPhotos]) => ({
      year: monthPhotos[0]?.year ?? new Date().getFullYear(),
      month,
      monthName: monthNames[month - 1],
      count: monthPhotos.length,
      sample: samplePhotos(monthPhotos, 20)
    }));
}

function samplePhotos(photos: PhotoAsset[], count: number): PhotoAsset[] {
  if (photos.length <= count) {
    return shuffle([...photos]);
  }
  return shuffle([...photos]).slice(0, count);
}

function shuffle<T>(items: T[]): T[] {
  for (let index = items.length - 1; index > 0; index -= 1) {
    const nextIndex = Math.floor(Math.random() * (index + 1));
    [items[index], items[nextIndex]] = [items[nextIndex], items[index]];
  }
  return items;
}

async function mapLimit<T, R>(
  items: T[],
  limit: number,
  mapper: (item: T) => Promise<R>
): Promise<R[]> {
  const results = new Array<R>(items.length);
  let cursor = 0;

  async function worker(): Promise<void> {
    while (cursor < items.length) {
      const index = cursor;
      cursor += 1;
      results[index] = await mapper(items[index]);
    }
  }

  await Promise.all(Array.from({ length: limit }, () => worker()));
  return results;
}

class UnsupportedImageError extends Error {
  constructor(filePath: string, reason: string) {
    super(`${path.basename(filePath)} is not readable by GridMode's image renderer: ${reason}`);
  }
}

function normalizePhotoVariant(value: string): "image" | "display" | "thumb" | undefined {
  if (value === "image" || value === "display" || value === "thumb") {
    return value;
  }
  return undefined;
}

function needsRenderedDisplay(filePath: string): boolean {
  return !browserNativeExtensions.has(path.extname(filePath).toLowerCase());
}

async function serveRenderedPhoto(filePath: string, variant: "display" | "thumb"): Promise<Response> {
  const cachedPath = await getCachedRender(filePath, variant);
  const bytes = await fs.readFile(cachedPath);

  return new Response(new Uint8Array(bytes), {
    headers: {
      "content-type": "image/webp",
      "cache-control": "public, max-age=31536000, immutable"
    }
  });
}

async function getCachedRender(filePath: string, variant: "display" | "thumb"): Promise<string> {
  const stats = await fs.stat(filePath);
  const outputPath = getImageCachePath(filePath, stats, variant);

  try {
    await fs.access(outputPath);
    return outputPath;
  } catch {
    // Cache miss, fall through and render.
  }

  const existingRender = pendingImageRenders.get(outputPath);
  if (existingRender) {
    return existingRender;
  }

  const renderPromise = runImageJob(async () => {
    await fs.mkdir(path.dirname(outputPath), { recursive: true });
    await renderImage(filePath, outputPath, variant);
    return outputPath;
  }).finally(() => {
    pendingImageRenders.delete(outputPath);
  });

  pendingImageRenders.set(outputPath, renderPromise);
  return renderPromise;
}

function getImageCachePath(
  filePath: string,
  stats: { size: number; mtimeMs: number },
  variant: "display" | "thumb"
): string {
  const hash = crypto
    .createHash("sha1")
    .update(`${imageCacheVersion}\0${variant}\0${filePath}\0${stats.size}\0${Math.trunc(stats.mtimeMs)}`)
    .digest("hex");

  return path.join(app.getPath("userData"), "image-cache", variant, hash.slice(0, 2), `${hash}.webp`);
}

async function renderImage(filePath: string, outputPath: string, variant: "display" | "thumb"): Promise<void> {
  try {
    const image = sharp(filePath, {
      failOn: "none",
      pages: 1
    }).rotate();

    if (variant === "thumb") {
      await image
        .resize({
          width: thumbnailSize,
          height: thumbnailSize,
          fit: "cover",
          position: "attention",
          withoutEnlargement: true
        })
        .toColorspace("srgb")
        .webp({
          quality: 78,
          effort: 4
        })
        .toFile(outputPath);
      return;
    }

    await image
      .resize({
        width: displayMaxDimension,
        height: displayMaxDimension,
        fit: "inside",
        withoutEnlargement: true
      })
      .toColorspace("srgb")
      .webp({
        quality: 88,
        effort: 4
      })
      .toFile(outputPath);
  } catch (error) {
    throw new UnsupportedImageError(filePath, getErrorMessage(error));
  }
}

function runImageJob<T>(job: () => Promise<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const start = () => {
      activeImageJobs += 1;
      job()
        .then(resolve, reject)
        .finally(() => {
          activeImageJobs -= 1;
          queuedImageJobs.shift()?.();
        });
    };

    if (activeImageJobs < imageJobLimit) {
      start();
      return;
    }

    queuedImageJobs.push(start);
  });
}

function makePhotoUrl(filePath: string, variant: "display" | "thumb" | "image" = "image"): string {
  return `gridmode-photo://${variant}/${encodePathToken(filePath)}`;
}

function encodePathToken(filePath: string): string {
  return Buffer.from(filePath, "utf8").toString("base64url");
}

function decodePathToken(token: string): string {
  return Buffer.from(token, "base64url").toString("utf8");
}

function isInsideDirectory(rootDir: string, filePath: string): boolean {
  const relative = path.relative(path.resolve(rootDir), path.resolve(filePath));
  return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}

function isInsideAnyDirectory(rootDirs: string[], filePath: string): boolean {
  return rootDirs.some((rootDir) => isInsideDirectory(rootDir, filePath));
}

function isConfiguredRootDirectory(rootDirs: string[], candidatePath: string): boolean {
  return rootDirs.some((rootDir) => sameDirectory(rootDir, candidatePath));
}

function isValidExcludedDirectory(rootDirs: string[], candidatePath: string): boolean {
  return isInsideAnyDirectory(rootDirs, candidatePath) && !isConfiguredRootDirectory(rootDirs, candidatePath);
}

function normalizePhotoDirectories(photoDirectories: string[] = []): string[] {
  const normalized = photoDirectories
    .filter((item): item is string => typeof item === "string" && item.trim().length > 0)
    .map((item) => path.resolve(item));

  const compacted: string[] = [];
  for (const item of normalized) {
    if (compacted.some((existing) => sameDirectory(existing, item) || isInsideDirectory(existing, item))) {
      continue;
    }

    for (let index = compacted.length - 1; index >= 0; index -= 1) {
      if (isInsideDirectory(item, compacted[index])) {
        compacted.splice(index, 1);
      }
    }
    compacted.push(item);
  }

  return compacted;
}

function getPhotoDirectories(nextSettings: Settings): string[] {
  return normalizePhotoDirectories([
    nextSettings.photoDirectory,
    ...(nextSettings.photoDirectories ?? [])
  ].filter((item): item is string => typeof item === "string"));
}

function normalizeExcludedDirectories(rootDirs: string[], excludedDirectories: string[] = []): string[] {
  if (rootDirs.length === 0) {
    return [];
  }

  const normalized = excludedDirectories
    .filter((item): item is string => typeof item === "string" && item.trim().length > 0)
    .map((item) => path.resolve(item))
    .filter((item) => isValidExcludedDirectory(rootDirs, item))
    .sort((a, b) => a.length - b.length || comparePath(a, b));

  const compacted: string[] = [];
  for (const item of normalized) {
    if (!compacted.some((existing) => sameDirectory(existing, item) || isInsideDirectory(existing, item))) {
      compacted.push(item);
    }
  }

  return compacted;
}

function addExcludedDirectory(rootDirs: string[], existing: string[], candidatePath: string): string[] {
  return normalizeExcludedDirectories(rootDirs, [...existing, candidatePath]);
}

function removeExcludedDirectory(existing: string[], candidatePath: string): string[] {
  const normalizedCandidate = path.resolve(candidatePath);
  return existing.filter((item) => !sameDirectory(path.resolve(item), normalizedCandidate));
}

function isPathExcluded(filePath: string, excludedDirectories: string[]): boolean {
  return excludedDirectories.some((excludedDirectory) => isInsideDirectory(excludedDirectory, filePath));
}

function formatRootSummary(rootDirs: string[]): string | undefined {
  if (rootDirs.length === 0) {
    return undefined;
  }
  if (rootDirs.length === 1) {
    return rootDirs[0];
  }
  return `${rootDirs.length.toLocaleString()} photo locations`;
}

function normalizeDate(value: unknown): Date | undefined {
  if (value instanceof Date && !Number.isNaN(value.getTime())) {
    return value;
  }
  if (typeof value === "string" || typeof value === "number") {
    const parsed = new Date(value);
    if (!Number.isNaN(parsed.getTime())) {
      return parsed;
    }
  }
  return undefined;
}

function normalizeLocation(metadata: Record<string, unknown> | undefined): PhotoLocation | undefined {
  const latitude = normalizeNumber(metadata?.latitude ?? metadata?.GPSLatitude);
  const longitude = normalizeNumber(metadata?.longitude ?? metadata?.GPSLongitude);
  if (latitude === undefined || longitude === undefined) {
    return undefined;
  }
  return {
    latitude,
    longitude
  };
}

function normalizeNumber(value: unknown): number | undefined {
  if (typeof value === "number" && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === "string") {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) {
      return parsed;
    }
  }
  return undefined;
}

function formatLocation(location: PhotoLocation | undefined): string | undefined {
  if (!location) {
    return undefined;
  }
  return `${location.latitude.toFixed(5)}, ${location.longitude.toFixed(5)}`;
}

function joinParts(...parts: unknown[]): string | undefined {
  const text = parts
    .map((part) => formatExifValue(part))
    .filter(Boolean)
    .join(" ");
  return text || undefined;
}

function joinDimensions(width: unknown, height: unknown): string | undefined {
  const normalizedWidth = normalizeNumber(width);
  const normalizedHeight = normalizeNumber(height);
  if (!normalizedWidth || !normalizedHeight) {
    return undefined;
  }
  return `${normalizedWidth} x ${normalizedHeight}`;
}

function formatExifValue(value: unknown): string | undefined {
  if (value === undefined || value === null || value === "") {
    return undefined;
  }
  if (value instanceof Date) {
    return value.toLocaleString();
  }
  if (Array.isArray(value)) {
    return value.map((item) => formatExifValue(item)).filter(Boolean).join(", ");
  }
  if (typeof value === "object") {
    return undefined;
  }
  return String(value);
}

function isMissingFileError(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    ((error as { code?: unknown }).code === "ENOENT" || (error as { code?: unknown }).code === "EACCES")
  );
}

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function loadCachedLibrary(rootDirs: string[], excludedDirectories: string[] = []): Promise<void> {
  cachedPhotos = [];
  cachedPhotoStats = new Map();
  cachedSummary = emptySummary(rootDirs);
  hasLibraryIndex = false;

  if (rootDirs.length === 0) {
    return;
  }

  try {
    const index = parseJson(await fs.readFile(libraryIndexPath(), "utf8")) as LibraryIndexFile;
    if (!isUsableLibraryIndex(index, rootDirs, excludedDirectories)) {
      return;
    }

    const entries = index.photos.filter((entry) => isUsableLibraryIndexEntry(entry, rootDirs));
    cachedPhotos = entries
      .map((entry) => refreshCachedPhotoAsset(entry.photo, entry))
      .sort((a, b) => Date.parse(b.capturedAt) - Date.parse(a.capturedAt));
    cachedPhotoStats = new Map(entries.map((entry) => [
      entry.path,
      {
        path: entry.path,
        size: entry.size,
        mtimeMs: entry.mtimeMs
      }
    ]));
    cachedSummary = buildSummary(rootDirs, cachedPhotos, index.warnings ?? [], index.scannedAt);
    hasLibraryIndex = true;
  } catch {
    cachedSummary = emptySummary(rootDirs);
  }
}

async function writeLibraryIndex(rootDirs: string[], summary: LibrarySummary): Promise<void> {
  const index: LibraryIndexFile = {
    version: libraryIndexVersion,
    rootDir: rootDirs[0],
    rootDirs,
    excludedDirectories: normalizeExcludedDirectories(rootDirs, settings.excludedDirectories ?? []),
    scannedAt: summary.lastScanAt ?? new Date().toISOString(),
    warnings: summary.warnings,
    photos: cachedPhotos.flatMap((photo) => {
      const stat = cachedPhotoStats.get(photo.path);
      return stat
        ? [
            {
              path: photo.path,
              size: stat.size,
              mtimeMs: stat.mtimeMs,
              photo
            }
          ]
        : [];
    })
  };

  await fs.mkdir(app.getPath("userData"), { recursive: true });
  const target = libraryIndexPath();
  const temp = `${target}.tmp`;
  await fs.writeFile(temp, JSON.stringify(index), "utf8");
  await fs.rename(temp, target);
}

function isUsableLibraryIndex(index: LibraryIndexFile, rootDirs: string[], excludedDirectories: string[]): boolean {
  const indexRootDirs = normalizePhotoDirectories([
    ...(index.rootDir ? [index.rootDir] : []),
    ...(index.rootDirs ?? [])
  ]);

  return (
    index.version === libraryIndexVersion &&
    sameDirectories(indexRootDirs, rootDirs) &&
    sameDirectories(
      normalizeExcludedDirectories(rootDirs, index.excludedDirectories ?? []),
      normalizeExcludedDirectories(rootDirs, excludedDirectories)
    ) &&
    typeof index.scannedAt === "string" &&
    Array.isArray(index.photos)
  );
}

function isUsableLibraryIndexEntry(entry: LibraryIndexEntry, rootDirs: string[]): boolean {
  return (
    typeof entry.path === "string" &&
    typeof entry.size === "number" &&
    typeof entry.mtimeMs === "number" &&
    typeof entry.photo?.capturedAt === "string" &&
    supportedExtensions.has(path.extname(entry.path).toLowerCase()) &&
    isInsideAnyDirectory(rootDirs, entry.path)
  );
}

function sameDirectory(left: string, right: string): boolean {
  const first = path.resolve(left);
  const second = path.resolve(right);
  return process.platform === "win32" ? first.toLowerCase() === second.toLowerCase() : first === second;
}

function comparePath(left: string, right: string): number {
  const first = process.platform === "win32" ? left.toLowerCase() : left;
  const second = process.platform === "win32" ? right.toLowerCase() : right;
  return first.localeCompare(second);
}

function sameDirectories(left: string[], right: string[]): boolean {
  return left.length === right.length && left.every((item, index) => sameDirectory(item, right[index]));
}

function settingsPath(): string {
  return path.join(app.getPath("userData"), "settings.json");
}

function libraryIndexPath(): string {
  return path.join(app.getPath("userData"), "library-index.json");
}

function imageCachePath(): string {
  return path.join(app.getPath("userData"), "image-cache");
}

async function clearLibraryCache(): Promise<void> {
  if (activeScan) {
    await activeScan;
  }

  const pendingRenders = Array.from(pendingImageRenders.values());
  if (pendingRenders.length > 0) {
    await Promise.allSettled(pendingRenders);
  }

  pendingImageRenders.clear();
  cachedPhotos = [];
  cachedPhotoStats = new Map();
  cachedSummary = emptySummary(getPhotoDirectories(settings));
  hasLibraryIndex = false;

  await Promise.all([
    fs.rm(imageCachePath(), { recursive: true, force: true }),
    fs.rm(libraryIndexPath(), { force: true })
  ]);
}

async function readSettings(): Promise<Settings> {
  try {
    const text = await fs.readFile(settingsPath(), "utf8");
    return normalizeSettings(parseJson(text) as Settings);
  } catch {
    return {};
  }
}

function normalizeSettings(rawSettings: Settings): Settings {
  const photoDirectories = normalizePhotoDirectories([
    rawSettings.photoDirectory,
    ...(rawSettings.photoDirectories ?? [])
  ].filter((item): item is string => typeof item === "string"));

  return {
    ...rawSettings,
    photoDirectory: photoDirectories[0],
    photoDirectories,
    excludedDirectories: normalizeExcludedDirectories(photoDirectories, rawSettings.excludedDirectories ?? [])
  };
}

function parseJson(text: string): unknown {
  return JSON.parse(text.replace(/^\uFEFF/, ""));
}

async function writeSettings(nextSettings: Settings): Promise<void> {
  const normalized = normalizeSettings(nextSettings);
  await fs.mkdir(app.getPath("userData"), { recursive: true });
  await fs.writeFile(settingsPath(), JSON.stringify(normalized, null, 2), "utf8");
}

function configureUpdates(): void {
  autoUpdater.autoDownload = false;
  autoUpdater.autoInstallOnAppQuit = false;

  autoUpdater.on("checking-for-update", () => {
    sendUpdateStatus({ state: "checking", message: "Checking for updates..." });
  });
  autoUpdater.on("update-available", (info) => {
    sendUpdateStatus({
      state: "available",
      version: info.version,
      message: `GridMode ${info.version} is available.`
    });
    void promptToDownloadUpdate(info.version);
  });
  autoUpdater.on("update-not-available", () => {
    sendUpdateStatus({
      state: "not-available",
      message: "GridMode is up to date."
    });
  });
  autoUpdater.on("download-progress", (progress) => {
    sendUpdateStatus({
      state: "downloading",
      percent: progress.percent,
      message: `Downloading update ${Math.round(progress.percent)}%`
    });
  });
  autoUpdater.on("update-downloaded", (info) => {
    updateDownloadInProgress = false;
    sendUpdateStatus({
      state: "downloaded",
      version: info.version,
      message: `GridMode ${info.version} is ready to install.`
    });
    void promptToInstallUpdate(info.version);
  });
  autoUpdater.on("error", (error) => {
    updateDownloadInProgress = false;
    sendUpdateStatus({
      state: "error",
      message: getErrorMessage(error)
    });
  });
}

async function checkForUpdates(): Promise<UpdateStatus> {
  if (!app.isPackaged) {
    const status: UpdateStatus = {
      state: "idle",
      message: "Updates are only available in installed builds."
    };
    sendUpdateStatus(status);
    return status;
  }

  if (process.platform === "darwin") {
    return checkForManualMacUpdate();
  }

  try {
    sendUpdateStatus({ state: "checking", message: "Checking for updates..." });
    await autoUpdater.checkForUpdates();
    return { state: "checking" };
  } catch (error) {
    const status: UpdateStatus = {
      state: "error",
      message: getErrorMessage(error)
    };
    sendUpdateStatus(status);
    return status;
  }
}

async function checkForManualMacUpdate(): Promise<UpdateStatus> {
  try {
    sendUpdateStatus({ state: "checking", message: "Checking for updates..." });
    const response = await net.fetch(githubLatestReleaseUrl, {
      headers: {
        accept: "application/vnd.github+json",
        "user-agent": `GridMode/${app.getVersion()}`
      }
    });

    if (!response.ok) {
      throw new Error(`GitHub returned ${response.status}`);
    }

    const release = (await response.json()) as GitHubRelease;
    const version = normalizeReleaseVersion(release);
    if (!version || !isNewerVersion(version, app.getVersion())) {
      const status: UpdateStatus = {
        state: "not-available",
        message: "GridMode is up to date."
      };
      manualMacUpdateStatus = undefined;
      sendUpdateStatus(status);
      return status;
    }

    const downloadUrl = findMacDownloadUrl(release) ?? getString(release.html_url);
    const status: UpdateStatus = {
      state: "available",
      version,
      downloadUrl,
      manualDownload: true,
      message: downloadUrl
        ? `GridMode ${version} is available. Download the Mac installer to update manually.`
        : `GridMode ${version} is available. Open GitHub Releases to download it.`
    };
    manualMacUpdateStatus = status;
    sendUpdateStatus(status);
    return status;
  } catch (error) {
    const status: UpdateStatus = {
      state: "error",
      message: getErrorMessage(error)
    };
    sendUpdateStatus(status);
    return status;
  }
}

async function openManualMacUpdateDownload(): Promise<UpdateStatus> {
  const status = manualMacUpdateStatus ?? await checkForManualMacUpdate();
  if (status.state !== "available" || !status.downloadUrl) {
    return status;
  }

  await shell.openExternal(status.downloadUrl);
  const nextStatus: UpdateStatus = {
    ...status,
    message: status.version
      ? `Opening GridMode ${status.version} download...`
      : "Opening GridMode download..."
  };
  sendUpdateStatus(nextStatus);
  return nextStatus;
}

interface GitHubReleaseAsset {
  name?: unknown;
  browser_download_url?: unknown;
}

interface GitHubRelease {
  tag_name?: unknown;
  name?: unknown;
  html_url?: unknown;
  assets?: unknown;
}

function normalizeReleaseVersion(release: GitHubRelease): string | undefined {
  const value = getString(release.tag_name) ?? getString(release.name);
  return value?.replace(/^v/i, "");
}

function findMacDownloadUrl(release: GitHubRelease): string | undefined {
  if (!Array.isArray(release.assets)) {
    return undefined;
  }

  const assets = release.assets.filter(isGitHubReleaseAsset);
  const dmg = assets.find((asset) => {
    const name = getString(asset.name)?.toLowerCase();
    return Boolean(name?.includes("mac") && name.endsWith(".dmg"));
  });
  return getString(dmg?.browser_download_url);
}

function isGitHubReleaseAsset(value: unknown): value is GitHubReleaseAsset {
  return typeof value === "object" && value !== null;
}

function isNewerVersion(candidate: string, current: string): boolean {
  const candidateParts = parseVersionParts(candidate);
  const currentParts = parseVersionParts(current);
  const length = Math.max(candidateParts.length, currentParts.length);
  for (let index = 0; index < length; index += 1) {
    const left = candidateParts[index] ?? 0;
    const right = currentParts[index] ?? 0;
    if (left > right) {
      return true;
    }
    if (left < right) {
      return false;
    }
  }
  return false;
}

function parseVersionParts(value: string): number[] {
  return value
    .replace(/^v/i, "")
    .split(/[.-]/)
    .map((part) => Number.parseInt(part, 10))
    .filter((part) => Number.isFinite(part));
}

function getString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim().length > 0 ? value : undefined;
}

function sendUpdateStatus(status: UpdateStatus): void {
  mainWindow?.webContents.send("updates:status", status);
}

function sendScanProgress(progress: ScanProgress): void {
  mainWindow?.webContents.send("scan:progress", progress);
}

async function promptToDownloadUpdate(version: string | undefined): Promise<void> {
  if (!mainWindow || promptedDownloadVersion === version) {
    return;
  }

  promptedDownloadVersion = version;
  const result = await dialog.showMessageBox(mainWindow, {
    type: "info",
    buttons: ["Download", "Later"],
    defaultId: 0,
    cancelId: 1,
    title: "GridMode update",
    message: version ? `GridMode ${version} is available.` : "A GridMode update is available.",
    detail: "Download the update now?"
  });

  if (result.response === 0) {
    await downloadAvailableUpdate();
  }
}

async function downloadAvailableUpdate(): Promise<void> {
  if (updateDownloadInProgress) {
    return;
  }

  updateDownloadInProgress = true;
  sendUpdateStatus({ state: "downloading", message: "Downloading update..." });
  try {
    await autoUpdater.downloadUpdate();
  } catch (error) {
    updateDownloadInProgress = false;
    throw error;
  }
}

async function promptToInstallUpdate(version: string | undefined): Promise<void> {
  if (!mainWindow || promptedInstallVersion === version) {
    return;
  }

  promptedInstallVersion = version;
  const result = await dialog.showMessageBox(mainWindow, {
    type: "info",
    buttons: ["Restart and install", "Later"],
    defaultId: 0,
    cancelId: 1,
    title: "GridMode update ready",
    message: version ? `GridMode ${version} is ready to install.` : "A GridMode update is ready to install.",
    detail: "Restart GridMode to finish installing the update?"
  });

  if (result.response === 0) {
    autoUpdater.quitAndInstall();
  }
}
