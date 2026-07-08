import {
  CalendarDays,
  CheckCircle2,
  Download,
  FolderOpen,
  Grid2X2,
  Image,
  Library,
  RefreshCcw,
  Settings,
  Sparkles
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { GridModeLogo } from "./components/GridModeLogo";
import type {
  ExifRow,
  HomePayload,
  LibrarySummary,
  MonthPayload,
  MonthSummary,
  PhotoAsset,
  PhotoDetails,
  ScanProgress,
  Settings as AppSettings,
  UpdateStatus,
  YearPayload
} from "../shared/types";

type View =
  | { name: "home" }
  | { name: "library" }
  | { name: "year"; year: number }
  | { name: "month"; year: number; month: number }
  | { name: "photo"; photo: PhotoAsset; previous: View }
  | { name: "settings" };

interface AppState {
  settings: AppSettings;
  summary: LibrarySummary;
  homePhotos: PhotoAsset[];
  year?: YearPayload;
  month?: MonthPayload;
  details?: PhotoDetails;
  scanProgress?: ScanProgress;
  loading: boolean;
  statusText?: string;
}

const initialSummary: LibrarySummary = {
  photoCount: 0,
  years: [],
  warnings: []
};

interface LoadHomeOptions {
  label?: string;
  rootDir?: string;
  scanMessage?: string;
}

export function App(): JSX.Element {
  const [view, setView] = useState<View>({ name: "home" });
  const [state, setState] = useState<AppState>({
    settings: {},
    summary: initialSummary,
    homePhotos: [],
    loading: true
  });
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>({ state: "idle" });

  const mergeState = useCallback((patch: Partial<AppState>) => {
    setState((current) => ({
      ...current,
      ...patch
    }));
  }, []);

  const loadHome = useCallback(async (options: LoadHomeOptions = {}) => {
    mergeState({
      loading: true,
      statusText: options.label ?? "Refreshing grid",
      scanProgress: options.scanMessage
        ? {
            phase: "discovering",
            rootDir: options.rootDir,
            message: options.scanMessage
          }
        : undefined
    });
    try {
      const payload: HomePayload = await window.gridMode.library.getHome();
      mergeState({
        summary: payload.summary,
        homePhotos: payload.photos,
        loading: false,
        statusText: undefined,
        scanProgress: undefined
      });
    } catch (error) {
      mergeState({
        loading: false,
        statusText: getErrorMessage(error),
        scanProgress: undefined
      });
    }
  }, [mergeState]);

  const openView = useCallback(
    async (nextView: View) => {
      setView(nextView);

      if (nextView.name === "home") {
        await loadHome();
      } else if (nextView.name === "library") {
        mergeState({ loading: true, statusText: "Loading library" });
        const summary = await window.gridMode.library.getYears();
        mergeState({ summary, loading: false, statusText: undefined });
      } else if (nextView.name === "year") {
        mergeState({ loading: true, statusText: `Loading ${nextView.year}` });
        const year = await window.gridMode.library.getYear(nextView.year);
        mergeState({ year, loading: false, statusText: undefined });
      } else if (nextView.name === "month") {
        mergeState({
          loading: true,
          statusText: `Loading ${monthName(nextView.month)} ${nextView.year}`
        });
        const month = await window.gridMode.library.getMonth(nextView.year, nextView.month);
        mergeState({ month, loading: false, statusText: undefined });
      } else if (nextView.name === "photo") {
        mergeState({ loading: true, statusText: "Reading metadata" });
        const details = await window.gridMode.photo.getDetails(nextView.photo.path);
        mergeState({ details, loading: false, statusText: undefined });
      }
    },
    [loadHome, mergeState]
  );

  useEffect(() => {
    let mounted = true;

    window.gridMode.settings
      .get()
      .then(async ({ settings, summary }) => {
        if (!mounted) {
          return;
        }
        mergeState({ settings, summary });
        if (settings.photoDirectory) {
          await loadHome({
            label: "Checking library",
            rootDir: settings.photoDirectory,
            scanMessage: "Checking library for changes"
          });
        } else {
          setView({ name: "home" });
          mergeState({ loading: false, statusText: undefined });
        }
      })
      .catch((error) => {
        if (mounted) {
          mergeState({ loading: false, statusText: getErrorMessage(error) });
        }
      });

    const unsubscribeUpdates = window.gridMode.updates.onStatus(setUpdateStatus);
    const unsubscribeScan = window.gridMode.library.onProgress((progress) => {
      mergeState({
        scanProgress: progress,
        statusText: progress.message
      });
    });

    return () => {
      mounted = false;
      unsubscribeUpdates();
      unsubscribeScan();
    };
  }, [loadHome, mergeState]);

  const chooseRoot = useCallback(async () => {
    mergeState({
      loading: true,
      statusText: "Scanning selected folder",
      scanProgress: {
        phase: "discovering",
        message: "Waiting for folder selection"
      }
    });
    const { settings, summary } = await window.gridMode.settings.chooseRoot();
    mergeState({
      settings,
      summary,
      loading: false,
      statusText: undefined,
      scanProgress: undefined
    });
    if (settings.photoDirectory) {
      await openView({ name: "home" });
    }
  }, [mergeState, openView]);

  const rescan = useCallback(async () => {
    mergeState({
      loading: true,
      statusText: "Rescanning library",
      scanProgress: {
        phase: "discovering",
        rootDir: state.settings.photoDirectory,
        message: "Finding photos"
      }
    });
    const summary = await window.gridMode.library.scan(true);
    mergeState({ summary, loading: false, statusText: undefined, scanProgress: undefined });
    if (view.name === "home") {
      await loadHome();
    }
  }, [loadHome, mergeState, state.settings.photoDirectory, view.name]);

  const openPhoto = useCallback(
    (photo: PhotoAsset) => {
      void openView({ name: "photo", photo, previous: view });
    },
    [openView, view]
  );

  const content = useMemo(() => {
    const isScanningLibrary = state.loading && Boolean(state.scanProgress);

    if (!state.settings.photoDirectory) {
      return (
        <FirstRunView
          onChooseRoot={chooseRoot}
          updateStatus={updateStatus}
          onCheckUpdates={() => void window.gridMode.updates.check()}
        />
      );
    }

    if (view.name === "settings") {
      return (
        <SettingsView
          settings={state.settings}
          summary={state.summary}
          onChooseRoot={chooseRoot}
          onRescan={rescan}
          updateStatus={updateStatus}
          onCheckUpdates={() => void window.gridMode.updates.check()}
        />
      );
    }

    if (view.name === "library") {
      return (
        <LibraryView
          summary={state.summary}
          onOpenYear={(year) => void openView({ name: "year", year })}
          onOpenPhoto={openPhoto}
        />
      );
    }

    if (view.name === "year") {
      return (
        <YearView
          payload={state.year}
          onOpenMonth={(month) => void openView({ name: "month", year: view.year, month })}
          onOpenPhoto={openPhoto}
        />
      );
    }

    if (view.name === "month") {
      return (
        <MonthView
          payload={state.month}
          onOpenPhoto={openPhoto}
        />
      );
    }

    if (view.name === "photo") {
      return (
        <PhotoView
          details={state.details}
          fallbackPhoto={view.photo}
          onBack={() => void openView(view.previous)}
        />
      );
    }

    return (
      <HomeView
        photos={state.homePhotos}
        summary={state.summary}
        onOpenPhoto={openPhoto}
        onRefresh={() => void loadHome()}
        isScanning={isScanningLibrary}
      />
    );
  }, [
    chooseRoot,
    loadHome,
    openPhoto,
    openView,
    rescan,
    state.details,
    state.homePhotos,
    state.settings,
    state.summary,
    state.year,
    state.month,
    updateStatus,
    view
  ]);

  const hasPhotoDirectory = Boolean(state.settings.photoDirectory);

  return (
    <div className={hasPhotoDirectory ? "app-shell" : "app-shell first-run-shell"}>
      {hasPhotoDirectory ? (
        <TopBar
          view={view}
          summary={state.summary}
          onHome={() => void openView({ name: "home" })}
          onLibrary={() => void openView({ name: "library" })}
          onSettings={() => setView({ name: "settings" })}
          onRefresh={view.name === "home" ? () => void loadHome() : rescan}
        />
      ) : null}
      <UpdateBanner
        status={updateStatus}
      />
      <main className="app-main">
        {state.loading ? (
          <LoadingOverlay
            label={state.statusText ?? "Loading"}
            progress={state.scanProgress}
          />
        ) : null}
        {content}
      </main>
    </div>
  );
}

function TopBar({
  view,
  summary,
  onHome,
  onLibrary,
  onSettings,
  onRefresh
}: {
  view: View;
  summary: LibrarySummary;
  onHome: () => void;
  onLibrary: () => void;
  onSettings: () => void;
  onRefresh: () => void;
}): JSX.Element {
  return (
    <header className="top-bar">
      <div className="brand-mark">
        <Grid2X2 size={22} />
        <span>GridMode</span>
      </div>
      <nav className="nav-group">
        <button
          className={view.name === "home" ? "nav-button active" : "nav-button"}
          onClick={onHome}
          title="Grid"
        >
          <Sparkles size={18} />
          <span>Grid</span>
        </button>
        <button
          className={view.name === "library" || view.name === "year" || view.name === "month" ? "nav-button active" : "nav-button"}
          onClick={onLibrary}
          title="Library"
        >
          <Library size={18} />
          <span>Library</span>
        </button>
      </nav>
      <div className="toolbar">
        <span className="photo-count">{summary.photoCount.toLocaleString()} photos</span>
        <button
          className="icon-button"
          onClick={onRefresh}
          title="Refresh"
        >
          <RefreshCcw size={18} />
        </button>
        <button
          className="icon-button"
          onClick={onSettings}
          title="Settings"
        >
          <Settings size={18} />
        </button>
      </div>
    </header>
  );
}

function UpdateBanner({ status }: { status: UpdateStatus }): JSX.Element | null {
  if (status.state === "idle" || status.state === "not-available") {
    return null;
  }

  const canDownload = status.state === "available";
  const canInstall = status.state === "downloaded";

  return (
    <aside className={`update-banner ${status.state}`}>
      <div>
        <strong>{status.version ? `Update ${status.version}` : "GridMode Update"}</strong>
        <span>{status.message}</span>
      </div>
      {canDownload ? (
        <button
          className="text-button"
          onClick={() => void window.gridMode.updates.download()}
        >
          <Download size={16} />
          <span>Download</span>
        </button>
      ) : null}
      {canInstall ? (
        <button
          className="text-button"
          onClick={() => window.gridMode.updates.install()}
        >
          <CheckCircle2 size={16} />
          <span>Install</span>
        </button>
      ) : null}
    </aside>
  );
}

function HomeView({
  photos,
  summary,
  onOpenPhoto,
  onRefresh,
  isScanning = false
}: {
  photos: PhotoAsset[];
  summary: LibrarySummary;
  onOpenPhoto: (photo: PhotoAsset) => void;
  onRefresh: () => void;
  isScanning?: boolean;
}): JSX.Element {
  if (isScanning && summary.photoCount === 0) {
    return (
      <section className="quiet-state">
        <RefreshCcw size={40} />
        <h1>Checking library</h1>
      </section>
    );
  }

  if (summary.photoCount === 0) {
    return (
      <section className="quiet-state">
        <Image size={40} />
        <h1>No photos found</h1>
      </section>
    );
  }

  return (
    <section className="view-stack">
      <div className="view-heading">
        <div>
          <p>Random grid</p>
          <h1>{summary.rootDir}</h1>
        </div>
        <button
          className="text-button"
          onClick={onRefresh}
        >
          <RefreshCcw size={16} />
          <span>Shuffle</span>
        </button>
      </div>
      <PhotoGrid
        photos={photos}
        onOpenPhoto={onOpenPhoto}
      />
    </section>
  );
}

function LibraryView({
  summary,
  onOpenYear,
  onOpenPhoto
}: {
  summary: LibrarySummary;
  onOpenYear: (year: number) => void;
  onOpenPhoto: (photo: PhotoAsset) => void;
}): JSX.Element {
  return (
    <section className="view-stack">
      <div className="view-heading">
        <div>
          <p>Library</p>
          <h1>{summary.years.length} years</h1>
        </div>
      </div>
      <div className="section-list">
        {summary.years.map((year) => (
          <PhotoSection
            key={year.year}
            title={String(year.year)}
            count={year.count}
            photos={year.sample}
            onOpenSection={() => onOpenYear(year.year)}
            onOpenPhoto={onOpenPhoto}
          />
        ))}
      </div>
    </section>
  );
}

function YearView({
  payload,
  onOpenMonth,
  onOpenPhoto
}: {
  payload?: YearPayload;
  onOpenMonth: (month: number) => void;
  onOpenPhoto: (photo: PhotoAsset) => void;
}): JSX.Element {
  return (
    <section className="view-stack">
      <div className="view-heading">
        <div>
          <p>Year</p>
          <h1>{payload?.year ?? ""}</h1>
        </div>
      </div>
      <div className="section-list">
        {(payload?.months ?? []).map((month) => (
          <PhotoSection
            key={month.month}
            title={month.monthName}
            count={month.count}
            photos={month.sample}
            onOpenSection={() => onOpenMonth(month.month)}
            onOpenPhoto={onOpenPhoto}
          />
        ))}
      </div>
    </section>
  );
}

function MonthView({
  payload,
  onOpenPhoto
}: {
  payload?: MonthPayload;
  onOpenPhoto: (photo: PhotoAsset) => void;
}): JSX.Element {
  return (
    <section className="view-stack">
      <div className="view-heading">
        <div>
          <p>{payload?.year}</p>
          <h1>{payload?.monthName}</h1>
        </div>
      </div>
      <PhotoGrid
        photos={payload?.photos ?? []}
        onOpenPhoto={onOpenPhoto}
      />
    </section>
  );
}

function PhotoView({
  details,
  fallbackPhoto,
  onBack
}: {
  details?: PhotoDetails;
  fallbackPhoto: PhotoAsset;
  onBack: () => void;
}): JSX.Element {
  const photo = details?.photo ?? fallbackPhoto;
  return (
    <section className="photo-view">
      <button
        className="photo-stage"
        onClick={onBack}
        title="Back"
      >
        <img
          src={photo.url}
          alt={photo.name}
        />
      </button>
      <div className="metadata-panel">
        <div className="metadata-title">
          <h1>{photo.name}</h1>
          <span>{formatDate(photo.capturedAt)}</span>
        </div>
        <MetadataRows
          rows={details?.exif ?? []}
          photo={photo}
        />
      </div>
    </section>
  );
}

function SettingsView({
  settings,
  summary,
  updateStatus,
  onChooseRoot,
  onRescan,
  onCheckUpdates
}: {
  settings: AppSettings;
  summary: LibrarySummary;
  updateStatus: UpdateStatus;
  onChooseRoot: () => void;
  onRescan: () => void;
  onCheckUpdates: () => void;
}): JSX.Element {
  return (
    <section className="settings-view">
      <div className="settings-panel">
        <div className="view-heading">
          <div>
            <p>Settings</p>
            <h1>Photo directory</h1>
          </div>
        </div>
        <div className="setting-row">
          <span>{settings.photoDirectory ?? "No folder selected"}</span>
          <button
            className="text-button"
            onClick={onChooseRoot}
          >
            <FolderOpen size={16} />
            <span>Choose</span>
          </button>
        </div>
        <div className="settings-metrics">
          <Metric
            label="Photos"
            value={summary.photoCount.toLocaleString()}
          />
          <Metric
            label="Years"
            value={summary.years.length.toLocaleString()}
          />
          <Metric
            label="Scanned"
            value={summary.lastScanAt ? formatDate(summary.lastScanAt) : "Never"}
          />
        </div>
        <div className="settings-actions">
          <button
            className="text-button"
            onClick={onRescan}
          >
            <RefreshCcw size={16} />
            <span>Rescan</span>
          </button>
          <button
            className="text-button"
            onClick={onCheckUpdates}
          >
            <Download size={16} />
            <span>Check updates</span>
          </button>
        </div>
        {updateStatus.message ? <p className="settings-note">{updateStatus.message}</p> : null}
      </div>
    </section>
  );
}

function FirstRunView({
  onChooseRoot,
  updateStatus,
  onCheckUpdates
}: {
  onChooseRoot: () => void;
  updateStatus: UpdateStatus;
  onCheckUpdates: () => void;
}): JSX.Element {
  return (
    <section className="first-run-view">
      <div>
        <GridModeLogo />
        <div className="first-run-actions">
          <button
            className="text-button primary"
            onClick={onChooseRoot}
          >
            <FolderOpen size={18} />
            <span>Choose photo folder</span>
          </button>
          <button
            className="text-button"
            onClick={onCheckUpdates}
          >
            <Download size={18} />
            <span>Check updates</span>
          </button>
        </div>
        {updateStatus.message ? <p className="settings-note">{updateStatus.message}</p> : null}
      </div>
    </section>
  );
}

function PhotoSection({
  title,
  count,
  photos,
  onOpenSection,
  onOpenPhoto
}: {
  title: string;
  count: number;
  photos: PhotoAsset[];
  onOpenSection: () => void;
  onOpenPhoto: (photo: PhotoAsset) => void;
}): JSX.Element {
  return (
    <section className="photo-section">
      <button
        className="section-heading"
        onClick={onOpenSection}
      >
        <span>{title}</span>
        <small>{count.toLocaleString()} photos</small>
      </button>
      <PhotoStrip
        photos={photos}
        onOpenPhoto={onOpenPhoto}
      />
    </section>
  );
}

function PhotoGrid({
  photos,
  onOpenPhoto
}: {
  photos: PhotoAsset[];
  onOpenPhoto: (photo: PhotoAsset) => void;
}): JSX.Element {
  return (
    <div className="photo-grid">
      {photos.map((photo) => (
        <PhotoTile
          key={photo.id}
          photo={photo}
          onOpen={() => onOpenPhoto(photo)}
        />
      ))}
    </div>
  );
}

function PhotoStrip({
  photos,
  onOpenPhoto
}: {
  photos: PhotoAsset[];
  onOpenPhoto: (photo: PhotoAsset) => void;
}): JSX.Element {
  return (
    <div className="photo-strip">
      {photos.map((photo) => (
        <PhotoTile
          key={photo.id}
          photo={photo}
          onOpen={() => onOpenPhoto(photo)}
        />
      ))}
    </div>
  );
}

function PhotoTile({ photo, onOpen }: { photo: PhotoAsset; onOpen: () => void }): JSX.Element {
  return (
    <button
      className="photo-tile"
      onClick={onOpen}
      title={photo.name}
    >
      <img
        src={photo.thumbnailUrl}
        alt={photo.name}
        loading="lazy"
        decoding="async"
      />
      <span>
        <CalendarDays size={13} />
        {photo.year}
      </span>
    </button>
  );
}

function MetadataRows({ rows, photo }: { rows: ExifRow[]; photo: PhotoAsset }): JSX.Element {
  const fallbackRows: ExifRow[] = [
    {
      label: "Taken",
      value: `${formatDate(photo.capturedAt)} (${photo.dateSource})`
    },
    {
      label: "Folder",
      value: photo.directory
    },
    {
      label: "File size",
      value: formatBytes(photo.size)
    }
  ];

  const allRows = rows.length > 0 ? rows : fallbackRows;

  return (
    <dl className="metadata-grid">
      {allRows.map((row) => (
        <div key={`${row.label}-${row.value}`}>
          <dt>{row.label}</dt>
          <dd>{row.value}</dd>
        </div>
      ))}
      {rows.length > 0 ? (
        <>
          <div>
            <dt>Folder</dt>
            <dd>{photo.directory}</dd>
          </div>
          <div>
            <dt>File size</dt>
            <dd>{formatBytes(photo.size)}</dd>
          </div>
        </>
      ) : null}
    </dl>
  );
}

function Metric({ label, value }: { label: string; value: string }): JSX.Element {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function LoadingOverlay({ label, progress }: { label: string; progress?: ScanProgress }): JSX.Element {
  const percent =
    progress?.phase === "reading-metadata" && progress.totalPhotos
      ? Math.round(((progress.photosProcessed ?? 0) / progress.totalPhotos) * 100)
      : undefined;

  return (
    <div className="loading-overlay">
      <RefreshCcw size={22} />
      <div>
        <span>{label}</span>
        {progress ? <small>{formatScanProgress(progress, percent)}</small> : null}
      </div>
    </div>
  );
}

function formatScanProgress(progress: ScanProgress, percent: number | undefined): string {
  if (progress.phase === "discovering") {
    const folders = progress.foldersScanned ?? 0;
    const photos = progress.photosFound ?? 0;
    return `${folders.toLocaleString()} folders checked - ${photos.toLocaleString()} photos found`;
  }

  if (progress.phase === "reading-metadata") {
    const processed = progress.photosProcessed ?? 0;
    const total = progress.totalPhotos ?? progress.photosFound ?? 0;
    const suffix = percent === undefined ? "" : ` - ${percent}%`;
    return `${processed.toLocaleString()} / ${total.toLocaleString()} photos processed${suffix}`;
  }

  if (progress.phase === "complete") {
    return progress.message ?? "Scan complete";
  }

  return progress.currentPath ?? "";
}

function monthName(month: number): string {
  return new Date(2024, month - 1, 1).toLocaleString(undefined, { month: "long" });
}

function formatDate(value: string): string {
  return new Date(value).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric"
  });
}

function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = bytes;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
