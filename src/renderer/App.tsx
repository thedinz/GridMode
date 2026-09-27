import { useCallback, useEffect, useRef, useState } from "react";
import type {
  DirectoryPayload,
  FoldersPayload,
  LibrarySummary,
  MonthPayload,
  PhotoAsset,
  PhotoDetails,
  ScanProgress,
  SearchQuery,
  Settings,
  SettingsPayload,
  UpdateStatus,
  YearPayload
} from "../shared/types";
import { LoadingOverlay } from "./components/LoadingOverlay";
import { PhotoActionsProvider } from "./components/PhotoActions";
import { TopBar, type NavSection } from "./components/TopBar";
import { UpdateBanner } from "./components/UpdateBanner";
import { gridModeApi } from "./gridModeApi";
import { formatLibraryActionComplete, getErrorMessage, monthName } from "./lib/format";
import { getPhotoDirectories } from "./lib/paths";
import { loadGridSize, saveGridSize, type GridSize } from "./lib/prefs";
import {
  DirectoryView,
  HomeView,
  LibraryView,
  MonthView,
  YearView,
  type LibraryTab
} from "./views/BrowseViews";
import { PhotoView } from "./views/PhotoView";
import { SearchView } from "./views/SearchView";
import { FirstRunView, SettingsView } from "./views/SettingsView";

type View =
  | { name: "home" }
  | { name: "library"; tab: LibraryTab }
  | { name: "year"; year: number }
  | { name: "month"; year: number; month: number }
  | { name: "directory"; path: string; previous: View }
  | { name: "photo"; photo: PhotoAsset; siblings: PhotoAsset[]; previous: View }
  | { name: "search"; query: SearchQuery }
  | { name: "settings" };

interface AppState {
  settings: Settings;
  summary: LibrarySummary;
  homePhotos: PhotoAsset[];
  onThisDay: PhotoAsset[];
  year?: YearPayload;
  month?: MonthPayload;
  folders?: FoldersPayload;
  directory?: DirectoryPayload;
  details?: PhotoDetails;
  scanProgress?: ScanProgress;
  loading: boolean;
  statusText?: string;
  settingsStatusText?: string;
}

const initialState: AppState = {
  settings: { photoDirectories: [], excludedDirectories: [] },
  summary: { rootDirs: [], photoCount: 0, years: [], warnings: [] },
  homePhotos: [],
  onThisDay: [],
  loading: true
};

/** Cached view data that goes stale when the set of photos changes. */
const clearedLibraryData: Partial<AppState> = {
  homePhotos: [],
  onThisDay: [],
  year: undefined,
  month: undefined,
  folders: undefined,
  directory: undefined
};

const automaticUpdateCheckDelayMs = 4500;
const noticeDurationMs = 3200;

function navSection(view: View): NavSection {
  switch (view.name) {
    case "home":
      return "home";
    case "library":
    case "year":
    case "month":
    case "directory":
      return "library";
    default:
      return "other";
  }
}

