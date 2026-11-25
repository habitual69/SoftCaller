# Packaging

Softcaller's installers are built with [cargo-packager](https://github.com/crabnebula-dev/cargo-packager)
from the configuration in `crates/softcaller-app/Cargo.toml` (`[package.metadata.packager]`).
The CI builds them for every push (artifacts `packages-<platform>`) and, for a `v*` tag
matching the crate version, publishes a GitHub Release with them.

To build every platform on demand, run the **Build all platforms** workflow
(`.github/workflows/build-all.yml`) from the Actions tab. It calls the same six
platform lanes unconditionally and uploads one `packages-<platform>` artifact per
platform; `ci.yml` remains the change-aware push/tag workflow.

| Platform | Formats | Notes |
|---|---|---|
| Windows x64, ARM64 | NSIS setup (per user), MSI | `SDL2.dll` and the OpenSSL DLLs are installed next to `softcaller.exe`. Both installers register `sip:`/`tel:`/`callto:`. |
| Linux x64, ARM64 | `.deb`, AppImage | The `.deb` depends on Ubuntu 24.04 / Debian 13 packages; the AppImage bundles the libraries. |
| macOS Apple silicon, Intel | `.dmg` | `bundle.sh` copies the Homebrew libraries into the app and signs it. |

Each is built natively on a runner of its architecture.

## Building locally

Build the release binaries first, then (`cargo install cargo-packager --locked`). The output
folder must be absolute: cargo-packager runs in the crate's folder.

```sh
# Windows (PowerShell)
cargo packager -p softcaller-app --release -f nsis,wix -o $PWD/dist/packages

# Linux (strip first: the release profile keeps debug info)
strip target/release/softcaller
APPIMAGE_EXTRACT_AND_RUN=1 cargo packager -p softcaller-app --release -f deb,appimage -o "$PWD/dist/packages"

# macOS
bash packaging/macos/icns.sh
cargo packager -p softcaller-app --release -f app -o "$PWD/dist/packages"
bash packaging/macos/bundle.sh dist/packages/Softcaller.app dist/packages/Softcaller.dmg
```

### Windows NSIS only

`packaging/windows/softcaller.nsi` is a standalone per-user installer that does not need
cargo-packager. It installs under `%LOCALAPPDATA%\Programs\Softcaller`, creates Start Menu and
optional desktop shortcuts, registers `sip:`/`tel:`/`callto:` links, and keeps the user's
settings, recordings and logs on uninstall. Install NSIS once, then:

```powershell
winget install NSIS.NSIS
pwsh packaging/windows/build-nsis.ps1
```

The output is `dist/Softcaller-<version>-x64-setup.exe`. Use `-SkipBuild` to package an existing
`target/release/softcaller.exe`, or `-BinaryDir` to choose another build.

## Files

* `icons/`: the logo (`assets/softcaller.svg`) rendered at 16–1024 px, and `softcaller.ico`, which
  is also embedded in `softcaller.exe` (`crates/softcaller-app/build.rs`).
* `linux/softcaller.desktop`: the desktop entry template (categories, link schemes).
* `macos/Info.plist`: merged into the generated one (microphone and camera usage texts).
* `macos/entitlements.plist`: hardened-runtime entitlements for signed builds.
* `macos/icns.sh`: makes `icons/softcaller.icns` from the PNGs (`iconutil`).
* `macos/bundle.sh`: bundles the libraries, signs, and makes the `.dmg`.
* `windows/main.wxs`: cargo-packager's MSI template with ARM64 added.
* `windows/softcaller.nsi`: the standalone per-user NSIS installer.
* `windows/build-nsis.ps1`: builds it from the release binary.
* `windows/sign.ps1`: Authenticode signing with `signtool`.

## Signing

Unsigned packages work, with the usual warnings (SmartScreen on Windows; on macOS the app has
to be opened with right-click → Open the first time). The CI signs automatically once these
repository secrets exist:

| Secret | Use |
|---|---|
| `WINDOWS_CERTIFICATE` | Base64 of the Authenticode `.pfx` |
| `WINDOWS_CERTIFICATE_PASSWORD` | Its password |
| `MACOS_CERTIFICATE` | Base64 of the "Developer ID Application" `.p12` |
| `MACOS_CERTIFICATE_PASSWORD` | Its password |
| `MACOS_SIGNING_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` |
| `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` | Notarization (app-specific password) |
