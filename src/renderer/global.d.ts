import type { GridModeApi } from "../preload/preload";

declare global {
  interface Window {
    gridMode?: GridModeApi;
    __TAURI_INTERNALS__?: unknown;
  }
}

export {};
