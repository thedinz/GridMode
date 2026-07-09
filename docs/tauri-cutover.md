# GridMode Tauri Cutover

GridMode now treats the Tauri build as the primary desktop app. The Electron
version is preserved on the `archive/electron-version` branch.

## Current identity

- Product name: `GridMode`
- Bundle identifier: `com.thedinz.gridmode`
- Rust crate: `gridmode`
- First migration version: `0.1.9`

## Windows migration release

Existing Windows users have an Electron build that checks GitHub Releases for an
Electron Builder `latest.yml`. The first Tauri release must keep that feed alive
by uploading:

- The Tauri NSIS installer from `src-tauri/target/release/bundle/nsis/`.
- A generated `latest.yml` from `scripts/create-electron-updater-yml.mjs`.

That lets the existing Electron updater discover a newer version and download
the Tauri installer. This should still be validated on a Windows machine before
calling the migration release done, because Tauri's NSIS installer is not the
same installer generator Electron Builder used.

## Future updates

After Windows users have migrated to the Tauri app, automatic updates should be
implemented with Tauri updater artifacts and signing keys. Until that is wired
in, the in-app update controls report that automatic updates are not configured.

macOS remains on a manual GitHub Releases download path until Developer ID
signing and notarization are configured.