export function App(): JSX.Element {
  const [view, setViewState] = useState<View>({ name: "home" });
  const [state, setState] = useState<AppState>(initialState);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>({ state: "idle" });
  const [gridSize, setGridSize] = useState<GridSize>(loadGridSize);
  const [notice, setNotice] = useState<string>();
  // Bumped when a background rescan changes the library, so self-loading views refetch.
  const [libraryVersion, setLibraryVersion] = useState(0);

  // Async loaders read the latest view and state through refs, so a response
  // for a view the user already left is discarded instead of rendered.
  const viewRef = useRef(view);
  const stateRef = useRef(state);
  stateRef.current = state;
  const noticeTimer = useRef<number>();

  const setView = useCallback((next: View) => {
    viewRef.current = next;
    setViewState(next);
  }, []);

  const mergeState = useCallback((patch: Partial<AppState>) => {
    setState((current) => ({ ...current, ...patch }));
  }, []);

  const notify = useCallback((message: string) => {
    window.clearTimeout(noticeTimer.current);
    setNotice(message);
    noticeTimer.current = window.setTimeout(() => setNotice(undefined), noticeDurationMs);
  }, []);

  useEffect(() => () => window.clearTimeout(noticeTimer.current), []);

  /**
   * Runs a user-visible operation with the loading overlay. The patch is
   * applied when it finishes; failures go to `onError`, or a toast by default.
   */
  const runTask = useCallback(
    async (
      label: string,
      task: () => Promise<Partial<AppState>>,
      options: { progress?: ScanProgress; onError?: (message: string) => Partial<AppState> } = {}
    ): Promise<boolean> => {
      mergeState({ loading: true, statusText: label, scanProgress: options.progress });
      try {
        const patch = await task();
        mergeState({ loading: false, statusText: undefined, scanProgress: undefined, ...patch });
        return true;
      } catch (error) {
        const message = getErrorMessage(error);
        mergeState({
          loading: false,
          statusText: undefined,
          scanProgress: undefined,
          ...(options.onError?.(message) ?? {})
        });
        if (!options.onError) {
          notify(message);
        }
        return false;
      }
    },
    [mergeState, notify]
  );

  const fetchHome = useCallback(async (): Promise<Partial<AppState>> => {
    const payload = await gridModeApi.library.getHome();
    return { summary: payload.summary, homePhotos: payload.photos, onThisDay: payload.onThisDay };
  }, []);

  /** Fetches the data `target` shows. Returns undefined when nothing needs loading. */
  const fetchViewData = useCallback(
    (target: View, force: boolean): { label: string; load: () => Promise<Partial<AppState>> } | undefined => {
      const current = stateRef.current;
      switch (target.name) {
        case "home":
          return force || current.homePhotos.length === 0
            ? { label: "Refreshing grid", load: fetchHome }
            : undefined;
        case "library":
          if (target.tab === "folders") {
            return force || !current.folders
              ? { label: "Loading folders", load: async () => ({ folders: await gridModeApi.library.getFolders() }) }
              : undefined;
          }
          return force
            ? { label: "Loading library", load: async () => ({ summary: await gridModeApi.library.getYears() }) }
            : undefined;
        case "year":
          return force || current.year?.year !== target.year
            ? {
                label: `Loading ${target.year}`,
                load: async () => ({ year: await gridModeApi.library.getYear(target.year) })
              }
            : undefined;
        case "month":
          return force || current.month?.year !== target.year || current.month?.month !== target.month
            ? {
                label: `Loading ${monthName(target.month)} ${target.year}`,
                load: async () => ({ month: await gridModeApi.library.getMonth(target.year, target.month) })
              }
            : undefined;
        case "directory":
          return force || current.directory?.path !== target.path
            ? {
                label: "Loading folder",
                load: async () => ({ directory: await gridModeApi.library.getDirectory(target.path) })
              }
            : undefined;
        default:
          return undefined;
      }
    },
    [fetchHome]
  );

  const loadView = useCallback(
    async (target: View, { force = true, quiet = false }: { force?: boolean; quiet?: boolean } = {}) => {
      if (target.name === "photo") {
        if (stateRef.current.details?.photo.path === target.photo.path) {
          return;
        }
        // Details load quietly so arrowing through photos never flashes the overlay.
        try {
          const details = await gridModeApi.photo.getDetails(target.photo.path);
          const active = viewRef.current;
          if (active.name === "photo" && active.photo.path === target.photo.path) {
            mergeState({ details });
          }
        } catch (error) {
          notify(getErrorMessage(error));
        }
        return;
      }

      const request = fetchViewData(target, force);
      if (!request) {
        return;
      }
      const isStillActive = () => viewRef.current === target;
      const load = async () => {
        const patch = await request.load();
        return isStillActive() ? patch : {};
      };
      if (quiet) {
        try {
          mergeState(await load());
        } catch (error) {
          notify(getErrorMessage(error));
        }
      } else {
        await runTask(request.label, load);
      }
    },
    [fetchViewData, mergeState, notify, runTask]
  );

  const navigate = useCallback(
    (target: View, force = true) => {
      setView(target);
      void loadView(target, { force });
    },
    [loadView, setView]
  );

  /** Returns to an earlier view, reusing its data (so the grid is not reshuffled). */
  const goBack = useCallback((target: View) => navigate(target, false), [navigate]);

  const loadHome = useCallback(
    (label: string, progress?: ScanProgress) => runTask(label, fetchHome, { progress }),
    [fetchHome, runTask]
  );

  /** Incremental scan; reloads the home grid only when the photo count changed. */
  const checkLibraryForChanges = useCallback(
    async (reloadHome: boolean) => {
      const [rootDir] = getPhotoDirectories(stateRef.current.settings);
      const before = stateRef.current.summary.photoCount;
      await runTask(
        "Checking library",
        async () => {
          const summary = await gridModeApi.library.scan(false);
          if (viewRef.current.name === "home" && (reloadHome || summary.photoCount !== before)) {
            return fetchHome();
          }
          return { summary };
        },
        { progress: { phase: "discovering", rootDir, message: "Checking folders for changes" } }
      );
    },
    [fetchHome, runTask]
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
        const [rootDir] = getPhotoDirectories(settings);
        if (!rootDir) {
          mergeState({ loading: false, statusText: undefined });
          return;
        }
        if (summary.photoCount > 0) {
          await loadHome("Loading cached library");
          if (mounted) {
            void checkLibraryForChanges(false);
          }
        } else {
          await loadHome("Building library cache", {
            phase: "discovering",
            rootDir,
            message: "Building library cache"
          });
        }
      })
      .catch((error) => {
        if (mounted) {
          mergeState({ loading: false, statusText: undefined });
          notify(getErrorMessage(error));
        }
      });

    const unsubscribeUpdates = gridModeApi.updates.onStatus(setUpdateStatus);
    const unsubscribeScan = gridModeApi.library.onProgress((progress) => {
      mergeState({ scanProgress: progress, statusText: progress.message });
    });
    const unsubscribeChanges = gridModeApi.library.onChanged((summary) => {
      // A background rescan found new, changed, or removed photos. Keep the
      // current home grid as it is, but refresh everything else.
      mergeState({ summary, year: undefined, month: undefined, folders: undefined, directory: undefined });
      setLibraryVersion((version) => version + 1);
      const active = viewRef.current;
      if (active.name !== "home" && active.name !== "photo") {
        void loadView(active, { force: true, quiet: true });
      }
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
      unsubscribeChanges();
    };
  }, [checkLibraryForChanges, loadHome, loadView, mergeState, notify]);

  const refresh = useCallback(() => {
    if (viewRef.current.name === "home") {
      void checkLibraryForChanges(true);
      return;
    }
    void runTask("Checking library", async () => {
      await gridModeApi.library.scan(false);
      return {};
    }).then(() => loadView(viewRef.current, { force: true, quiet: true }));
  }, [checkLibraryForChanges, loadView, runTask]);

  /** Settings changes that rescan the library; `goHome` also shows the fresh grid. */
  const changeLibrary = useCallback(
    async (label: string, action: () => Promise<SettingsPayload>, goHome = false) => {
      const succeeded = await runTask(
        label,
        async () => {
          const { settings, summary } = await action();
          return { ...clearedLibraryData, settings, summary };
        },
        {
          progress: {
            phase: "discovering",
            rootDir: getPhotoDirectories(stateRef.current.settings)[0],
            message: "Waiting for folder selection"
          }
        }
      );
      if (succeeded && goHome && getPhotoDirectories(stateRef.current.settings).length > 0) {
        navigate({ name: "home" });
      }
    },
    [navigate, runTask]
  );

  const settingsFeedback = (prefix: string) => (message: string) => ({
    settingsStatusText: `${prefix}: ${message}`
  });

  const rescan = useCallback(async () => {
    mergeState({ settingsStatusText: undefined });
    await runTask(
      "Rescanning library",
      async () => {
        const summary = await gridModeApi.library.scan(true);
        return {
          ...clearedLibraryData,
          summary,
          settingsStatusText: formatLibraryActionComplete("Rescan complete", summary)
        };
      },
      {
        progress: {
          phase: "discovering",
          rootDir: getPhotoDirectories(stateRef.current.settings)[0],
          message: "Finding photos"
        },
        onError: settingsFeedback("Rescan failed")
      }
    );
  }, [mergeState, runTask]);

  const clearCache = useCallback(async () => {
    mergeState({ settingsStatusText: undefined });
    await runTask(
      "Clearing cache",
      async () => {
        const { settings, summary } = await gridModeApi.settings.clearCache();
        return {
          settings,
          summary,
          settingsStatusText: "Image cache cleared. Thumbnails will be recreated on the next scan or as needed."
        };
      },
      { onError: settingsFeedback("Clear cache failed") }
    );
  }, [mergeState, runTask]);

  const rebuildThumbnails = useCallback(async () => {
    mergeState({ settingsStatusText: undefined });
    await runTask(
      "Rebuilding thumbnails",
      async () => {
        const { settings, summary, thumbnails } = await gridModeApi.settings.rebuildThumbnails();
        const ready = thumbnails.generated + thumbnails.reused;
        const failureText =
          thumbnails.failed > 0
            ? ` ${thumbnails.failed.toLocaleString()} could not be generated; see library warnings for details.`
            : "";
        return { settings, summary, settingsStatusText: `${ready.toLocaleString()} thumbnails ready.${failureText}` };
      },
      {
        progress: {
          phase: "generating-thumbnails",
          rootDir: getPhotoDirectories(stateRef.current.settings)[0],
          photosProcessed: 0,
          totalPhotos: stateRef.current.summary.photoCount,
          message: "Preparing thumbnail cache"
        },
        onError: settingsFeedback("Thumbnail rebuild failed")
      }
    );
  }, [mergeState, runTask]);

  const openPhoto = useCallback(
    (photo: PhotoAsset, siblings: PhotoAsset[]) => {
      const current = viewRef.current;
      const previous = current.name === "photo" ? current.previous : current;
      navigate({ name: "photo", photo, siblings, previous });
    },
    [navigate]
  );

  const openDirectory = useCallback(
    (path: string) => navigate({ name: "directory", path, previous: viewRef.current }),
    [navigate]
  );

  const changeGridSize = useCallback((size: GridSize) => {
    setGridSize(size);
    saveGridSize(size);
  }, []);

  const photoDirectories = getPhotoDirectories(state.settings);
  const hasPhotoDirectory = photoDirectories.length > 0;
  const backgroundThumbnailProgress =
    !state.loading && state.scanProgress?.phase === "generating-thumbnails" ? state.scanProgress : undefined;

  const renderContent = (): JSX.Element => {
    if (!hasPhotoDirectory) {
      return (
        <FirstRunView
          onChooseRoot={() => void changeLibrary("Scanning selected folder", gridModeApi.settings.chooseRoot, true)}
          updateStatus={updateStatus}
          onCheckUpdates={() => void gridModeApi.updates.check()}
        />
      );
    }

    switch (view.name) {
      case "settings":
        return (
          <SettingsView
            settings={state.settings}
            summary={state.summary}
            updateStatus={updateStatus}
            gridSize={gridSize}
            libraryStatusText={state.settingsStatusText}
            isBusy={state.loading || state.scanProgress?.phase === "generating-thumbnails"}
            onChooseRoot={() => void changeLibrary("Scanning selected folder", gridModeApi.settings.chooseRoot, true)}
            onAddPhotoDirectory={() => void changeLibrary("Adding photo location", gridModeApi.settings.addRoot)}
            onRemovePhotoDirectory={(rootPath) =>
              void changeLibrary("Removing photo location", () => gridModeApi.settings.removeRoot(rootPath))
            }
            onChooseExclusion={() => void changeLibrary("Updating exclusions", gridModeApi.settings.chooseExclusion)}
            onRemoveExclusion={(excludedPath) =>
              void changeLibrary("Updating exclusions", () => gridModeApi.settings.removeExclusion(excludedPath))
            }
            onRescan={() => void rescan()}
            onRebuildThumbnails={() => void rebuildThumbnails()}
            onClearCache={() => void clearCache()}
            onCheckUpdates={() => void gridModeApi.updates.check()}
            onGridSizeChange={changeGridSize}
          />
        );
      case "library":
        return (
          <LibraryView
            tab={view.tab}
            summary={state.summary}
            folders={state.folders}
            onChangeTab={(tab) => navigate({ name: "library", tab }, false)}
            onOpenYear={(year) => navigate({ name: "year", year })}
            onOpenDirectory={openDirectory}
          />
        );
      case "year":
        return (
          <YearView
            payload={state.year?.year === view.year ? state.year : undefined}
            onBack={() => goBack({ name: "library", tab: "dates" })}
            onOpenMonth={(month) => navigate({ name: "month", year: view.year, month })}
          />
        );
      case "month":
        return (
          <MonthView
            payload={state.month?.year === view.year && state.month.month === view.month ? state.month : undefined}
            onBack={() => goBack({ name: "year", year: view.year })}
          />
        );
      case "directory":
        return (
          <DirectoryView
            payload={state.directory?.path === view.path ? state.directory : undefined}
            onBack={() => goBack(view.previous)}
            onOpenDirectory={openDirectory}
          />
        );
      case "photo":
        return (
          <PhotoView
            photo={view.photo}
            siblings={view.siblings}
            details={state.details}
            onBack={() => goBack(view.previous)}
            onNavigate={(photo) => navigate({ ...view, photo })}
            onOpenDirectory={openDirectory}
          />
        );
      case "search":
        return (
          <SearchView
            query={view.query}
            libraryVersion={libraryVersion}
            onQueryChange={(query) => setView({ name: "search", query })}
          />
        );
      case "home":
        return (
          <HomeView
            photos={state.homePhotos}
            onThisDay={state.onThisDay}
            summary={state.summary}
            onShuffle={() => navigate({ name: "home" })}
            isScanning={state.loading && Boolean(state.scanProgress)}
          />
        );
    }
  };

  return (
    <PhotoActionsProvider
      onOpenPhoto={openPhoto}
      notify={notify}
    >
      <div className={`app-shell grid-size-${gridSize}${hasPhotoDirectory ? "" : " first-run-shell"}`}>
        {hasPhotoDirectory ? (
          <TopBar
            section={navSection(view)}
            summary={state.summary}
            onHome={() => navigate({ name: "home" })}
            onLibrary={() => navigate({ name: "library", tab: view.name === "library" ? view.tab : "dates" })}
            onSettings={() => setView({ name: "settings" })}
            onRefresh={refresh}
            onSearch={(text) => setView({ name: "search", query: { text } })}
          />
        ) : null}
        <UpdateBanner status={updateStatus} />
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
          {renderContent()}
        </main>
        {notice ? (
          <div
            className="toast"
            role="status"
          >
            {notice}
          </div>
        ) : null}
      </div>
    </PhotoActionsProvider>
  );
}
