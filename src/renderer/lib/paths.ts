import type { Settings } from "../../shared/types";

function trimTrailingSeparators(path: string): string {
  return path.replace(/[\\/]+$/, "");
}

/** The primary photo location followed by any additional ones, without duplicates. */
export function getPhotoDirectories(settings: Settings): string[] {
  const directories = [settings.photoDirectory, ...(settings.photoDirectories ?? [])].filter(
    (item): item is string => typeof item === "string" && item.trim().length > 0
  );

  const unique: string[] = [];
  for (const directory of directories) {
    const normalized = trimTrailingSeparators(directory);
    if (!unique.some((item) => item.toLowerCase() === normalized.toLowerCase())) {
      unique.push(normalized);
    }
  }
  return unique;
}

/** Shows an excluded folder relative to the library root that contains it. */
export function formatExcludedDirectory(rootDirs: string[], excludedPath: string): string {
  const lowerPath = excludedPath.toLowerCase();
  for (const rootDir of rootDirs) {
    const normalizedRoot = trimTrailingSeparators(rootDir);
    const lowerRoot = normalizedRoot.toLowerCase();
    if (lowerPath === lowerRoot) {
      return excludedPath;
    }
    if (lowerPath.startsWith(`${lowerRoot}\\`) || lowerPath.startsWith(`${lowerRoot}/`)) {
      return excludedPath.slice(normalizedRoot.length + 1);
    }
  }
  return excludedPath;
}
