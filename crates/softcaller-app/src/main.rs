//! Softcaller: a SIP softphone built on pjproject, egui and fastframe.

// No console window for release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autostart;
mod calls;
mod directory;
mod headset;
mod hookrun;
mod icons;
mod idle;
mod jiofiber;
mod media_keys;
mod netwatch;
mod phonebook;
mod sip;
mod theme;
mod tray;
mod urlhandler;
mod ui;
mod window;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fastframe_instance::{Claim, Slot};
use fastframe_shell::{Shell, Waker};
use softcaller_core::command::Verb;
use softcaller_core::settings::{self, Settings};

use crate::app::{App, Paths, Request, Requests};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const INSTANCE_ID: &str = "io.github.habitual69.softcaller";
/// The Linux desktop entry and icon name the packages install
/// (`softcaller.desktop`); the window's app id and the media controls' and
/// notifications' desktop entry must match it.
pub const DESKTOP_ENTRY: &str = "softcaller";

const DIRS: fastframe_dirs::App = fastframe_dirs::App {
    // Not copied into a portable folder: the running copy's lock and logs.
    not_copied: &["instance", "softcaller.log", "panic.log"],
    ..fastframe_dirs::App::new("softcaller", "io.github", "habitual69", APP_FOLDER)
};
/// The data folders' name under `%APPDATA%\habitual69` and `%LOCALAPPDATA%\habitual69`.
const APP_FOLDER: &str = "Softcaller";

#[derive(Debug, Default)]
struct Args {
    /// Start in the tray.
    minimized: bool,
    /// What the rest of the command line asks for: a number or link to
    /// dial, or a verb (`/answer`, `msip:hold`…).
    verb: Option<Verb>,
    /// That command line, to hand to a running copy.
    command: String,
}

impl Args {
    fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut out = Self::default();
        let mut rest = Vec::new();
        for arg in args {
            match arg.to_ascii_lowercase().as_str() {
                "--minimized" | "/minimized" | "-m" => out.minimized = true,
                _ => rest.push(arg),
            }
        }
        out.command = rest.join(" ");
        out.verb = softcaller_core::command::parse(&out.command);
        out
    }
}

/// Prefix of a request carrying a command line to the running copy.
const COMMAND: &str = "cmd ";

fn main() -> ExitCode {
    let args = Args::parse(std::env::args().skip(1));
    let waker = Waker::default();
    let requests = Requests::default();

    // One running copy per user (and per portable folder). A second launch
    // asks the running copy to show its window, then exits.
    let mut slot = Slot::new(INSTANCE_ID);
    if let Some(scope) = DIRS.portable().instance_scope() {
        slot = slot.scoped(&scope);
    }
    let (handler_requests, handler_waker) = (requests.clone(), waker.clone());
    let request = match &args.verb {
        Some(_) => format!("{COMMAND}{}", args.command),
        None if args.minimized => "ping".to_owned(),
        None => "show".to_owned(),
    };
    let _instance = match slot.claim(&request, move |request| {
        let push = |r: Request| {
            handler_requests.lock().unwrap_or_else(|e| e.into_inner()).push(r);
            handler_waker.wake();
        };
        match request {
            "show" => push(Request::Show),
            "ping" => {}
            _ => match request.strip_prefix(COMMAND).and_then(softcaller_core::command::parse) {
                Some(verb) => push(Request::Verb(verb)),
                None => return None,
            },
        }
        Some("ok".to_owned())
    }) {
        Claim::First(guard) => guard,
        Claim::Running(_) | Claim::Declined => return ExitCode::SUCCESS,
        Claim::Unanswered => return ExitCode::FAILURE,
    };
    // macOS sends clicked links to the running app as Apple Events rather
    // than starting a second copy with the link as an argument.
    #[cfg(target_os = "macos")]
    {
        let (link_requests, link_waker) = (requests.clone(), waker.clone());
        softcaller_mac::on_open_url(move |url| {
            if let Some(verb) = softcaller_core::command::parse(&url) {
                link_requests.lock().unwrap_or_else(|e| e.into_inner()).push(Request::Verb(verb));
                link_waker.wake();
            }
        });
    }
    // Nothing is running: call verbs have nothing to act on;
    // a number or link is dialled once the app is up.
    match args.verb {
        Some(Verb::Exit | Verb::Answer | Verb::HangupAll | Verb::HangupIncoming) => return ExitCode::SUCCESS,
        Some(verb) => requests.lock().unwrap_or_else(|e| e.into_inner()).push(Request::Verb(verb)),
        None => {}
    }

    let found = DIRS.discover(|_, _| Ok(()));
    let dirs = found.dirs.unwrap_or_else(fallback_dirs);
    if let Err(e) = fastframe_log::Logging::new("softcaller", VERSION)
        .filter("warn,softcaller_app=info,sip_engine=info")
        .file(dirs.state.join("softcaller.log"))
        .panic_log(dirs.state.join("panic.log"))
        .redact(sip::app_log_tap)
        .init()
    {
        eprintln!("logging unavailable: {e}");
    }
    log::info!("data folders: {:?}", found.mode);

    let settings_path = dirs.config.join(settings::FILE_NAME);
    let settings = load_settings(&settings_path);

    fastframe_titlebar::init();
    let start_hidden = args.minimized || settings.ui.start_minimized;
    let paths = Paths { settings: settings_path, sounds: dirs.cache.join("sounds"), sip_log: dirs.state.join("sip.log"), database: dirs.config.join(softcaller_core::store::FILE_NAME) };
    let app = App::new(settings, paths, requests, &waker);

    let shell = Shell::new(app, &waker).start_hidden(start_hidden);
    // macOS needs AppKit's loop to keep the menu-bar item alive; elsewhere
    // the shell's own sleep answers window messages.
    #[cfg(target_os = "macos")]
    let shell = shell.idle(fastframe_tray::idle);

    let result = shell.run(|lease| {
        let options = lease.peek(window::native_options);
        eframe::run_native(
            "Softcaller",
            options,
            Box::new(move |cc| {
                let mut app = lease.take(&cc.egui_ctx);
                app.attach(&cc.egui_ctx);
                Ok(Box::new(window::Window::new(app)))
            }),
        )
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("window failed: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Settings from disk. An unreadable file is kept aside and replaced by defaults.
fn load_settings(path: &Path) -> Settings {
    match Settings::load(path) {
        Ok(mut settings) => {
            if settings.grow_window_to_default() {
                // Saved now, so the step runs once.
                if let Err(e) = fastframe_file::write(path, settings.to_toml()) {
                    log::warn!("cannot save {}: {e}", path.display());
                }
            }
            settings
        }
        Err(e) => {
            let aside = path.with_extension("toml.bad");
            log::warn!("{e}; keeping it as {} and using defaults", aside.display());
            let _ = std::fs::rename(path, aside);
            Settings::default()
        }
    }
}

/// Used only when no per-user folder exists: a folder beside the executable.
fn fallback_dirs() -> fastframe_dirs::Dirs {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    fastframe_dirs::Dirs::inside(&exe_dir.join("softcaller-data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimized_switch_accepts_every_spelling() {
        for arg in ["--minimized", "/minimized", "/MINIMIZED", "-m"] {
            assert!(Args::parse([arg.to_owned()]).minimized, "{arg}");
        }
        assert!(!Args::parse(["sip:123@example.com".to_owned()]).minimized);
    }
}
