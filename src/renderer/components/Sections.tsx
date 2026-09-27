import { ChevronRight, Folder } from "lucide-react";
import type { DirectoryBreadcrumb, FolderSummary, PhotoAsset } from "../../shared/types";
import { PhotoStrip } from "./PhotoGrid";

const photoSectionPreviewLimit = 12;

/** A titled row of preview tiles that opens the full section when the title is clicked. */
export function PhotoSection({
  title,
  count,
  photos,
  onOpenSection
}: {
  title: string;
  count: number;
  photos: PhotoAsset[];
  onOpenSection?: () => void;
}): JSX.Element {
  const heading = (
    <>
      <span>{title}</span>
      <small>{count.toLocaleString()} photos</small>
    </>
  );

  return (
    <section className="photo-section">
      {onOpenSection ? (
        <button
          className="section-heading"
          onClick={onOpenSection}
        >
          {heading}
        </button>
      ) : (
        <div className="section-heading static">{heading}</div>
      )}
      <PhotoStrip photos={photos.slice(0, photoSectionPreviewLimit)} />
    </section>
  );
}

export function FolderCards({
  folders,
  onOpenDirectory
}: {
  folders: FolderSummary[];
  onOpenDirectory: (path: string) => void;
}): JSX.Element {
  return (
    <div className="folder-grid">
      {folders.map((folder) => (
        <button
          key={folder.path}
          className="folder-card"
          onClick={() => onOpenDirectory(folder.path)}
          title={folder.path}
        >
          <div className="folder-cover">
            {folder.cover ? (
              <img
                src={folder.cover.thumbnailUrl}
                alt=""
                loading="lazy"
                decoding="async"
              />
            ) : (
              <Folder size={28} />
            )}
          </div>
          <div className="folder-copy">
            <strong>{folder.name}</strong>
            <small>{folder.photoCount.toLocaleString()} photos</small>
          </div>
        </button>
      ))}
    </div>
  );
}

export function DirectoryBreadcrumbs({
  breadcrumbs,
  onOpenDirectory,
  fallback
}: {
  breadcrumbs: DirectoryBreadcrumb[];
  onOpenDirectory: (path: string) => void;
  fallback?: string;
}): JSX.Element {
  if (breadcrumbs.length === 0) {
    return <span className="directory-breadcrumb-fallback">{fallback}</span>;
  }

  return (
    <nav
      className="directory-breadcrumbs"
      aria-label="Photo folder path"
    >
      {breadcrumbs.map((breadcrumb, index) => (
        <span
          className="directory-breadcrumb"
          key={breadcrumb.path}
        >
          {index > 0 ? <ChevronRight size={13} aria-hidden="true" /> : null}
          <button
            type="button"
            onClick={() => onOpenDirectory(breadcrumb.path)}
            title={`Open ${breadcrumb.path} in GridMode`}
          >
            {breadcrumb.name}
          </button>
        </span>
      ))}
    </nav>
  );
}

export function Metric({ label, value }: { label: string; value: string }): JSX.Element {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}
