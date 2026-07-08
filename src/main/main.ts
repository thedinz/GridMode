import { app, BrowserWindow, dialog, ipcMain, net, protocol } from "electron";
import { autoUpdater } from "electron-updater";
import crypto from "node:crypto";
import type { Dirent } from "node:fs";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import exifr from "exifr";
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
  UpdateStatus,
  YearPayload,
  YearSummary
} from "../shared/types";

const supportedExtensions = new Set([
  ".jpg",
  ".jpeg",
  ".png",
  ".webp",
  ".gif",
  ".bmp",
  ".tif",
  ".tiff",
  ".heic",
  ".heif"
]);

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
let activeScan: Promise<LibrarySummary> | null = null;
let updateDownloadInProgress = false;
let promptedDownloadVersion: string | undefined;
let promptedInstallVersion: string | undefined;

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
  cachedSummary = emptySummary(settings.photoDirectory);
  registerPhotoProtocol();
  registerIpc();
  configureUpdates();
  await createWindow();
  if (settings.photoDirectory) {
    void scanLibrary(false);
  }
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
    const encodedPath = url.pathname.replace(/^\//, "");
    const filePath = decodePathToken(encodedPath);

    if (!settings.photoDirectory || !isInsideDirectory(settings.photoDirectory, filePath)) {
      return new Response("Photo is outside the configured library.", { status: 403 });
    }

    try {
      await fs.access(filePath);
      return net.fetch(pathToFileURL(filePath).toString());
    } catch {
      return new Response("Photo could not be read.", { status: 404 });
    }
  });
}

function registerIpc(): void {
  ipcMain.handle("settings:get", async () => ({
    settings,
    summary: cachedSummary
  }));

  ipcMain.handle("settings:choose-root", async () => {
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

    settings = {
      photoDirectory: result.filePaths[0]
    };
    await writeSettings(settings);
    const summary = await scanLibrary(true);

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
    await downloadAvailableUpdate();
    return { state: "downloading" } satisfies UpdateStatus;
  });
  ipcMain.on("updates:install", () => {
    if (app.isPackaged) {
      autoUpdater.quitAndInstall();
    }
  });
}

async function ensureLibrary(): Promise<LibrarySummary> {
  if (!settings.photoDirectory) {
    cachedPhotos = [];
    cachedSummary = emptySummary();
    return cachedSummary;
  }

  if (cachedPhotos.length === 0) {
    return scanLibrary(false);
  }

  return cachedSummary;
}

async function scanLibrary(force: boolean): Promise<LibrarySummary> {
  if (!settings.photoDirectory) {
    cachedPhotos = [];
    cachedSummary = emptySummary();
    return cachedSummary;
  }

  if (activeScan && !force) {
    return activeScan;
  }

  activeScan = doScan(settings.photoDirectory);
  try {
    cachedSummary = await activeScan;
    settings = {
      ...settings,
      lastScanAt: cachedSummary.lastScanAt
    };
    await writeSettings(settings);
    return cachedSummary;
  } finally {
    activeScan = null;
  }
}

async function doScan(rootDir: string): Promise<LibrarySummary> {
  const warnings: string[] = [];
  const emitProgress = createScanProgressEmitter(rootDir);

  emitProgress({
    phase: "discovering",
    foldersScanned: 0,
    photosFound: 0,
    message: "Finding photos"
  });

  const files = await findPhotoFiles(rootDir, warnings, (progress) => {
    emitProgress({
      phase: "discovering",
      ...progress,
      message: "Finding photos"
    });
  });

  let processed = 0;
  emitProgress({
    phase: "reading-metadata",
    photosFound: files.length,
    photosProcessed: processed,
    totalPhotos: files.length,
    message: files.length === 0 ? "No supported photos found" : "Reading dates and metadata"
  });

  const photos = await mapLimit(files, Math.max(2, Math.min(os.cpus().length, 6)), async (filePath) => {
    try {
      return await buildPhotoAsset(filePath);
    } catch (error) {
      warnings.push(`${path.basename(filePath)}: ${getErrorMessage(error)}`);
      return null;
    } finally {
      processed += 1;
      emitProgress({
        phase: "reading-metadata",
        photosFound: files.length,
        photosProcessed: processed,
        totalPhotos: files.length,
        currentPath: filePath,
        message: "Reading dates and metadata"
      });
    }
  });

  cachedPhotos = photos
    .filter((photo): photo is PhotoAsset => photo !== null)
    .sort((a, b) => Date.parse(b.capturedAt) - Date.parse(a.capturedAt));

  const summary = buildSummary(rootDir, cachedPhotos, warnings);
  emitProgress({
    phase: "complete",
    photosFound: files.length,
    photosProcessed: processed,
    totalPhotos: files.length,
    message: `Indexed ${cachedPhotos.length.toLocaleString()} photos`
  }, true);

  return summary;
}

async function findPhotoFiles(
  rootDir: string,
  warnings: string[],
  onProgress: (progress: Pick<ScanProgress, "foldersScanned" | "photosFound" | "currentPath">) => void
): Promise<string[]> {
  const found: string[] = [];
  const pending = [rootDir];
  let foldersScanned = 0;

  while (pending.length > 0) {
    const current = pending.pop();
    if (!current) {
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
        pending.push(fullPath);
      } else if (entry.isFile() && supportedExtensions.has(path.extname(entry.name).toLowerCase())) {
        found.push(fullPath);
      }
    }

    onProgress({
      foldersScanned,
      photosFound: found.length,
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

async function buildPhotoAsset(filePath: string): Promise<PhotoAsset> {
  const stats = await fs.stat(filePath);
  const parsed = await readFastExif(filePath);
  const capturedAt = parsed.capturedAt ?? stats.mtime;
  const year = capturedAt.getFullYear();
  const month = capturedAt.getMonth() + 1;

  return {
    id: crypto.createHash("sha1").update(filePath).digest("hex").slice(0, 16),
    name: path.basename(filePath),
    path: filePath,
    directory: path.dirname(filePath),
    extension: path.extname(filePath).toLowerCase().replace(".", ""),
    size: stats.size,
    url: makePhotoUrl(filePath),
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

function buildSummary(rootDir: string | undefined, photos: PhotoAsset[], warnings: string[]): LibrarySummary {
  const years = groupYears(photos);
  return {
    rootDir,
    photoCount: photos.length,
    years,
    lastScanAt: new Date().toISOString(),
    warnings: warnings.slice(0, 20)
  };
}

function emptySummary(rootDir?: string): LibrarySummary {
  return {
    rootDir,
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

function makePhotoUrl(filePath: string): string {
  return `gridmode-photo://image/${encodePathToken(filePath)}`;
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

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function settingsPath(): string {
  return path.join(app.getPath("userData"), "settings.json");
}

async function readSettings(): Promise<Settings> {
  try {
    const text = await fs.readFile(settingsPath(), "utf8");
    return JSON.parse(text) as Settings;
  } catch {
    return {};
  }
}

async function writeSettings(nextSettings: Settings): Promise<void> {
  await fs.mkdir(app.getPath("userData"), { recursive: true });
  await fs.writeFile(settingsPath(), JSON.stringify(nextSettings, null, 2), "utf8");
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
