# GridMode

GridMode is a Windows-first desktop photo viewer for people who already like their folder structure. It shows large, square, responsive grids from a chosen photo directory, opens photos into a focused detail view, and builds library views from EXIF dates without asking you to reorganize anything.

## First Version

- Home grid with randomized square thumbnails from the selected photo directory.
- Photo detail view with a large image and extracted EXIF/date/location metadata.
- Library view grouped by year, then month, using EXIF dates with file modified time as a fallback.
- Settings view for choosing and rescanning the photo directory.
- Windows NSIS installer built with Electron Builder.
- Auto-update plumbing through GitHub releases.

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

## Updates

Installed builds check GitHub releases for updates on startup and from the Settings page. The included GitHub Actions workflow publishes a new Windows release for every push to `main` by assigning the build a run-based version such as `0.1.42`.

Local unsigned installers are fine for early testing. A production-ready Windows build should eventually add code signing.
