export type GridSize = "small" | "medium" | "large";

export const gridSizes: { value: GridSize; label: string }[] = [
  { value: "small", label: "Small" },
  { value: "medium", label: "Medium" },
  { value: "large", label: "Large" }
];

const gridSizeKey = "gridmode.gridSize";

export function isGridSize(value: unknown): value is GridSize {
  return value === "small" || value === "medium" || value === "large";
}

// Display preferences are per-machine conveniences, so localStorage is enough;
// storage can be unavailable, in which case the default applies.
export function loadGridSize(): GridSize {
  try {
    const stored = window.localStorage.getItem(gridSizeKey);
    return isGridSize(stored) ? stored : "medium";
  } catch {
    return "medium";
  }
}

export function saveGridSize(size: GridSize): void {
  try {
    window.localStorage.setItem(gridSizeKey, size);
  } catch {
    // Ignore: the choice still applies for this session.
  }
}
