import { afterEach, describe, expect, it, vi } from "vitest";
import { isGridSize, loadGridSize, saveGridSize } from "./prefs";

function stubStorage(storage: Partial<Storage>) {
  vi.stubGlobal("window", { localStorage: storage });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("grid size preference", () => {
  it("round-trips a saved size", () => {
    const values = new Map<string, string>();
    stubStorage({
      getItem: (key) => values.get(key) ?? null,
      setItem: (key, value) => void values.set(key, value)
    });
    saveGridSize("large");
    expect(loadGridSize()).toBe("large");
  });

  it("ignores unknown stored values", () => {
    stubStorage({ getItem: () => "enormous" });
    expect(loadGridSize()).toBe("medium");
  });

  it("falls back to the default when storage is unavailable", () => {
    stubStorage({
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      }
    });
    expect(() => saveGridSize("small")).not.toThrow();
    expect(loadGridSize()).toBe("medium");
  });

  it("validates sizes", () => {
    expect(isGridSize("small")).toBe(true);
    expect(isGridSize("tiny")).toBe(false);
  });
});
