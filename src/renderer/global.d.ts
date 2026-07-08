import type { GridModeApi } from "../preload/preload";

declare global {
  interface Window {
    gridMode: GridModeApi;
  }
}

export {};
