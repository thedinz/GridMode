import { Download, FolderOpen, FolderX, Image, RefreshCcw, Trash2, X } from "lucide-react";
import type { LibrarySummary, Settings, UpdateStatus } from "../../shared/types";
import { GridModeLogo } from "../components/GridModeLogo";
import { Metric } from "../components/Sections";
import { formatDateTime } from "../lib/format";
import { formatExcludedDirectory, getPhotoDirectories } from "../lib/paths";
import { gridSizes, type GridSize } from "../lib/prefs";

export function SettingsView({
  settings,
  summary,
  updateStatus,
  gridSize,
  libraryStatusText,
  isBusy,
  onChooseRoot,
  onAddPhotoDirectory,
  onRemovePhotoDirectory,
  onChooseExclusion,
  onRemoveExclusion,
  onRescan,
  onRebuildThumbnails,
  onClearCache,
  onCheckUpdates,
  onGridSizeChange
}: {
  settings: Settings;
  summary: LibrarySummary;
  updateStatus: UpdateStatus;
  gridSize: GridSize;
  libraryStatusText?: string;
  isBusy: boolean;
  onChooseRoot: () => void;
  onAddPhotoDirectory: () => void;
  onRemovePhotoDirectory: (rootPath: string) => void;
  onChooseExclusion: () => void;
  onRemoveExclusion: (excludedPath: string) => void;
  onRescan: () => void;
  onRebuildThumbnails: () => void;
  onClearCache: () => void;
  onCheckUpdates: () => void;
  onGridSizeChange: (size: GridSize) => void;
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
        <div className="settings-section">
          <div className="section-title-row">
            <div>
              <p>Display</p>
              <h2>Grid size</h2>
            </div>
            <div
              className="segmented"
              role="radiogroup"
              aria-label="Grid size"
            >
              {gridSizes.map((option) => (
                <button
                  key={option.value}
                  role="radio"
                  aria-checked={gridSize === option.value}
                  className={gridSize === option.value ? "active" : undefined}
                  onClick={() => onGridSizeChange(option.value)}
                >
                  <span>{option.label}</span>
                </button>
              ))}
            </div>
          </div>
        </div>
        <p className="settings-note">
          GridMode watches your photo locations and picks up new photos automatically. Scans prebuild thumbnails in the
          app data cache so photo grids can load immediately.
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
              {summary.warnings.map((warning) => (
                <li key={warning}>{warning}</li>
              ))}
            </ul>
          </details>
        ) : null}
        {updateStatus.message ? <p className="settings-note">{updateStatus.message}</p> : null}
      </div>
    </section>
  );
}

export function FirstRunView({
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
