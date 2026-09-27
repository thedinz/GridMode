import { describe, expect, it } from "vitest";
import { formatExcludedDirectory, getPhotoDirectories } from "./paths";

describe("getPhotoDirectories", () => {
  it("puts the primary directory first and drops duplicates and blanks", () => {
    expect(
      getPhotoDirectories({
        photoDirectory: "C:\\Photos\\",
        photoDirectories: ["c:\\photos", "D:\\Archive", " "],
        excludedDirectories: []
      })
    ).toEqual(["C:\\Photos", "D:\\Archive"]);
  });

  it("returns nothing when no location is configured", () => {
    expect(getPhotoDirectories({ photoDirectories: [], excludedDirectories: [] })).toEqual([]);
  });
});

describe("formatExcludedDirectory", () => {
  it("shows exclusions relative to their library root", () => {
    expect(formatExcludedDirectory(["C:\\Photos"], "C:\\Photos\\Screenshots")).toBe("Screenshots");
    expect(formatExcludedDirectory(["/Users/me/Pictures/"], "/Users/me/Pictures/Old/2010")).toBe("Old/2010");
  });

  it("leaves paths outside every root untouched", () => {
    expect(formatExcludedDirectory(["C:\\Photos"], "C:\\PhotosBackup\\x")).toBe("C:\\PhotosBackup\\x");
    expect(formatExcludedDirectory([], "C:\\Photos\\x")).toBe("C:\\Photos\\x");
  });
});
