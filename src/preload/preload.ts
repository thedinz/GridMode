import { contextBridge, ipcRenderer } from "electron";
import type {
  HomePayload,
  LibrarySummary,
  MonthPayload,
  PhotoDetails,
  ScanProgress,
  Settings,
  UpdateStatus,
  YearPayload
} from "../shared/types";

const api = {
  settings: {
    get: (): Promise<{ settings: Settings; summary: LibrarySummary }> => ipcRenderer.invoke("settings:get"),
    chooseRoot: (): Promise<{ settings: Settings; summary: LibrarySummary }> =>
      ipcRenderer.invoke("settings:choose-root")
  },
  library: {
    scan: (force = false): Promise<LibrarySummary> => ipcRenderer.invoke("library:scan", force),
    getHome: (): Promise<HomePayload> => ipcRenderer.invoke("library:get-home"),
    getYears: (): Promise<LibrarySummary> => ipcRenderer.invoke("library:get-years"),
    getYear: (year: number): Promise<YearPayload> => ipcRenderer.invoke("library:get-year", year),
    getMonth: (year: number, month: number): Promise<MonthPayload> =>
      ipcRenderer.invoke("library:get-month", year, month),
    onProgress: (callback: (progress: ScanProgress) => void): (() => void) => {
      const listener = (_event: Electron.IpcRendererEvent, progress: ScanProgress) => callback(progress);
      ipcRenderer.on("scan:progress", listener);
      return () => ipcRenderer.off("scan:progress", listener);
    }
  },
  photo: {
    getDetails: (photoPath: string): Promise<PhotoDetails> => ipcRenderer.invoke("photo:get-details", photoPath)
  },
  updates: {
    check: (): Promise<UpdateStatus> => ipcRenderer.invoke("updates:check"),
    download: (): Promise<UpdateStatus> => ipcRenderer.invoke("updates:download"),
    install: (): void => ipcRenderer.send("updates:install"),
    onStatus: (callback: (status: UpdateStatus) => void): (() => void) => {
      const listener = (_event: Electron.IpcRendererEvent, status: UpdateStatus) => callback(status);
      ipcRenderer.on("updates:status", listener);
      return () => ipcRenderer.off("updates:status", listener);
    }
  }
};

contextBridge.exposeInMainWorld("gridMode", api);

export type GridModeApi = typeof api;
