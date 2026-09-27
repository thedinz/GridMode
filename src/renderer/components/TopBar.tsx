import { Grid2X2, Library, RefreshCcw, Search, Settings, Sparkles } from "lucide-react";
import { useState } from "react";
import type { LibrarySummary } from "../../shared/types";

export type NavSection = "home" | "library" | "other";

export function TopBar({
  section,
  summary,
  onHome,
  onLibrary,
  onSettings,
  onRefresh,
  onSearch
}: {
  section: NavSection;
  summary: LibrarySummary;
  onHome: () => void;
  onLibrary: () => void;
  onSettings: () => void;
  onRefresh: () => void;
  onSearch: (text: string) => void;
}): JSX.Element {
  const [searchText, setSearchText] = useState("");

  return (
    <header className="top-bar">
      <div className="brand-mark">
        <Grid2X2 size={22} />
        <span>GridMode</span>
      </div>
      <nav className="nav-group">
        <button
          className={section === "home" ? "nav-button active" : "nav-button"}
          onClick={onHome}
          title="Grid"
        >
          <Sparkles size={18} />
          <span>Grid</span>
        </button>
        <button
          className={section === "library" ? "nav-button active" : "nav-button"}
          onClick={onLibrary}
          title="Library"
        >
          <Library size={18} />
          <span>Library</span>
        </button>
      </nav>
      <div className="toolbar">
        <form
          className="search-box"
          role="search"
          onSubmit={(event) => {
            event.preventDefault();
            onSearch(searchText.trim());
          }}
        >
          <Search size={16} />
          <input
            type="search"
            value={searchText}
            onChange={(event) => setSearchText(event.target.value)}
            placeholder={`Search ${summary.photoCount.toLocaleString()} photos`}
            aria-label="Search photos"
          />
        </form>
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
