import {
  ChevronLeft,
  ChevronRight,
  ExternalLink,
  FolderOpen,
  MapPin,
  RotateCcw,
  X,
  ZoomIn,
  ZoomOut
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ExifRow, PhotoAsset, PhotoDetails } from "../../shared/types";
import { usePhotoActions } from "../components/PhotoActions";
import { DirectoryBreadcrumbs } from "../components/Sections";
import { ZoomableImage, type ZoomControls } from "../components/ZoomableImage";
import { gridModeApi } from "../gridModeApi";
import {
  formatBytes,
  formatCaptureTime,
  formatCoordinates,
  formatDimensions,
  getErrorMessage
} from "../lib/format";
import { isTypingTarget, revealLabel } from "../lib/platform";

export function PhotoView({
  photo,
  siblings,
  details,
  onBack,
  onNavigate,
  onOpenDirectory
}: {
  photo: PhotoAsset;
  siblings: PhotoAsset[];
  details?: PhotoDetails;
  onBack: () => void;
  onNavigate: (photo: PhotoAsset) => void;
  onOpenDirectory: (path: string) => void;
}): JSX.Element {
  const { notify } = usePhotoActions();
  const zoomRef = useRef<ZoomControls | null>(null);
  const [isZoomed, setIsZoomed] = useState(false);
  const currentDetails = details?.photo.path === photo.path ? details : undefined;
  const index = siblings.findIndex((sibling) => sibling.path === photo.path);
  const previous = index > 0 ? siblings[index - 1] : undefined;
  const next = index >= 0 && index < siblings.length - 1 ? siblings[index + 1] : undefined;

  useEffect(() => {
    // Warm the cache so arrowing through photos feels instant.
    for (const neighbor of [previous, next]) {
      if (neighbor) {
        new window.Image().src = neighbor.url;
      }
    }
  }, [next, previous]);

  const run = useCallback(
    (task: Promise<void>) => {
      task.catch((error) => notify(getErrorMessage(error)));
    },
    [notify]
  );

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (isTypingTarget(event.target) || event.altKey || event.ctrlKey || event.metaKey) {
        return;
      }
      if (event.key === "ArrowLeft" && previous) {
        onNavigate(previous);
      } else if (event.key === "ArrowRight" && next) {
        onNavigate(next);
      } else if (event.key === "Escape") {
        if (zoomRef.current?.isZoomed()) {
          zoomRef.current.reset();
        } else {
          onBack();
        }
      } else if (event.key === "+" || event.key === "=") {
        zoomRef.current?.zoomBy(1.5);
      } else if (event.key === "-") {
        zoomRef.current?.zoomBy(1 / 1.5);
      } else if (event.key === "0") {
        zoomRef.current?.reset();
      } else {
        return;
      }
      event.preventDefault();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [next, onBack, onNavigate, previous]);

  return (
    <section className="photo-view">
      <div className="photo-stage">
        <ZoomableImage
          ref={zoomRef}
          src={photo.url}
          alt={photo.name}
          onZoomChange={setIsZoomed}
        />
        <div className="stage-toolbar stage-toolbar-start">
          <button
            className="icon-button"
            onClick={onBack}
            title="Back (Esc)"
          >
            <X size={18} />
          </button>
          {siblings.length > 1 && index >= 0 ? (
            <span className="stage-counter">
              {(index + 1).toLocaleString()} / {siblings.length.toLocaleString()}
            </span>
          ) : null}
        </div>
        <div className="stage-toolbar stage-toolbar-end">
          <button
            className="icon-button"
            onClick={() => zoomRef.current?.zoomBy(1 / 1.5)}
            title="Zoom out (-)"
            disabled={!isZoomed}
          >
            <ZoomOut size={18} />
          </button>
          <button
            className="icon-button"
            onClick={() => zoomRef.current?.zoomBy(1.5)}
            title="Zoom in (+)"
          >
            <ZoomIn size={18} />
          </button>
          <button
            className="icon-button"
            onClick={() => zoomRef.current?.reset()}
            title="Fit to window (0)"
            disabled={!isZoomed}
          >
            <RotateCcw size={18} />
          </button>
        </div>
        {previous ? (
          <button
            className="stage-arrow stage-arrow-previous"
            onClick={() => onNavigate(previous)}
            title="Previous photo (←)"
          >
            <ChevronLeft size={28} />
          </button>
        ) : null}
        {next ? (
          <button
            className="stage-arrow stage-arrow-next"
            onClick={() => onNavigate(next)}
            title="Next photo (→)"
          >
            <ChevronRight size={28} />
          </button>
        ) : null}
      </div>
      <div className="metadata-panel">
        <div className="metadata-title">
          <h1>{photo.name}</h1>
          <div className="metadata-actions">
            <button
              className="text-button"
              onClick={() => run(gridModeApi.photo.reveal(photo.path))}
            >
              <FolderOpen size={16} />
              <span>{revealLabel}</span>
            </button>
            <button
              className="text-button"
              onClick={() => run(gridModeApi.photo.open(photo.path))}
            >
              <ExternalLink size={16} />
              <span>Open with default app</span>
            </button>
          </div>
        </div>
        <MetadataRows
          photo={photo}
          rows={currentDetails?.exif ?? []}
          onOpenDirectory={onOpenDirectory}
          breadcrumbs={currentDetails?.directoryBreadcrumbs}
          onOpenMap={() => run(gridModeApi.photo.openMap(photo.path))}
        />
      </div>
    </section>
  );
}

function MetadataRows({
  photo,
  rows,
  breadcrumbs,
  onOpenDirectory,
  onOpenMap
}: {
  photo: PhotoAsset;
  rows: ExifRow[];
  breadcrumbs?: PhotoDetails["directoryBreadcrumbs"];
  onOpenDirectory: (path: string) => void;
  onOpenMap: () => void;
}): JSX.Element {
  const dimensions = formatDimensions(photo);
  const allRows: ExifRow[] = [
    {
      label: "Taken",
      value: `${formatCaptureTime(photo.capturedAt)}${photo.dateSource === "file" ? " (file date)" : ""}`
    },
    ...rows,
    ...(dimensions ? [{ label: "Dimensions", value: dimensions }] : []),
    { label: "File size", value: formatBytes(photo.size) }
  ];

  return (
    <dl className="metadata-grid">
      {allRows.map((row) => (
        <div key={row.label}>
          <dt>{row.label}</dt>
          <dd title={row.value}>{row.value}</dd>
        </div>
      ))}
      {photo.location ? (
        <div>
          <dt>Location</dt>
          <dd>
            <button
              type="button"
              className="link-button"
              onClick={onOpenMap}
              title="Show on OpenStreetMap"
            >
              <MapPin size={14} />
              <span>{formatCoordinates(photo.location)}</span>
            </button>
          </dd>
        </div>
      ) : null}
      <div className="metadata-folder">
        <dt>Folder</dt>
        <dd>
          <DirectoryBreadcrumbs
            breadcrumbs={breadcrumbs ?? []}
            onOpenDirectory={onOpenDirectory}
            fallback={photo.directory}
          />
        </dd>
      </div>
    </dl>
  );
}
