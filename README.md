<div align="center">

<img src="packaging/icons/256x256.png" alt="Softcaller" width="112">

# Softcaller

**A calm, fast SIP softphone for every provider — home fiber line, office PBX, SIP trunk, or direct IP call.**

[![CI](https://github.com/habitual69/softcaller/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/habitual69/softcaller/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/habitual69/softcaller)](https://github.com/habitual69/softcaller/releases/latest)
[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20Linux%20%7C%20macOS-555)
![Architectures](https://img.shields.io/badge/arch-x64%20%7C%20ARM64-555)

[Providers](#providers) · [Features](#features) · [How it works](#how-it-works) · [Build from source](#building-from-source) · [Design](#design) · [Website](https://habitual69.github.io/SoftCaller)

</div>

---

## What is Softcaller?

Softcaller turns your computer into a phone. It registers a standard SIP account, places and receives voice
and video calls, keeps your contacts and call history, and lives quietly in the system tray when no call is
up.

It is built for **any standards-compliant SIP provider** — a hosted VoIP service, an Asterisk / FreeSWITCH /
3CX PBX, a SIP trunk, or a direct IP call. There is no provider lock-in. The optional
**Connect JioFiber/AirFiber** flow can also fetch SIP credentials from a compatible home router, for users
who have a fiber voice line.

Softcaller is an independent project created and maintained by
[habitual69](https://github.com/habitual69).

## Providers

| Provider type | How it connects |
|---|---|
| Hosted SIP / VoIP service | Server, username, password, optional proxy; UDP, TCP or TLS |
| Asterisk / FreeSWITCH / 3CX / other PBX | Registrar or domain, auth ID, dial plan; SRTP optional or mandatory |
| SIP trunk | Server, outbound proxy, codec order, caller-ID privacy |
| Direct IP call | Built-in local account; dial `sip:user@host` or use “Call by IP” |
| JioFiber / AirFiber | Optional **Connect JioFiber/AirFiber** in Settings → Accounts |

Default codecs are chosen for broad compatibility and for AMR-only fiber lines:
**PCMA, PCMU, G.722, Opus, AMR-WB, AMR**. Presence, messages, transfer and video start on and can be
switched off in Settings.

## Features

### Calling

- Voice and video calls, with remote video and a picture-in-picture preview drawn inside the window; full screen; add video to an audio call
- Several calls at once, call waiting, hold, mute, and local conferences
- Blind and attended transfer (SIP REFER or the PBX’s feature codes) and call pickup
- DTMF over RFC 2833, SIP INFO or in-band audio (automatic by default)
- Call recording to WAV, by hand or automatically
- Auto-answer, do not disturb, call forwarding and auto hang-up
- Dial plan rules, a dialling prefix, and caller-ID privacy
- Call quality at a glance: codec, bitrate and an estimated MOS

### Messaging and presence

- SIP text messages, kept per conversation next to that contact’s calls
- Presence and busy-lamp field (BLF) for contacts, with pickup of a ringing colleague
- Your own status: Online, Away when idle, Busy in do not disturb
- Voicemail indicator (MWI) with a one-click voicemail button

### Contacts and history

- Contacts with search and favourites; vCard (`.vcf`) import and export
- A users directory (JSON, XML, Yealink or CSV from a URL), kept in sync
- Call history with filters and CSV export, missed-call badges and notifications
- Speed-dial buttons, with presence lights

### Audio

- Microphone, speaker and ringtone devices chosen by name, followed when devices are plugged in or out
- Echo cancellation, voice activity detection, codec order, and level meters
- USB telephony headsets (hook switch, mute and LEDs) and media keys or Bluetooth headset buttons

### Desktop integration

- A collapsible left sidebar for navigation, with Settings, accounts and providers all opening in the main screen
- Lives in the tray; optional start with the system, hidden
- Opens `sip:`, `tel:` and `callto:` links from browsers and other apps
- Command line and single instance: `softcaller sip:1001@pbx.example`, `softcaller /answer`, `softcaller /hangupall`, …
- Command hooks that run your own programs on call events
- Light and dark themes that follow the system, and a built-in SIP debugger with logs, SIP messages, RTP statistics and audio meters

### Accounts and security

- SIP over UDP, TCP or TLS, with SRTP (SDES, DTLS) for encrypted media
- STUN and ICE for NAT traversal, outbound proxies and DNS SRV
- Passwords in the operating system’s credential store (Windows Credential Manager, macOS Keychain, the Secret Service on Linux), never in a file
- A local “call by IP” account for direct calls without a server

**Codecs:** G.711 (PCMA/PCMU), G.722, AMR-NB, AMR-WB, Speex, iLBC, GSM and G.722.1 for audio; H.264 for video.

## Quickstart

### Download

Installers are built by CI for every platform and attached to a `v*` release:

| System | Download | Install |
|---|---|---|
| Windows 10/11 | `softcaller_<version>_x64-setup.exe` or `softcaller_<version>_arm64-setup.exe` | Run it; it installs for your user, no administrator rights needed |
| | `softcaller_<version>_x64_en-US.msi` or `softcaller_<version>_arm64_en-US.msi` | For managed or per-machine deployment |
| Ubuntu 24.04+ / Debian 13+ | `softcaller_<version>_amd64.deb` or `softcaller_<version>_arm64.deb` | `sudo apt install ./softcaller_<version>_<arch>.deb` |
| Other Linux | `softcaller_<version>_x86_64.AppImage` or `softcaller_<version>_aarch64.AppImage` | `chmod +x softcaller_*.AppImage`, then run it |
| macOS 13+ | `Softcaller_<version>_aarch64.dmg` or `Softcaller_<version>_x64.dmg` | Open it and drag Softcaller to Applications |

Every system is built for both 64-bit Intel/AMD (x64) and 64-bit ARM. Until the release builds are
code-signed, Windows SmartScreen may warn about an unknown publisher, and macOS may ask you to open the app
the first time with right-click → **Open**.

### First account

1. Start **Softcaller**. It opens on the dialer and adds an icon to the tray.
2. Open **Settings → General → Accounts**.
3. **Add account** for any SIP provider, or **Connect JioFiber/AirFiber** for a compatible router.
4. When the status turns green (**Registered**), type a number and press the call button.

## How it works

```text
┌──────────────────────────── softcaller (egui + fastframe) ───────────────────────────┐
│  Dialer · Calls · Contacts · Messages · Settings · Debug         tray · links · hooks │
└─────────────────────────────┬────────▲────────────────────────────────────────────────┘
                              │        │ events
                     commands ▼        │
┌──────────────────────────── sip-engine (engine thread) ────────────────────────────┐
│  owns pjsua: accounts, calls, media, presence, messages, video taps, device watch  │
└──────────────────────────────────────┬─────────────────────────────────────────────┘
                                       │ FFI (pjsua-sys, bindgen)
                                       │
┌──────────────────────────── pjproject (statically linked) ──────────────────────────┐
│  SIP · RTP/SRTP · ICE/STUN · TLS · audio devices · codecs · video conference bridge │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

- **pjproject does the telephony.** Softcaller links `pjsua` statically. It builds against
  [JFC-pjproject at a pinned commit](https://github.com/JFC-Group/JFC-pjproject/tree/bf04f99c08ba4a2547773362888d042729ebe9ea),
  which adds the `+sip.instance` parameter that JioFiber/AirFiber routers expect.
- **One thread owns the SIP stack.** `sip-engine` runs pjsua on a dedicated thread. The rest of the app
  sends it commands and receives events, so the interface never waits on the network and calls keep
  working while no window is open.
- **The interface is a view over the app’s state.** The window is drawn with
  [egui](https://github.com/emilk/egui); [fastframe](https://github.com/crmne/fastframe) provides the
  tray-resident shell, single instance, notifications, theming and other desktop services.
- **Video is drawn in the window.** Decoded video frames are tapped from pjsua’s video conference bridge,
  converted to RGBA and drawn by egui.
- **Your data stays local.** Settings are TOML; contacts, history and messages are in a SQLite database;
  passwords are in the system credential store.

### Repository layout

| Crate | Purpose |
| --- | --- |
| [`crates/softcaller-app`](crates/softcaller-app) | The desktop app (`softcaller`): interface, tray, notifications, integrations |
| [`crates/sip-engine`](crates/sip-engine) | Safe, threaded wrapper around pjsua |
| [`crates/pjsua-sys`](crates/pjsua-sys) | Raw bindings to pjsua, link setup, and the C shim |
| [`crates/softcaller-core`](crates/softcaller-core) | The app model: settings, accounts, contacts, history, rules (no GUI, no unsafe code) |
| [`crates/softcaller-win`](crates/softcaller-win) / [`crates/softcaller-mac`](crates/softcaller-mac) | Windows and macOS integrations that need system APIs |
| [`crates/softcaller-cli`](crates/softcaller-cli) | `softcaller-cli`, a command-line tool to test a SIP account without the app |
| [`packaging`](packaging) | Installer configuration, icons and signing scripts |

## Building from source

Softcaller is a Cargo workspace. Building it takes two steps: build pjproject (with its codec and TLS
libraries), then point Cargo at it. Use JFC-pjproject at the pinned commit shown below; plain pjproject
builds a Softcaller that a JioFiber/AirFiber router won’t accept. The
[CI workflow](.github/workflows/platform.yml) does exactly this for Windows, Linux and macOS on both x64
and ARM64, and is the reference recipe.

You need [Rust](https://rustup.rs) 1.98 or newer (stable), Git, and the platform’s C toolchain.

### Linux (Ubuntu 24.04 / Debian 13, x64 or ARM64)

```sh
git clone https://github.com/habitual69/softcaller && cd softcaller

# Build tools and libraries (OpenSSL, SDL2, OpenH264, AMR, Opus, ALSA, V4L2, GTK, …)
bash .github/scripts/linux-deps.sh

# JFC-pjproject at the pinned commit, installed into ../pjproject-install
git clone https://github.com/JFC-Group/JFC-pjproject ../pjproject
git -C ../pjproject checkout bf04f99c08ba4a2547773362888d042729ebe9ea
bash .github/scripts/build-pjproject.sh ../pjproject ../pjproject-install crates/pjsua-sys/config_site.h

export PJPROJECT_DIR=$PWD/../pjproject-install
cargo run --release
```

### macOS (Apple silicon or Intel)

Install the Xcode command line tools and [Homebrew](https://brew.sh), then:

```sh
git clone https://github.com/habitual69/softcaller && cd softcaller
bash .github/scripts/macos-deps.sh

git clone https://github.com/JFC-Group/JFC-pjproject ../pjproject
git -C ../pjproject checkout bf04f99c08ba4a2547773362888d042729ebe9ea
bash .github/scripts/build-pjproject.sh ../pjproject ../pjproject-install crates/pjsua-sys/config_site.h

export PJPROJECT_DIR=$PWD/../pjproject-install
cargo run --release
```

### Windows

You need Visual Studio 2022 or newer with the **Desktop development with C++** workload (and its ARM64
build tools on ARM). pjproject is built with MSBuild against a dependencies folder holding OpenSSL 3, SDL2,
OpenH264 (static, `/MT`), Opus and the OpenCORE AMR / VisualOn AMR-WB libraries (built with MSYS2: GCC on x64,
clang on ARM64). The `deps` job in [platform.yml](.github/workflows/platform.yml) shows how each one is
built for each architecture. With those in a folder laid out as `include/`, `lib/` and `bin/`:

```powershell
git clone https://github.com/habitual69/softcaller; cd softcaller
git clone https://github.com/JFC-Group/JFC-pjproject ..\pjproject
git -C ..\pjproject checkout bf04f99c08ba4a2547773362888d042729ebe9ea
Copy-Item crates\pjsua-sys\config_site.h ..\pjproject\pjlib\include\pj\config_site.h
```

Add a `Directory.Build.props` next to `pjproject-vs14.sln` that adds the dependencies’ `include` and `lib`
folders (see the `pjproject` job in the workflow), then, from a Developer PowerShell:

```powershell
# /p:Platform=ARM64 on Windows on ARM
msbuild ..\pjproject\pjproject-vs14.sln /t:libpjproject /p:Configuration=Release-Static /p:Platform=x64 /m

$env:PJPROJECT_DIR = "$PWD\..\pjproject"          # defaults to ..\pjproject
$env:SOFTCALLER_DEPS_DIR = "C:\path\to\deps"      # defaults to ..\..\deps
cargo run --release
```

The build copies `SDL2.dll` and the OpenSSL DLLs next to `softcaller.exe`.

### Tests and checks

```sh
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

Some engine tests place real calls over the loopback interface or need a camera; they are ignored by
default and run with `cargo test -- --ignored`.

### Installers

Installers are made with [cargo-packager](https://github.com/crabnebula-dev/cargo-packager); see
[`packaging/README.md`](packaging/README.md). Pushing a `v` version tag matching the version in
`Cargo.toml` builds every platform’s installers in CI and publishes a GitHub Release with them.

## Acknowledgements

Softcaller stands on these projects:

- [pjproject](https://www.pjsip.org) by Teluu, with libsrtp, libyuv, Speex, WebRTC audio processing, iLBC, GSM and G.722.1
- [OpenH264](https://www.openh264.org) by Cisco
- [opencore-amr](https://sourceforge.net/projects/opencore-amr/) and VisualOn’s AMR-WB encoder
- [OpenSSL](https://www.openssl.org), TLS and SRTP key exchange
- [SDL](https://www.libsdl.org), pjproject’s video device layer
- [egui and eframe](https://github.com/emilk/egui), with egui_plot and egui_kittest
- [fastframe](https://github.com/crmne/fastframe) by Carmine Paolino, the app shell, tray and desktop services
- [winit](https://github.com/rust-windowing/winit)
- [Lucide](https://lucide.dev) icons and the [Inter](https://rsms.me/inter/) typeface
- [resvg](https://github.com/linebender/resvg), SVG rendering of the logo
- [rfd](https://github.com/PolyMeilex/rfd), file dialogs
- [rusqlite](https://github.com/rusqlite/rusqlite) and SQLite
- [keyring-core](https://github.com/open-source-cooperative/keyring-core) and its platform stores
- [hidapi](https://github.com/ruabmbua/hidapi-rs), USB headsets
- [windows-rs](https://github.com/microsoft/windows-rs), [winreg](https://github.com/gentoo90/winreg-rs) and [objc2](https://github.com/madsmtm/objc2)
- [ureq](https://github.com/algesten/ureq), [serde](https://serde.rs), [toml](https://github.com/toml-rs/toml), [roxmltree](https://github.com/RazrFalcon/roxmltree), [regex](https://github.com/rust-lang/regex), [jiff](https://github.com/BurntSushi/jiff), [realfft](https://github.com/HEnquist/realfft), [user-idle](https://github.com/olback/user-idle-rs), [if-addrs](https://github.com/messense/if-addrs), [clap](https://github.com/clap-rs/clap) and [log](https://github.com/rust-lang/log)
- [bindgen](https://github.com/rust-lang/rust-bindgen) and [cc](https://github.com/rust-lang/cc-rs)
- [cargo-packager](https://github.com/crabnebula-dev/cargo-packager), with NSIS, WiX, linuxdeploy and dylibbundler
- [winresource](https://github.com/BenjaminRi/winresource), [MSYS2](https://www.msys2.org), [vcpkg](https://github.com/microsoft/vcpkg), [meson](https://mesonbuild.com) and [Homebrew](https://brew.sh)

## License

Softcaller is free software, released under the [GNU General Public License v3.0](LICENSE). It statically
links pjproject, which is GPL. The codecs it includes are covered by their own licenses, and H.264 and AMR
may be subject to patents in some countries; check what applies to you before distributing builds.

Softcaller is Copyright 2026 habitual69 and contributors.

Softcaller is an independent community project created and maintained by
[habitual69](https://github.com/habitual69). It isn’t made, endorsed or supported by Reliance Jio;
JioFiber and AirFiber are its trademarks, used here only to say which routers it works with.
