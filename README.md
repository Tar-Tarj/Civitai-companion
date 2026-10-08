# Civitai Companion

Lightweight Windows 10/11 x64 desktop companion for Civitai, built with Tauri 2, Rust and vanilla TypeScript.

## Downloads

Version 1.0.23 is provided in three forms:

- NSIS setup executable for normal installation;
- MSI package for Windows Installer deployments;
- portable executable for users who do not want to install the application.

Public builds are currently unsigned, so Windows SmartScreen may display a warning. Verify the SHA-256 values supplied with the release assets before running a download.

## Local development

Prerequisites:

- Rust stable with the `x86_64-pc-windows-msvc` target
- Microsoft Visual C++ Build Tools
- Node.js and npm
- Microsoft Edge WebView2 Runtime

Install and verify:

```powershell
npm ci
npm run check
npm test
cargo test --manifest-path src-tauri/Cargo.toml
npm run desktop:build
```

The release build creates Windows bundles under `src-tauri/target/release/bundle/`.

## Runtime model

- Rust owns all Civitai network access, synchronization, notification rules and persisted state.
- The WebView receives a sanitized `AppSnapshot`; credentials never cross back into the frontend.
- Civitai avatars and thumbnails are fetched by a credential-free Rust image path, restricted to the exact Civitai image/CDN hosts, validated, size-limited and delivered to the WebView as local Blob URLs.
- The account summary shows only positive blue, yellow and green Buzz balances, compacting all three into the existing card size.
- Clicking the Buzz balance card opens an on-demand, color-filtered transaction ledger for the previous and current month. Image-linked rows receive a safe local preview lookup and open only on `civitai.red`.
- Every user-facing Civitai destination is validated and canonicalized to `https://civitai.red` before opening the default browser.
- The default window is 430 pixels wide; both width and height remain user-resizable.
- Windows notification toasts are cleared from this app's notification history after four seconds.
- Closing the main window hides it. Use the tray menu to show, synchronize or quit.
- Start with Windows is opt-in and disabled by default.

## Persistence

The Civitai API key is stored as `Civitai Companion Desktop / civitai-api-key` through Windows Credential Manager.
Non-secret state is stored as an atomic, versioned `state-v1.json` file in the per-user application local-data directory.
Preference exports contain only the versioned `Settings` structure and are written atomically to the user's Downloads directory. Imports are limited to 1 MiB and preserve the current Start with Windows setting.

No automatic migration from the extension is performed. No updater, telemetry, analytics, browsing data, cookies or browser bridge is included.

## Source availability

The complete application source and dependency lockfiles are included for inspection and reproducible local builds. Development credentials, local state, build caches and user data are never included.

No license has been selected yet. The source is public for transparency, but publication alone does not grant redistribution or modification rights until a license file is added.
