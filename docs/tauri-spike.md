# GridMode Tauri Spike

## Why this spike exists

GridMode is already useful, but the current Electron build can feel slow when the
computer is busy. That is a release risk: users may not stay long enough to see
future optimizations if the first experience feels heavy.

Tauri is worth a focused spike now because it can reduce app shell weight and
lets us move file scanning, metadata extraction, and thumbnail generation into a
native backend. The spike should prove performance and behavior, not just prove
that the React app can boot inside a different window.

## Current local blocker

This machine does not currently have Rust installed:

- `rustc` is not available.
- `cargo` is not available.

The Tauri spike can be prepared in the repository, but a buildable proof of life
requires the Rust toolchain and the macOS desktop build prerequisites.

## What can stay

- The React/Vite renderer can mostly stay.
- Shared TypeScript payload shapes can stay as the UI contract.
- The current `window.gridMode` bridge shape is a good migration seam.
- Visual design, library navigation, settings UI, and update UI can stay.

## What must be replaced

| Current Electron surface | Tauri replacement |
| --- | --- |
| `BrowserWindow` | Tauri window configuration |
| `ipcMain`/`ipcRenderer` | Tauri commands and events |
| `dialog.showOpenDialog` | Tauri dialog plugin |
| `protocol.handle("gridmode-photo")` | Tauri asset/custom protocol plus scoped file access |
| `app.getPath("userData")` | Tauri app data path |
| `electron-updater` | Tauri updater plugin or manual GitHub release check |
| Node `fs/path/crypto/os` backend | Rust backend modules |
| `sharp` WebP rendering | Rust image pipeline or a sidecar strategy |
| `exifr` metadata reads | Rust EXIF/image metadata pipeline or a sidecar strategy |

## Tauri feasibility notes

- Tauri supports Vite/React frontends, so we do not need to replace the UI stack.
- Tauri's dialog plugin returns file system paths on Windows, macOS, and Linux,
  which covers the selected photo folder workflows.
- Tauri can convert file paths into webview-safe asset URLs, but the app must
  configure asset protocol access and CSP correctly.
- Tauri has a GitHub Actions path via `tauri-action` for building and uploading
  release artifacts.
- Tauri updater support exists, but updater artifacts must be signed with Tauri
  updater keys. That is separate from Apple Developer signing and cannot be
  treated as optional for automatic update installation.
- macOS signing/notarization rules still apply. Unsigned or ad-hoc signed Mac
  builds can still require users to bypass warnings in Privacy & Security.

## Biggest technical risks

1. Image decoding parity

   GridMode currently relies on `sharp` for thumbnails and display renders.
   Replacing this in Rust is straightforward for JPEG/PNG/WebP, but HEIC/HEIF,
   AVIF, TIFF, orientation handling, color conversion, and WebP output need
   careful testing. If Rust-native support is not good enough, a sidecar may be
   needed, which would reduce some of the simplicity benefit.

2. Metadata parity

   `exifr` handles EXIF/GPS/XMP/IPTC in JavaScript today. A Rust replacement
   needs to preserve capture dates, dimensions, GPS coordinates, and details
   shown on the photo page.

3. Performance can move, not disappear

   Tauri can make the app shell lighter, but a one-to-one port of the current
   scan/render behavior can still feel bad. The spike has to introduce a real
   background work model with bounded CPU use, progress events, cache reuse, and
   responsive cancellation/deferral.

4. Update semantics

   Windows can keep real update installation only if the Tauri updater artifacts
   and signing keys are configured. Mac can keep the current manual update link
   path until an Apple Developer signing/notarization path exists.

## Recommended spike scope

The spike should live off `main` until it proves itself.

1. Add Tauri project scaffolding beside the current Electron app.
2. Keep the current React renderer and introduce a small API adapter so the UI
   can call either Electron or Tauri behind the same `gridMode` contract.
3. Implement the minimum Tauri backend commands:
   - `settings:get`
   - `settings:choose-root`
   - `library:scan`
   - `library:get-home`
   - `library:get-years`
   - `library:get-year`
   - `library:get-month`
   - `settings:clear-cache`
4. Implement thumbnail serving for common image formats first.
5. Measure with a representative library:
   - cold launch time
   - idle memory
   - first scan time
   - repeat scan time
   - thumbnail cache build time
   - UI responsiveness while another CPU-heavy task is running
6. Add update behavior only after the core performance path is proven.

## Exit criteria

The spike is successful only if it proves all of these:

- The app launches and renders the existing UI.
- A user can choose one or more photo folders.
- Home, library, year, month, photo details, exclusions, rescan, and clear cache
  work from the Tauri build.
- The UI stays responsive during scanning and thumbnail generation.
- Installer size, cold launch, and idle memory are materially better than the
  Electron build.
- The Windows and Mac release workflows can be described without losing the
  current update/download behavior.

## My recommendation

Do the Tauri spike now, but do not freeze feature work while it is in flight.
The migration is promising, and the app shape is favorable because the OS bridge
is already concentrated in one place. The thing to guard against is spending
time on a shell swap while the real bottleneck remains image and metadata work.

In parallel, we should still make no-regret Electron performance fixes:

- Lower the priority of background rescans after showing cached library data.
- Cap thumbnail generation more aggressively when the system is under load.
- Consider virtualizing large month grids.
- Avoid starting expensive cache rebuild work while the user is actively
  navigating.
- Add rough timing logs around scan, metadata, and render phases so comparisons
  are grounded in measurements.

## References

- Tauri updater plugin: https://v2.tauri.app/plugin/updater/
- Tauri dialog plugin: https://v2.tauri.app/plugin/dialog/
- Tauri file system plugin: https://v2.tauri.app/plugin/file-system/
- Tauri asset URL API: https://v2.tauri.app/reference/javascript/api/namespacecore/#convertfilesrc
- Tauri GitHub Actions guide: https://v2.tauri.app/distribute/pipelines/github/
- Tauri macOS signing guide: https://v2.tauri.app/distribute/sign/macos/
- Tauri prerequisites: https://v2.tauri.app/start/prerequisites/
