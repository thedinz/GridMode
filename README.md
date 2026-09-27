# GridMode

GridMode is a Windows and macOS desktop photo viewer for people who already like their folder structure. It shows large, square, responsive grids from your photo folders, opens photos into a focused viewer, and builds date and folder views from EXIF data without asking you to reorganize anything. Source photos are never modified.

## Features

- **Home grid**: a random selection of square thumbnails, with an "On this day" row of photos taken on today's date in earlier years. Shuffle for a new selection.
- **Photo viewer**: large image with ←/→ to move through the grid you came from, wheel or double-click zoom, drag to pan, and Esc to go back. The detail panel shows capture date, camera, lens, exposure, dimensions, GPS location (with a link to OpenStreetMap), and the folder path.
- **Library by date**: years, then months, grouped by the EXIF capture date. Photos without EXIF fall back to the file's modified time and are marked "file date".
- **Library by folder**: browse each photo location and its subfolders, with photo counts.
- **Search**: filter by file name, folder, or camera, and by date range.
- **Right-click a photo** to open it, show it in Explorer or Finder, open it with the default app, or copy its path.
- **Stays current**: GridMode watches your photo locations and picks up added, changed, or removed photos in the background.
- **Settings**: multiple photo locations, excluded folders, grid size, rescan, rebuild thumbnails, clear cache, and update checks.

## Supported images

GridMode scans JPEG, JPE, JFIF, PNG, WebP, GIF, BMP, TIFF/TIF, HEIC/HEIF, and AVIF files. Browser-native formats stream directly into the photo viewer. Thumbnails, very large files, and formats a browser cannot display are rendered to cached JPEGs in the app data folder.

When the bundled decoder rejects a file (HEIC, a damaged JPEG, or a file whose extension does not match its contents), GridMode falls back to the operating system's decoder: Windows Imaging Component on Windows, `sips` on macOS. HEIC on Windows needs Microsoft's HEIF Image Extensions.

## How it works

The app is a Tauri 2 shell around a React renderer.

- `src-tauri/src/` is the Rust backend, split by concern: `library.rs` (scanning, the persistent index, queries), `metadata.rs` (EXIF), `render.rs` (thumbnail and display rendering), `protocol.rs` (the `gridmode-photo://` image scheme), `state.rs` (shared state and scan orchestration), `watcher.rs` (folder watching), `settings.rs`, `paths.rs`, `updates.rs`, and `commands.rs` (the commands the renderer calls).
- `src/renderer/` is the React UI: `App.tsx` handles state and navigation, `views/` holds each screen, and `components/` holds shared pieces.
- `src/shared/generated/bindings.ts` is **generated** from the Rust models in `src-tauri/src/model.rs`. Do not edit it by hand; run `pnpm gen:types` after changing a model.

Settings, the library index (`library-index.json`), and the image cache live in the app data folder (`%APPDATA%\com.thedinz.gridmode` on Windows, `~/Library/Application Support/com.thedinz.gridmode` on macOS). A scan reuses indexed metadata for files whose size and modified time are unchanged.

## Development

Prerequisites: Node.js 24+, pnpm 11, and Rust (stable). On Windows you also need the Visual Studio Build Tools with the "Desktop development with C++" workload.

```powershell
pnpm install
pnpm dev
```

To keep a development build away from an installed copy's settings and index, point it at a scratch data folder (debug builds only):

```powershell
$env:GRIDMODE_DATA_DIR = "C:\temp\gridmode-dev"; pnpm dev
```

Checks:

```powershell
pnpm typecheck   # TypeScript
pnpm test        # renderer unit tests (Vitest)
pnpm test:rust   # backend tests; also regenerates the TypeScript bindings
```

CI runs all three on every push and pull request, and fails if the generated bindings are out of date.

## Releasing

Releases are published by the "Release Desktop" GitHub workflow when a version tag is pushed:

```powershell
git tag v0.1.31
git push origin v0.1.31
```

You can also run the workflow manually from the Actions tab with a version number. The version must be higher than the latest published release, because installed copies only auto-update to newer versions. The workflow runs the checks, stamps the version into `package.json`, `tauri.conf.json`, and `Cargo.toml`, builds the Windows NSIS installer and the universal macOS DMG, and publishes them with the Tauri updater manifest (`latest.json`).

### Building installers locally

```powershell
pnpm dist:win
```

The Windows installer is written under `src-tauri/target/release/bundle/nsis/`.

On a Mac, install both Rust targets once, then build the universal release:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm dist:mac
```

The unsigned universal DMG, for both Intel and Apple Silicon Macs, is written under `src-tauri/target/universal-apple-darwin/release/bundle/dmg/`.

## Updates

Automatic updates use Tauri updater artifacts signed with the updater key. The Mac updater publishes one universal app archive for both `darwin-x86_64` and `darwin-aarch64`. If the automatic updater is unavailable, macOS falls back to opening the universal DMG from GitHub Releases.

The updater signature verifies release integrity but is separate from Apple platform signing. Until Developer ID signing and notarization are configured, macOS may ask the user to approve the downloaded app through Gatekeeper. Production-ready macOS builds need Developer ID signing and notarization, and a production-ready Windows build should add code signing.
