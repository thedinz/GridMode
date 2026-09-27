import { CalendarDays } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { PhotoAsset } from "../../shared/types";
import { usePhotoActions } from "./PhotoActions";

const initialGridRenderCount = 96;
const gridRenderBatchSize = 64;
const eagerGridThumbnailCount = 48;
const thumbnailLoadRootMargin = "700px 0px";
const gridRenderRootMargin = "1000px 0px";
const thumbnailRetryLimit = 3;
const thumbnailStallTimeoutMs = 8000;

/** A square-tile grid that renders incrementally as the user scrolls. */
export function PhotoGrid({ photos }: { photos: PhotoAsset[] }): JSX.Element {
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
    return () => observer.disconnect();
  }, [photos.length, visibleCount]);

  return (
    <>
      <div className="photo-grid">
        {photos.slice(0, visibleCount).map((photo, index) => (
          <PhotoTile
            key={photo.id}
            photo={photo}
            siblings={photos}
            eager={index < eagerGridThumbnailCount}
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

/** A single row of tiles, used for section previews. */
export function PhotoStrip({ photos }: { photos: PhotoAsset[] }): JSX.Element {
  return (
    <div className="photo-strip">
      {photos.map((photo) => (
        <PhotoTile
          key={photo.id}
          photo={photo}
          siblings={photos}
          eager
        />
      ))}
    </div>
  );
}

function PhotoTile({
  photo,
  siblings,
  eager = false
}: {
  photo: PhotoAsset;
  siblings: PhotoAsset[];
  eager?: boolean;
}): JSX.Element {
  const { openPhoto, showMenu } = usePhotoActions();
  const tileRef = useRef<HTMLButtonElement | null>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);
  const [shouldLoad, setShouldLoad] = useState(eager);
  const [isLoaded, setIsLoaded] = useState(false);
  const [requestAttempt, setRequestAttempt] = useState(0);

  useEffect(() => {
    setShouldLoad(eager);
    setIsLoaded(false);
    setRequestAttempt(0);
  }, [eager, photo.thumbnailUrl]);

  const thumbnailSrc = useMemo(() => {
    if (requestAttempt === 0) {
      return photo.thumbnailUrl;
    }
    const separator = photo.thumbnailUrl.includes("?") ? "&" : "?";
    return `${photo.thumbnailUrl}${separator}retry=${requestAttempt}`;
  }, [photo.thumbnailUrl, requestAttempt]);

  const retryThumbnail = useCallback(() => {
    setIsLoaded(false);
    setRequestAttempt((current) => Math.min(current + 1, thumbnailRetryLimit));
  }, []);

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
    return () => observer.disconnect();
  }, [shouldLoad]);

  useEffect(() => {
    if (!shouldLoad || isLoaded) {
      return undefined;
    }

    const image = imageRef.current;
    if (image?.complete && image.naturalWidth > 0) {
      setIsLoaded(true);
      return undefined;
    }

    if (requestAttempt >= thumbnailRetryLimit) {
      return undefined;
    }

    // Thumbnails generate on demand; retry if one stalls behind a busy render queue.
    const timeout = window.setTimeout(retryThumbnail, thumbnailStallTimeoutMs);
    return () => window.clearTimeout(timeout);
  }, [isLoaded, requestAttempt, retryThumbnail, shouldLoad, thumbnailSrc]);

  return (
    <button
      ref={tileRef}
      className="photo-tile"
      onClick={() => openPhoto(photo, siblings)}
      onContextMenu={(event) => showMenu(event, photo, siblings)}
      title={photo.name}
    >
      {!isLoaded ? (
        <div
          className="photo-tile-placeholder"
          aria-hidden="true"
        />
      ) : null}
      {shouldLoad ? (
        <img
          ref={imageRef}
          className={isLoaded ? "loaded" : undefined}
          src={thumbnailSrc}
          alt={photo.name}
          loading={eager ? "eager" : "lazy"}
          decoding="async"
          onLoad={() => setIsLoaded(true)}
          onError={retryThumbnail}
        />
      ) : null}
      <span>
        <CalendarDays size={13} />
        {photo.year}
      </span>
    </button>
  );
}
