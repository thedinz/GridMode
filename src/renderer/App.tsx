import {
  CalendarDays,
  CheckCircle2,
  ChevronLeft,
  Download,
  FolderOpen,
  FolderX,
  Grid2X2,
  Image,
  Library,
  RefreshCcw,
  Settings,
  Sparkles,
  Trash2,
  X
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { GridModeLogo } from "./components/GridModeLogo";
import { gridModeApi } from "./gridModeApi";
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
  settingsStatusText?: string;
}

const initialSummary: LibrarySummary = {
  photoCount: 0,
  years: [],
  warnings: []
};

const initialMonthPhotoLimit = 240;
const monthPhotoLimitIncrement = 240;
const photoSectionPreviewLimit = 12;
const initialGridRenderCount = 96;
const gridRenderBatchSize = 64;
const eagerGridThumbnailCount = 48;
const thumbnailLoadRootMargin = "700px 0px";
const gridRenderRootMargin = "1000px 0px";
const automaticUpdateCheckDelayMs = 4500;

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
      const payload: HomePayload = await gridModeApi.library.getHome();
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

  const checkLibraryForChanges = useCallback(async (rootDir: string) => {
    mergeState({
      loading: true,
      statusText: "Checking library",
      scanProgress: {
        phase: "discovering",
        rootDir,
        message: "Checking folders for changes"
      }
    });

    try {
      await gridModeApi.library.scan(false);
      const payload = await gridModeApi.library.getHome();
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

  const refreshHome = useCallback(async () => {
    const [rootDir] = getPhotoDirectories(state.settings);
    if (rootDir) {
      await checkLibraryForChanges(rootDir);
    } else {
      await loadHome();
    }
  }, [checkLibraryForChanges, loadHome, state.settings]);

  const openView = useCallback(
    async (nextView: View) => {
      setView(nextView);

      if (nextView.name === "home") {
        await loadHome();
      } else if (nextView.name === "library") {
        mergeState({ loading: true, statusText: "Loading library" });
        const summary = await gridModeApi.library.getYears();
        mergeState({ summary, loading: false, statusText: undefined });
      } else if (nextView.name === "year") {
        mergeState({ loading: true, statusText: `Loading ${nextView.year}` });
        const year = await gridModeApi.library.getYear(nextView.year);
        mergeState({ year, loading: false, statusText: undefined });
      } else if (nextView.name === "month") {
        mergeState({
          loading: true,
          statusText: `Loading ${monthName(nextView.month)} ${nextView.year}`
        });
        const month = await gridModeApi.library.getMonth(nextView.year, nextView.month);
        mergeState({ month, loading: false, statusText: undefined });
      } else if (nextView.name === "photo") {
        mergeState({ loading: true, statusText: "Reading metadata" });
        const details = await gridModeApi.photo.getDetails(nextView.photo.path);
        mergeState({ details, loading: false, statusText: undefined });
      }
    },
    [loadHome, mergeState]
  );

  useEffect(() => {
    let mounted = true;

    gridModeApi.settings
      .get()
      .then(async ({ settings, summary }) => {
        if (!mounted) {
          return;
        }
        mergeState({ settings, summary });
        const photoDirectories = getPhotoDirectories(settings);
        if (photoDirectories.length > 0) {
          const hasCachedLibrary = summary.photoCount > 0;
          if (hasCachedLibrary) {
            await loadHome({ label: "Loading cached library" });
            if (mounted) {
              void checkLibraryForChanges(photoDirectories[0]);
            }
          } else {
            await loadHome({
              label: "Building library cache",
              rootDir: photoDirectories[0],
              scanMessage: "Building library cache"
            });
          }
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

    const unsubscribeUpdates = gridModeApi.updates.onStatus(setUpdateStatus);
    const unsubscribeScan = gridModeApi.library.onProgress((progress) => {
      mergeState({
        scanProgress: progress,
        statusText: progress.message
      });
    });
    const updateCheckTimer = window.setTimeout(() => {
      void gridModeApi.updates.check({ automatic: true }).then((status) => {
        if (status.state !== "idle" && status.state !== "not-available") {
          setUpdateStatus(status);
        }
      });
    }, automaticUpdateCheckDelayMs);

    return () => {
      mounted = false;
      window.clearTimeout(updateCheckTimer);
      unsubscribeUpdates();
      unsubscribeScan();
    };
  }, [checkLibraryForChanges, loadHome, mergeState]);

  const chooseRoot = useCallback(async () => {
    mergeState({
      loading: true,
      statusText: "Scanning selected folder",
      scanProgress: {
        phase: "discovering",
        message: "Waiting for folder selection"
      }
    });
    const { settings, summary } = await gridModeApi.settings.chooseRoot();
    mergeState({
      settings,
      summary,
      loading: false,
      statusText: undefined,
      scanProgress: undefined
    });
    if (getPhotoDirectories(settings).length > 0) {
      await openView({ name: "home" });
    }
  }, [mergeState, openView]);

  const addPhotoDirectory = useCallback(async () => {
    mergeState({
      loading: true,
      statusText: "Adding photo location",
      scanProgress: {
        phase: "discovering",
        rootDir: state.settings.photoDirectory,
        message: "Waiting for folder selection"
      }
    });
    const { settings, summary } = await gridModeApi.settings.addRoot();
    mergeState({
      settings,
      summary,
      loading: false,
      statusText: undefined,
      scanProgress: undefined
    });
    if (view.name === "home") {
      await loadHome();
    }
  }, [loadHome, mergeState, state.settings.photoDirectory, view.name]);

  const removePhotoDirectory = useCallback(async (rootPath: string) => {
    mergeState({
      loading: true,
      statusText: "Removing photo location",
      scanProgress: {
        phase: "discovering",
        rootDir: state.settings.photoDirectory,
        message: "Checking folders for changes"
      }
    });
    const { settings, summary } = await gridModeApi.settings.removeRoot(rootPath);
    mergeState({
      settings,
      summary,
      loading: false,
      statusText: undefined,
      scanProgress: undefined
    });
    if (getPhotoDirectories(settings).length > 0 && view.name === "home") {
      await loadHome();
    }
  }, [loadHome, mergeState, state.settings.photoDirectory, view.name]);

  const chooseExclusion = useCallback(async () => {
    mergeState({
      loading: true,
      statusText: "Updating exclusions",
      scanProgress: {
        phase: "discovering",
        rootDir: state.settings.photoDirectory,
        message: "Waiting for folder selection"
      }
    });
    const { settings, summary } = await gridModeApi.settings.chooseExclusion();
    mergeState({
      settings,
      summary,
      loading: false,
      statusText: undefined,
      scanProgress: undefined
    });
  }, [mergeState, state.settings.photoDirectory]);

  const removeExclusion = useCallback(async (excludedPath: string) => {
    mergeState({
      loading: true,
      statusText: "Updating exclusions",
      scanProgress: {
        phase: "discovering",
        rootDir: state.settings.photoDirectory,
        message: "Checking folders for changes"
      }
    });
    const { settings, summary } = await gridModeApi.settings.removeExclusion(excludedPath);
    mergeState({
      settings,
      summary,
      loading: false,
      statusText: undefined,
      scanProgress: undefined
    });
  }, [mergeState, state.settings.photoDirectory]);

  const reloadCurrentLibraryView = useCallback(async (summary: LibrarySummary): Promise<Partial<AppState>> => {
    if (view.name === "home") {
      const payload = await gridModeApi.library.getHome();
      return {
        summary: payload.summary,
        homePhotos: payload.photos
      };
    }

    if (view.name === "year") {
      return {
        summary,
        year: await gridModeApi.library.getYear(view.year)
      };
    }

    if (view.name === "month") {
      return {
        summary,
        month: await gridModeApi.library.getMonth(view.year, view.month)
      };
    }

    if (view.name === "photo") {
      return {
        summary,
        details: await gridModeApi.photo.getDetails(view.photo.path)
      };
    }

    return { summary };
  }, [view]);

  const rescan = useCallback(async () => {
    mergeState({
      loading: true,
      statusText: "Rescanning library",
      settingsStatusText: undefined,
      scanProgress: {
        phase: "discovering",
        rootDir: getPhotoDirectories(state.settings)[0],
        message: "Finding photos"
      }
    });

    try {
      const summary = await gridModeApi.library.scan(true);
      const viewPatch = await reloadCurrentLibraryView(summary);
      mergeState({
        ...viewPatch,
        loading: false,
        statusText: undefined,
        scanProgress: undefined,
        settingsStatusText: formatLibraryActionComplete("Rescan complete", viewPatch.summary ?? summary)
      });
    } catch (error) {
      mergeState({
        loading: false,
        statusText: undefined,
        scanProgress: undefined,
        settingsStatusText: `Rescan failed: ${getErrorMessage(error)}`
      });
    }
  }, [mergeState, reloadCurrentLibraryView, state.settings]);

  const clearCache = useCallback(async () => {
    mergeState({
      loading: true,
      statusText: "Clearing cache",
      settingsStatusText: undefined,
      scanProgress: undefined
    });

    try {
      const { settings, summary } = await gridModeApi.settings.clearCache();
      mergeState({
        settings,
        summary,
        loading: false,
        statusText: undefined,
        scanProgress: undefined,
        settingsStatusText: "Image cache cleared. Thumbnails will be recreated on the next scan or as needed."
      });
    } catch (error) {
      mergeState({
        loading: false,
        statusText: undefined,
        scanProgress: undefined,
        settingsStatusText: `Clear cache failed: ${getErrorMessage(error)}`
      });
    }
  }, [mergeState]);

  const rebuildThumbnails = useCallback(async () => {
    mergeState({
      loading: true,
      statusText: "Rebuilding thumbnails",
      settingsStatusText: undefined,
      scanProgress: {
        phase: "generating-thumbnails",
        rootDir: getPhotoDirectories(state.settings)[0],
        photosProcessed: 0,
        totalPhotos: state.summary.photoCount,
        message: "Preparing thumbnail cache"
      }
    });

    try {
      const { settings, summary, thumbnails } = await gridModeApi.settings.rebuildThumbnails();
      const ready = thumbnails.generated + thumbnails.reused;
      const failureText = thumbnails.failed > 0
        ? ` ${thumbnails.failed.toLocaleString()} could not be generated; see library warnings for details.`
        : "";
      mergeState({
        settings,
        summary,
        loading: false,
        statusText: undefined,
        scanProgress: undefined,
        settingsStatusText: `${ready.toLocaleString()} thumbnails ready.${failureText}`
      });
    } catch (error) {
      mergeState({
        loading: false,
        statusText: undefined,
        scanProgress: undefined,
        settingsStatusText: `Thumbnail rebuild failed: ${getErrorMessage(error)}`
      });
    }
  }, [mergeState, state.settings, state.summary.photoCount]);

  const openPhoto = useCallback(
    (photo: PhotoAsset) => {
      void openView({ name: "photo", photo, previous: view });
    },
    [openView, view]
  );

  const content = useMemo(() => {
    const isScanningLibrary = state.loading && Boolean(state.scanProgress);

    if (getPhotoDirectories(state.settings).length === 0) {
      return (
        <FirstRunView
          onChooseRoot={chooseRoot}
          updateStatus={updateStatus}
          onCheckUpdates={() => void gridModeApi.updates.check()}
        />
      );
    }

    if (view.name === "settings") {
      return (
        <SettingsView
          settings={state.settings}
          summary={state.summary}
          onChooseRoot={chooseRoot}
          onAddPhotoDirectory={addPhotoDirectory}
          onRemovePhotoDirectory={removePhotoDirectory}
          onChooseExclusion={chooseExclusion}
          onRemoveExclusion={removeExclusion}
          onRescan={rescan}
          onRebuildThumbnails={rebuildThumbnails}
          onClearCache={clearCache}
          libraryStatusText={state.settingsStatusText}
          isBusy={state.loading}
          updateStatus={updateStatus}
          onCheckUpdates={() => void gridModeApi.updates.check()}
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
          onBack={() => void openView({ name: "library" })}
          onOpenMonth={(month) => void openView({ name: "month", year: view.year, month })}
          onOpenPhoto={openPhoto}
        />
      );
    }

    if (view.name === "month") {
      return (
        <MonthView
          payload={state.month}
          onBack={() => void openView({ name: "year", year: view.year })}
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
    addPhotoDirectory,
    chooseExclusion,
    clearCache,
    loadHome,
    openPhoto,
    openView,
    refreshHome,
    removeExclusion,
    removePhotoDirectory,
    rebuildThumbnails,
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

  const hasPhotoDirectory = getPhotoDirectories(state.settings).length > 0;
  const backgroundThumbnailProgress =
    !state.loading && state.scanProgress?.phase === "generating-thumbnails"
      ? state.scanProgress
      : undefined;

  return (
    <div className={hasPhotoDirectory ? "app-shell" : "app-shell first-run-shell"}>
      {hasPhotoDirectory ? (
        <TopBar
          view={view}
          summary={state.summary}
          onHome={() => void openView({ name: "home" })}
          onLibrary={() => void openView({ name: "library" })}
          onSettings={() => setView({ name: "settings" })}
          onRefresh={view.name === "home" ? () => void refreshHome() : rescan}
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
        ) : backgroundThumbnailProgress ? (
          <LoadingOverlay
            label="Building thumbnail cache in background"
            progress={backgroundThumbnailProgress}
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
  const downloadLabel = status.manualDownload ? "Open download" : "Download";
  const download = () => {
    if (status.manualDownload && status.downloadUrl) {
      void gridModeApi.updates.openDownload(status.downloadUrl);
      return;
    }

    void gridModeApi.updates.download();
  };

  return (
    <aside className={`update-banner ${status.state}`}>
      <div>
        <strong>{status.version ? `Update ${status.version}` : "GridMode Update"}</strong>
        <span>{status.message}</span>
      </div>
      {canDownload ? (
        <button
          className="text-button"
          onClick={download}
        >
          <Download size={16} />
          <span>{downloadLabel}</span>
        </button>
      ) : null}
      {canInstall ? (
        <button
          className="text-button"
          onClick={() => gridModeApi.updates.install()}
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
  onBack,
  onOpenMonth,
  onOpenPhoto
}: {
  payload?: YearPayload;
  onBack: () => void;
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
        <button
          className="text-button"
          onClick={onBack}
        >
          <ChevronLeft size={16} />
          <span>Library</span>
        </button>
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
  onBack,
  onOpenPhoto
}: {
  payload?: MonthPayload;
  onBack: () => void;
  onOpenPhoto: (photo: PhotoAsset) => void;
}): JSX.Element {
  const photos = payload?.photos ?? [];
  const [visiblePhotoCount, setVisiblePhotoCount] = useState(initialMonthPhotoLimit);

  useEffect(() => {
    setVisiblePhotoCount(initialMonthPhotoLimit);
  }, [payload?.year, payload?.month, photos.length]);

  const visiblePhotos = photos.slice(0, visiblePhotoCount);
  const remainingPhotos = Math.max(0, photos.length - visiblePhotos.length);

  return (
    <section className="view-stack">
      <div className="view-heading">
        <div>
          <p>{payload?.year}</p>
          <h1>{payload?.monthName}</h1>
        </div>
        <button
          className="text-button"
          onClick={onBack}
        >
          <ChevronLeft size={16} />
          <span>{payload?.year ? String(payload.year) : "Year"}</span>
        </button>
      </div>
      <PhotoGrid
        photos={visiblePhotos}
        onOpenPhoto={onOpenPhoto}
      />
      {remainingPhotos > 0 ? (
        <button
          className="text-button load-more-button"
          onClick={() => setVisiblePhotoCount((current) => current + monthPhotoLimitIncrement)}
        >
          <span>Show {Math.min(monthPhotoLimitIncrement, remainingPhotos).toLocaleString()} more</span>
        </button>
      ) : null}
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
  onAddPhotoDirectory,
  onRemovePhotoDirectory,
  onChooseExclusion,
  onRemoveExclusion,
  onRescan,
  onRebuildThumbnails,
  onClearCache,
  libraryStatusText,
  isBusy,
  onCheckUpdates
}: {
  settings: AppSettings;
  summary: LibrarySummary;
  updateStatus: UpdateStatus;
  onChooseRoot: () => void;
  onAddPhotoDirectory: () => void;
  onRemovePhotoDirectory: (rootPath: string) => void;
  onChooseExclusion: () => void;
  onRemoveExclusion: (excludedPath: string) => void;
  onRescan: () => void;
  onRebuildThumbnails: () => void;
  onClearCache: () => void;
  libraryStatusText?: string;
  isBusy: boolean;
  onCheckUpdates: () => void;
}): JSX.Element {
  const excludedDirectories = settings.excludedDirectories ?? [];
  const photoDirectories = getPhotoDirectories(settings);

  return (
    <section className="settings-view">
      <div className="settings-panel">
        <div className="view-heading">
          <div>
            <p>Settings</p>
            <h1>Photo locations</h1>
          </div>
        </div>
        <div className="settings-section">
          <div className="section-title-row">
            <div>
              <p>Library</p>
              <h2>Photo locations</h2>
            </div>
            <button
              className="text-button"
              onClick={onAddPhotoDirectory}
              disabled={isBusy}
            >
              <FolderOpen size={16} />
              <span>Add</span>
            </button>
          </div>
          {photoDirectories.length > 0 ? (
            <ul className="path-list">
              {photoDirectories.map((photoDirectory, index) => (
                <li key={photoDirectory}>
                  <span>{index === 0 ? `Primary - ${photoDirectory}` : photoDirectory}</span>
                  {index === 0 ? (
                    <button
                      className="text-button"
                      onClick={onChooseRoot}
                      disabled={isBusy}
                    >
                      <FolderOpen size={16} />
                      <span>Change</span>
                    </button>
                  ) : (
                    <button
                      className="icon-button"
                      onClick={() => onRemovePhotoDirectory(photoDirectory)}
                      title="Remove photo location"
                      disabled={isBusy}
                    >
                      <X size={16} />
                    </button>
                  )}
                </li>
              ))}
            </ul>
          ) : (
            <p className="settings-note">No photo locations selected</p>
          )}
        </div>
        <div className="settings-metrics">
          <Metric
            label="Photos"
            value={summary.photoCount.toLocaleString()}
          />
          <Metric
            label="Locations"
            value={photoDirectories.length.toLocaleString()}
          />
          <Metric
            label="Years"
            value={summary.years.length.toLocaleString()}
          />
          <Metric
            label="Scanned"
            value={summary.lastScanAt ? formatDateTime(summary.lastScanAt) : "Never"}
          />
        </div>
        <div className="settings-section">
          <div className="section-title-row">
            <div>
              <p>Exclusions</p>
              <h2>Excluded folders</h2>
            </div>
            <button
              className="text-button"
              onClick={onChooseExclusion}
              disabled={isBusy}
            >
              <FolderX size={16} />
              <span>Add</span>
            </button>
          </div>
          {excludedDirectories.length > 0 ? (
            <ul className="path-list">
              {excludedDirectories.map((excludedPath) => (
                <li key={excludedPath}>
                  <span>{formatExcludedDirectory(photoDirectories, excludedPath)}</span>
                  <button
                    className="icon-button"
                    onClick={() => onRemoveExclusion(excludedPath)}
                    title="Remove exclusion"
                    disabled={isBusy}
                  >
                    <X size={16} />
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <p className="settings-note">No folders excluded</p>
          )}
        </div>
        <p className="settings-note">
          Scans prebuild thumbnails in the app data cache so photo grids can load immediately.
        </p>
        <div className="settings-actions">
          <button
            className="text-button"
            onClick={onRescan}
            disabled={isBusy}
          >
            <RefreshCcw size={16} />
            <span>Rescan</span>
          </button>
          <button
            className="text-button"
            onClick={onRebuildThumbnails}
            disabled={isBusy}
          >
            <Image size={16} />
            <span>Rebuild thumbnails</span>
          </button>
          <button
            className="text-button"
            onClick={onClearCache}
            disabled={isBusy}
          >
            <Trash2 size={16} />
            <span>Clear cache</span>
          </button>
          <button
            className="text-button"
            onClick={onCheckUpdates}
            disabled={isBusy}
          >
            <Download size={16} />
            <span>Check updates</span>
          </button>
        </div>
        {libraryStatusText ? <p className="settings-note">{libraryStatusText}</p> : null}
        {summary.warnings.length > 0 ? (
          <details className="settings-warnings">
            <summary>{summary.warnings.length.toLocaleString()} library warnings</summary>
            <ul>
              {summary.warnings.map((warning) => <li key={warning}>{warning}</li>)}
            </ul>
          </details>
        ) : null}
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
  const previewPhotos = photos.slice(0, photoSectionPreviewLimit);

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
        photos={previewPhotos}
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
  const [visibleCount, setVisibleCount] = useState(() => Math.min(photos.length, initialGridRenderCount));
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  const gridResetKey = photos.slice(0, 8).map((photo) => photo.id).join(":");

  useEffect(() => {
    setVisibleCount(Math.min(photos.length, initialGridRenderCount));
  }, [gridResetKey]);

  useEffect(() => {
    setVisibleCount((current) => {
      const minimum = Math.min(photos.length, initialGridRenderCount);
      return Math.min(Math.max(current, minimum), photos.length);
    });
  }, [photos.length]);

  useEffect(() => {
    if (visibleCount >= photos.length) {
      return undefined;
    }

    const sentinel = sentinelRef.current;
    if (!sentinel || !("IntersectionObserver" in window)) {
      setVisibleCount((current) => Math.min(photos.length, current + gridRenderBatchSize));
      return undefined;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setVisibleCount((current) => Math.min(photos.length, current + gridRenderBatchSize));
        }
      },
      { rootMargin: gridRenderRootMargin }
    );

    observer.observe(sentinel);

    return () => {
      observer.disconnect();
    };
  }, [photos.length, visibleCount]);

  const visiblePhotos = photos.slice(0, visibleCount);

  return (
    <>
      <div className="photo-grid">
        {visiblePhotos.map((photo, index) => (
          <PhotoTile
            key={photo.id}
            photo={photo}
            eager={index < eagerGridThumbnailCount}
            onOpen={() => onOpenPhoto(photo)}
          />
        ))}
      </div>
      {visibleCount < photos.length ? (
        <div
          ref={sentinelRef}
          className="photo-grid-sentinel"
          aria-hidden="true"
        />
      ) : null}
    </>
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
          eager
          onOpen={() => onOpenPhoto(photo)}
        />
      ))}
    </div>
  );
}

function PhotoTile({
  photo,
  eager = false,
  onOpen
}: {
  photo: PhotoAsset;
  eager?: boolean;
  onOpen: () => void;
}): JSX.Element {
  const tileRef = useRef<HTMLButtonElement | null>(null);
  const [shouldLoad, setShouldLoad] = useState(eager);
  const [isLoaded, setIsLoaded] = useState(false);

  useEffect(() => {
    setShouldLoad(eager);
    setIsLoaded(false);
  }, [eager, photo.thumbnailUrl]);

  useEffect(() => {
    if (shouldLoad) {
      return undefined;
    }

    const tile = tileRef.current;
    if (!tile || !("IntersectionObserver" in window)) {
      setShouldLoad(true);
      return undefined;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setShouldLoad(true);
          observer.disconnect();
        }
      },
      { rootMargin: thumbnailLoadRootMargin }
    );

    observer.observe(tile);

    return () => {
      observer.disconnect();
    };
  }, [shouldLoad]);

  return (
    <button
      ref={tileRef}
      className="photo-tile"
      onClick={onOpen}
      title={photo.name}
    >
      {shouldLoad ? (
        <img
          className={isLoaded ? "loaded" : undefined}
          src={photo.thumbnailUrl}
          alt={photo.name}
          loading={eager ? "eager" : "lazy"}
          decoding="async"
          onLoad={() => setIsLoaded(true)}
        />
      ) : (
        <div
          className="photo-tile-placeholder"
          aria-hidden="true"
        />
      )}
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
  const percent = getScanProgressPercent(progress);

  return (
    <div className="loading-overlay">
      <RefreshCcw size={22} />
      <div className="loading-copy">
        <span>{label}</span>
        {progress ? <small>{formatScanProgress(progress, percent)}</small> : null}
        <div
          className={`scan-progress-track${percent === undefined ? " indeterminate" : ""}`}
          role="progressbar"
          aria-label={label}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
        >
          <i style={percent === undefined ? undefined : { width: `${percent}%` }} />
        </div>
      </div>
    </div>
  );
}

function getScanProgressPercent(progress?: ScanProgress): number | undefined {
  if (
    !progress ||
    (progress.phase !== "reading-metadata" && progress.phase !== "generating-thumbnails") ||
    !progress.totalPhotos
  ) {
    return undefined;
  }

  return Math.min(100, Math.round(((progress.photosProcessed ?? 0) / progress.totalPhotos) * 100));
}

function formatScanProgress(progress: ScanProgress, percent: number | undefined): string {
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

function formatDateTime(value: string): string {
  return new Date(value).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    second: "2-digit"
  });
}

function formatLibraryActionComplete(label: string, summary: LibrarySummary): string {
  const scannedAt = summary.lastScanAt ? formatDateTime(summary.lastScanAt) : formatDateTime(new Date().toISOString());
  return `${label} - ${summary.photoCount.toLocaleString()} photos indexed at ${scannedAt}`;
}

function getPhotoDirectories(settings: AppSettings): string[] {
  const directories = [
    settings.photoDirectory,
    ...(settings.photoDirectories ?? [])
  ].filter((item): item is string => typeof item === "string" && item.trim().length > 0);

  const unique: string[] = [];
  for (const directory of directories) {
    const normalizedDirectory = directory.replace(/[\\/]+$/, "");
    if (!unique.some((item) => item.toLowerCase() === normalizedDirectory.toLowerCase())) {
      unique.push(normalizedDirectory);
    }
  }
  return unique;
}

function formatExcludedDirectory(rootDirs: string[], excludedPath: string): string {
  if (rootDirs.length === 0) {
    return excludedPath;
  }

  for (const rootDir of rootDirs) {
    const normalizedRoot = rootDir.replace(/[\\/]+$/, "");
    const lowerRoot = normalizedRoot.toLowerCase();
    const lowerPath = excludedPath.toLowerCase();
    if (lowerPath === lowerRoot) {
      return excludedPath;
    }

    if (lowerPath.startsWith(`${lowerRoot}\\`) || lowerPath.startsWith(`${lowerRoot}/`)) {
      return excludedPath.slice(normalizedRoot.length + 1);
    }
  }

  return excludedPath;
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
