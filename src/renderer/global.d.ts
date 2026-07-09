import type { GridModeApi } from "../shared/types";

declare global {
  interface Window {
    gridMode?: GridModeApi;
    __TAURI_INTERNALS__?: unknown;
  }
}

export {};
