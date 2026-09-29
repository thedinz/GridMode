import { Download, FolderOpen, FolderX, Image, KeyRound, RefreshCcw, Trash2, X } from "lucide-react";
import { useState } from "react";
import type { LibrarySummary, LicenseStatus, Settings, UpdateStatus } from "../../shared/types";
import { GridModeLogo } from "../components/GridModeLogo";
import { LicenseKeyForm } from "../components/LicenseKeyForm";
import { Metric } from "../components/Sections";
import { formatDateTime, formatTrialRemaining } from "../lib/format";
import { formatExcludedDirectory, getPhotoDirectories } from "../lib/paths";
import { gridSizes, type GridSize } from "../lib/prefs";

export function SettingsView({
  settings,
  summary,
  updateStatus,
  license,
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
  onGridSizeChange,
  onStartTrial,
  onActivateLicense,
  onDeactivateLicense,
  onBuyLicense
}: {
  settings: Settings;
  summary: LibrarySummary;
  updateStatus: UpdateStatus;
  license?: LicenseStatus;
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
  onStartTrial: () => void;
  onActivateLicense: (licenseKey: string) => Promise<unknown>;
  onDeactivateLicense: () => void;
  onBuyLicense: () => void;
}): JSX.Element {
  const excludedDirectories = settings.excludedDirectories ?? [];
  const photoDirectories = getPhotoDirectories(settings);

  return (
    <section className="settings-view">
      <div className="settings-panel">
        <div className="view-heading">
          <div>
            <p>GridMode</p>
            <h1>Settings</h1>
          </div>
        </div>
        {license ? (
          <LicenseSection
            license={license}
            onStartTrial={onStartTrial}
            onActivateLicense={onActivateLicense}
            onDeactivateLicense={onDeactivateLicense}
            onBuyLicense={onBuyLicense}
          />
        ) : null}
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

function LicenseSection({
  license,
  onStartTrial,
  onActivateLicense,
  onDeactivateLicense,
  onBuyLicense
}: {
  license: LicenseStatus;
  onStartTrial: () => void;
  onActivateLicense: (licenseKey: string) => Promise<unknown>;
  onDeactivateLicense: () => void;
  onBuyLicense: () => void;
}): JSX.Element {
  const [showKeyForm, setShowKeyForm] = useState(false);
  const buyDisabled = !license.checkoutAvailable;

  const deactivate = () => {
    if (window.confirm("Deactivate GridMode on this computer? You can activate it again with your license key.")) {
      onDeactivateLicense();
    }
  };

  const wrongMajorNote = license.otherMajorLicense ? (
    <p className="settings-note">
      This license is for GridMode {license.otherMajorLicense}.x and does not include GridMode {license.appMajor}.
    </p>
  ) : null;

  const statusBody = (): JSX.Element => {
    switch (license.state) {
      case "licensed":
        return (
          <>
            <div className="settings-metrics license-metrics">
              <Metric
                label="Edition"
                value={`GridMode ${license.appMajor}.x`}
              />
              <Metric
                label="Status"
                value="Licensed"
              />
              <Metric
                label="License"
                value={license.license?.maskedKey ?? "Unknown"}
              />
            </div>
            {license.offline ? (
              <p className="settings-note">
                Last verified {license.license ? formatDateTime(license.license.lastValidatedAt) : "recently"}.
                GridMode will check again when you're online.
              </p>
            ) : null}
            {wrongMajorNote}
            <div className="settings-actions">
              <button
                className="text-button"
                onClick={deactivate}
              >
                <X size={16} />
                <span>Deactivate This Computer</span>
              </button>
            </div>
          </>
        );
      case "trial":
        return (
          <>
            <div className="settings-metrics license-metrics">
              <Metric
                label="Plan"
                value={`${license.trialLengthDays}-day trial`}
              />
              <Metric
                label="Remaining"
                value={
                  license.trialDaysRemaining !== undefined
                    ? formatTrialRemaining(license.trialDaysRemaining)
                    : "-"
                }
              />
            </div>
            <div className="settings-actions">
              <button
                className="text-button primary"
                onClick={onBuyLicense}
                disabled={buyDisabled}
              >
                <Download size={16} />
                <span>Buy GridMode — {license.priceLabel}</span>
              </button>
              {!showKeyForm ? (
                <button
                  className="text-button"
                  onClick={() => setShowKeyForm(true)}
                  disabled={!license.configured}
                >
                  <KeyRound size={16} />
                  <span>Enter license key</span>
                </button>
              ) : null}
            </div>
            {showKeyForm && license.configured ? (
              <LicenseKeyForm
                onActivate={onActivateLicense}
                onActivated={() => setShowKeyForm(false)}
                autoFocus={false}
              />
            ) : null}
          </>
        );
      case "trialExpired":
        return (
          <>
            <p className="settings-note">Your GridMode trial has ended.</p>
            <div className="settings-actions">
              <button
                className="text-button primary"
                onClick={onBuyLicense}
                disabled={buyDisabled}
              >
                <Download size={16} />
                <span>Buy GridMode — {license.priceLabel}</span>
              </button>
              {!showKeyForm ? (
                <button
                  className="text-button"
                  onClick={() => setShowKeyForm(true)}
                  disabled={!license.configured}
                >
                  <KeyRound size={16} />
                  <span>Enter License Key</span>
                </button>
              ) : null}
            </div>
            {showKeyForm && license.configured ? (
              <LicenseKeyForm
                onActivate={onActivateLicense}
                onActivated={() => setShowKeyForm(false)}
                autoFocus={false}
              />
            ) : null}
          </>
        );
      case "validationRequired":
        return <p className="settings-note">Connect to the internet so GridMode can confirm your license.</p>;
      case "trialAvailable":
      default:
        return (
          <div className="settings-actions">
            <button
              className="text-button primary"
              onClick={onStartTrial}
            >
              <span>Start {license.trialLengthDays}-day free trial</span>
            </button>
          </div>
        );
    }
  };

  return (
    <div className="settings-section">
      <div className="section-title-row">
        <div>
          <p>Account</p>
          <h2>License</h2>
        </div>
      </div>
      {statusBody()}
      {license.modeLabel === "Test Mode" ? (
        <p className="settings-note license-test-mode">Lemon Squeezy Test Mode</p>
      ) : null}
    </div>
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
