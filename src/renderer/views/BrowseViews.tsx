import { CalendarDays, ChevronLeft, FolderTree, Image, RefreshCcw } from "lucide-react";
import { useEffect, useState } from "react";
import type {
  DirectoryPayload,
  FoldersPayload,
  LibrarySummary,
  MonthPayload,
  PhotoAsset,
  YearPayload
} from "../../shared/types";
import { PhotoGrid } from "../components/PhotoGrid";
import { DirectoryBreadcrumbs, FolderCards, PhotoSection } from "../components/Sections";

const initialMonthPhotoLimit = 240;
const monthPhotoLimitIncrement = 240;

function BackButton({ label, onClick }: { label: string; onClick: () => void }): JSX.Element {
  return (
    <button
      className="text-button"
      onClick={onClick}
    >
      <ChevronLeft size={16} />
      <span>{label}</span>
    </button>
  );
}

export function HomeView({
  photos,
  onThisDay,
  summary,
  onShuffle,
  isScanning
}: {
  photos: PhotoAsset[];
  onThisDay: PhotoAsset[];
  summary: LibrarySummary;
  onShuffle: () => void;
  isScanning: boolean;
}): JSX.Element {
  if (summary.photoCount === 0) {
    return (
      <section className="quiet-state">
        {isScanning ? <RefreshCcw size={40} /> : <Image size={40} />}
        <h1>{isScanning ? "Checking library" : "No photos found"}</h1>
      </section>
    );
  }

  return (
    <section className="view-stack">
      {onThisDay.length > 0 ? (
        <PhotoSection
          title="On this day"
          count={onThisDay.length}
          photos={onThisDay}
        />
      ) : null}
      <div className="view-heading">
        <div>
          <p>Random grid</p>
          <h1>{summary.rootDir}</h1>
        </div>
        <button
          className="text-button"
          onClick={onShuffle}
        >
          <RefreshCcw size={16} />
          <span>Shuffle</span>
        </button>
      </div>
      <PhotoGrid photos={photos} />
    </section>
  );
}

export type LibraryTab = "dates" | "folders";

export function LibraryView({
  tab,
  summary,
  folders,
  onChangeTab,
  onOpenYear,
  onOpenDirectory
}: {
  tab: LibraryTab;
  summary: LibrarySummary;
  folders?: FoldersPayload;
  onChangeTab: (tab: LibraryTab) => void;
  onOpenYear: (year: number) => void;
  onOpenDirectory: (path: string) => void;
}): JSX.Element {
  return (
    <section className="view-stack">
      <div className="view-heading">
        <div>
          <p>Library</p>
          <h1>
            {tab === "dates"
              ? `${summary.years.length} years`
              : `${summary.rootDirs.length} ${summary.rootDirs.length === 1 ? "location" : "locations"}`}
          </h1>
        </div>
        <div
          className="segmented"
          role="tablist"
          aria-label="Browse library by"
        >
          <button
            role="tab"
            aria-selected={tab === "dates"}
            className={tab === "dates" ? "active" : undefined}
            onClick={() => onChangeTab("dates")}
          >
            <CalendarDays size={16} />
            <span>Dates</span>
          </button>
          <button
            role="tab"
            aria-selected={tab === "folders"}
            className={tab === "folders" ? "active" : undefined}
            onClick={() => onChangeTab("folders")}
          >
            <FolderTree size={16} />
            <span>Folders</span>
          </button>
        </div>
      </div>
      {tab === "dates" ? (
        <div className="section-list">
          {summary.years.map((year) => (
            <PhotoSection
              key={year.year}
              title={String(year.year)}
              count={year.count}
              photos={year.sample}
              onOpenSection={() => onOpenYear(year.year)}
            />
          ))}
        </div>
      ) : (
        <FolderCards
          folders={folders?.roots ?? []}
          onOpenDirectory={onOpenDirectory}
        />
      )}
    </section>
  );
}

export function YearView({
  payload,
  onBack,
  onOpenMonth
}: {
  payload?: YearPayload;
  onBack: () => void;
  onOpenMonth: (month: number) => void;
}): JSX.Element {
  return (
    <section className="view-stack">
      <div className="view-heading">
        <div>
          <p>Year</p>
          <h1>{payload?.year ?? ""}</h1>
        </div>
        <BackButton
          label="Library"
          onClick={onBack}
        />
      </div>
      <div className="section-list">
        {(payload?.months ?? []).map((month) => (
          <PhotoSection
            key={month.month}
            title={month.monthName}
            count={month.count}
            photos={month.sample}
            onOpenSection={() => onOpenMonth(month.month)}
          />
        ))}
      </div>
    </section>
  );
}

export function MonthView({ payload, onBack }: { payload?: MonthPayload; onBack: () => void }): JSX.Element {
  const photos = payload?.photos ?? [];
  const [visiblePhotoCount, setVisiblePhotoCount] = useState(initialMonthPhotoLimit);

  useEffect(() => {
    setVisiblePhotoCount(initialMonthPhotoLimit);
  }, [payload?.year, payload?.month]);

  const visiblePhotos = photos.slice(0, visiblePhotoCount);
  const remainingPhotos = Math.max(0, photos.length - visiblePhotos.length);

  return (
    <section className="view-stack">
      <div className="view-heading">
        <div>
          <p>{payload?.year}</p>
          <h1>{payload?.monthName}</h1>
        </div>
        <BackButton
          label={payload?.year ? String(payload.year) : "Year"}
          onClick={onBack}
        />
      </div>
      <PhotoGrid photos={visiblePhotos} />
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

export function DirectoryView({
  payload,
  onBack,
  onOpenDirectory
}: {
  payload?: DirectoryPayload;
  onBack: () => void;
  onOpenDirectory: (path: string) => void;
}): JSX.Element {
  const subfolders = payload?.subfolders ?? [];

  return (
    <section className="view-stack">
      <div className="view-heading">
        <div>
          <p>Folder · {(payload?.photoCount ?? 0).toLocaleString()} photos</p>
          <h1>{payload?.name ?? "Folder"}</h1>
          <DirectoryBreadcrumbs
            breadcrumbs={payload?.breadcrumbs ?? []}
            onOpenDirectory={onOpenDirectory}
          />
        </div>
        <BackButton
          label="Back"
          onClick={onBack}
        />
      </div>
      {subfolders.length > 0 ? (
        <FolderCards
          folders={subfolders}
          onOpenDirectory={onOpenDirectory}
        />
      ) : null}
      <PhotoGrid photos={payload?.photos ?? []} />
    </section>
  );
}
