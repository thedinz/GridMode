# GridMode

GridMode is a Windows and macOS desktop photo viewer for people who already like their folder structure. It shows large, square, responsive grids from a chosen photo directory, opens photos into a focused detail view, and builds library views from EXIF dates without asking you to reorganize anything.

## First Version

- Home grid with randomized square thumbnails from the selected photo locations.
- Photo detail view with a large image and extracted EXIF/date/location metadata.
- Scan-time thumbnail pre-generation for instantly available grids, including TIFF/TIF support through a native image pipeline.
- Persistent library indexing so startup can reuse cached metadata and only process new or modified photos.
- Library view grouped by year, then month, using EXIF dates with file modified time as a fallback.
- Settings view for choosing, adding, rescanning, excluding photo locations, clearing generated caches, and rebuilding all thumbnails.
- Windows NSIS and macOS DMG installers built with Tauri.
- Migration release support for existing Windows Electron installs through GitHub Releases.

## Supported Images

GridMode scans JPEG, JPE, JFIF, PNG, WebP, GIF, BMP, TIFF/TIF, HEIC/HEIF, and AVIF files. Browser-native formats stream directly for the large photo view, while thumbnails and non-browser-native formats are rendered into cached WebP images under the app data folder.

## Development

```powershell
pnpm install
pnpm dev
```

## Build A Windows Installer

```powershell
pnpm dist:win
```

The installer is written to `release/`.
The Tauri installer is written under `src-tauri/target/release/bundle/nsis/`.

## Build A Mac Installer

```powershell
pnpm dist:mac
```

The unsigned DMG is written under `src-tauri/target/release/bundle/dmg/`. The DMG is uploaded to GitHub Releases for manual macOS downloads.

## Updates

The first Tauri Windows release also publishes an Electron-compatible `latest.yml` beside the Tauri NSIS installer. Existing Windows Electron installs use that file to discover and download the migration installer.

After the migration release, future automatic updates should move to Tauri updater artifacts and signing keys. macOS builds are unsigned, so they stay on the manual GitHub Releases download path until Developer ID signing and notarization are configured.

Local unsigned installers are fine for early testing. Production-ready macOS builds need Developer ID signing and notarization, and a production-ready Windows build should eventually add code signing.
