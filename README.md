# GridMode

GridMode is a Windows and macOS desktop photo viewer for people who already like their folder structure. It shows large, square, responsive grids from a chosen photo directory, opens photos into a focused detail view, and builds library views from EXIF dates without asking you to reorganize anything.

## First Version

- Home grid with randomized square thumbnails from the selected photo locations.
- Photo detail view with a large image and extracted EXIF/date/location metadata.
- Scan-time JPEG thumbnail pre-generation for instantly available grids, including TIFF/TIF support through a native image pipeline.
- Persistent library indexing so startup can reuse cached metadata and only process new or modified photos.
- Library view grouped by year, then month, using EXIF dates with file modified time as a fallback.
- Settings view for choosing, adding, rescanning, excluding photo locations, clearing generated caches, and rebuilding all thumbnails.
- Windows NSIS and universal macOS DMG installers built with Tauri.
- Migration release support for existing Windows Electron installs through GitHub Releases.

## Supported Images

GridMode scans JPEG, JPE, JFIF, PNG, WebP, GIF, BMP, TIFF/TIF, HEIC/HEIF, and AVIF files. Browser-native formats stream directly for the large photo view, while thumbnails and non-browser-native formats are rendered into cached JPEG images under the app data folder.

Thumbnail generation uses the native Windows or macOS image codec as a fallback when the primary decoder rejects a damaged JPEG or a file whose extension does not match its contents. Source photos are never modified; recovered renders are written only to GridMode's cache.

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

Install both Rust targets once on the Mac:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

Then build the universal release:

```bash
pnpm dist:mac
```

The unsigned universal DMG, supporting both Intel and Apple Silicon Macs, is written under `src-tauri/target/universal-apple-darwin/release/bundle/dmg/`. The DMG is uploaded to GitHub Releases for manual macOS downloads.

## Updates

The first Tauri Windows release also publishes an Electron-compatible `latest.yml` beside the Tauri NSIS installer. Existing Windows Electron installs use that file to discover and download the migration installer.

Automatic updates use Tauri updater artifacts protected by the updater signing key. The Mac updater publishes one universal app archive for both `darwin-x86_64` and `darwin-aarch64`, so Intel installations and native Apple Silicon installations update to the same universal app. If the automatic updater is unavailable, macOS falls back to opening the universal DMG from GitHub Releases.

The updater signature verifies release integrity but is separate from Apple platform signing. Until Developer ID signing and notarization are configured, macOS may still require the user to approve the downloaded app through Gatekeeper.

Local unsigned installers are fine for early testing. Production-ready macOS builds need Developer ID signing and notarization, and a production-ready Windows build should eventually add code signing.
