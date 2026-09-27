# GridMode Tauri Cutover

GridMode now treats the Tauri build as the primary desktop app. The Electron
version is preserved on the `archive/electron-version` branch.

## Current identity

- Product name: `GridMode`
- Bundle identifier: `com.thedinz.gridmode`
- Rust crate: `gridmode`
- First migration version: `0.1.9`

## Electron installs

Electron builds check GitHub Releases for an Electron Builder `latest.yml`.
This repository does not generate that file: there is no script for it and
the release workflow does not publish one. Electron installs therefore do not
find Tauri releases on their own, and those users need to download and run the
Tauri installer from GitHub Releases once. After that, the Tauri updater keeps
them current.

If an automatic migration is needed later, add a step to the release workflow
that writes an Electron Builder `latest.yml` (version, installer file name,
SHA-512, and size) next to the NSIS installer. Test it on a Windows machine
with an Electron install before relying on it, because Tauri's NSIS installer
is not the installer Electron Builder produced.

## Updates

Automatic updates use Tauri updater artifacts and signing keys. The macOS
manifest maps both Intel and Apple Silicon updater targets to the same
universal app archive, so existing Intel installations update directly to the
universal build and native Apple Silicon installations receive the same build.

If the automatic updater is unavailable, macOS falls back to opening the
universal DMG from GitHub Releases until Developer ID signing and notarization
are configured.

## Library index format

Version 2 of `library-index.json` added EXIF capture dates, camera, GPS, and
dimensions. The first launch after upgrading from an index version 1 build
rescans the library once to read that metadata. Cached thumbnails are keyed
independently and are reused.
