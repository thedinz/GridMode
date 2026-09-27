import { Search, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { SearchPayload, SearchQuery } from "../../shared/types";
import { PhotoGrid } from "../components/PhotoGrid";
import { gridModeApi } from "../gridModeApi";
import { getErrorMessage } from "../lib/format";

const searchDebounceMs = 250;

export function SearchView({
  query,
  libraryVersion,
  onQueryChange
}: {
  query: SearchQuery;
  /** Changes when a background rescan updates the library, re-running the search. */
  libraryVersion: number;
  /** Keeps the query in the view so returning from a photo restores it. */
  onQueryChange: (query: SearchQuery) => void;
}): JSX.Element {
  const [result, setResult] = useState<SearchPayload>();
  const [error, setError] = useState<string>();

  useEffect(() => {
    let cancelled = false;
    const timer = window.setTimeout(() => {
      gridModeApi.library
        .search(query)
        .then((payload) => {
          if (!cancelled) {
            setResult(payload);
            setError(undefined);
          }
        })
        .catch((reason) => {
          if (!cancelled) {
            setError(getErrorMessage(reason));
          }
        });
    }, searchDebounceMs);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [libraryVersion, query]);

  const update = (patch: Partial<SearchQuery>) => onQueryChange({ ...query, ...patch });
  const hasFilters = Boolean(query.text || query.camera || query.from || query.to);
  const cameras = result?.cameras ?? [];

  return (
    <section className="view-stack">
      <div className="view-heading">
        <div>
          <p>Search</p>
          <h1>
            {result ? `${result.total.toLocaleString()} ${result.total === 1 ? "photo" : "photos"}` : "Searching…"}
          </h1>
        </div>
      </div>
      <div className="search-filters">
        <label className="search-field">
          <Search size={16} />
          <input
            type="search"
            autoFocus
            value={query.text ?? ""}
            onChange={(event) => update({ text: event.target.value })}
            placeholder="File name, folder, or camera"
            aria-label="Search text"
          />
        </label>
        <label className="filter-field">
          <span>Camera</span>
          <select
            value={query.camera ?? ""}
            onChange={(event) => update({ camera: event.target.value || undefined })}
          >
            <option value="">Any camera</option>
            {cameras.map((camera) => (
              <option
                key={camera}
                value={camera}
              >
                {camera}
              </option>
            ))}
          </select>
        </label>
        <label className="filter-field">
          <span>From</span>
          <input
            type="date"
            value={query.from ?? ""}
            max={query.to}
            onChange={(event) => update({ from: event.target.value || undefined })}
          />
        </label>
        <label className="filter-field">
          <span>To</span>
          <input
            type="date"
            value={query.to ?? ""}
            min={query.from}
            onChange={(event) => update({ to: event.target.value || undefined })}
          />
        </label>
        {hasFilters ? (
          <button
            className="text-button"
            onClick={() => onQueryChange({})}
          >
            <X size={16} />
            <span>Clear</span>
          </button>
        ) : null}
      </div>
      {error ? <p className="settings-note">{error}</p> : null}
      {result?.truncated ? (
        <p className="settings-note">
          Showing the {result.photos.length.toLocaleString()} most recent matches. Narrow the search to see the rest.
        </p>
      ) : null}
      {result && result.total === 0 ? <p className="settings-note">No photos match these filters.</p> : null}
      <PhotoGrid photos={result?.photos ?? []} />
    </section>
  );
}
