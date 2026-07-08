# GridMode

GridMode is a Windows and macOS desktop photo viewer for people who already like their folder structure. It shows large, square, responsive grids from a chosen photo directory, opens photos into a focused detail view, and builds library views from EXIF dates without asking you to reorganize anything.

## First Version

- Home grid with randomized square thumbnails from the selected photo directory.
- Photo detail view with a large image and extracted EXIF/date/location metadata.
- Cached thumbnail rendering for faster grids, including TIFF/TIF support through a native image pipeline.
- Persistent library indexing so startup can reuse cached metadata and only process new or modified photos.
- Library view grouped by year, then month, using EXIF dates with file modified time as a fallback.
- Settings view for choosing, rescanning, and excluding folders from the photo directory.
- Windows NSIS and macOS DMG/ZIP installers built with Electron Builder.
- Auto-update plumbing through GitHub releases.

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

## Build A Mac Installer

```powershell
pnpm dist:mac
```

The DMG, ZIP, blockmaps, and `latest-mac.yml` are written to `release/`. The ZIP is required for macOS auto-updates.

## Updates

Installed builds check GitHub releases for updates on startup and from the Settings page. The included GitHub Actions workflow publishes a new Windows release for every push to `main` by assigning the build a run-based version such as `0.1.42`. It also publishes signed macOS DMG/ZIP update assets when the repository has `CSC_LINK` and `CSC_KEY_PASSWORD` signing secrets configured.

macOS auto-updates require a signed app. For notarized public Mac releases, also configure `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD`, and `APPLE_TEAM_ID` secrets.

Local unsigned installers are fine for early testing. Production-ready macOS builds need Developer ID signing and notarization, and a production-ready Windows build should eventually add code signing.
