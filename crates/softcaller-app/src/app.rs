//! The resident application: everything that outlives the window.
//!
//! fastframe-shell drops the window when it closes to the tray and builds a
//! new one (with a fresh egui context) when it's shown again, so all state
//! lives here. Per-window state is reset in `window_gone`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fastframe_shell::{Closed, Headless, Resident, Waker};
use fastframe_tray::{Event, Tray};
use softcaller_core::secrets::{Keyring, MemoryStore, SecretStore};
use softcaller_core::settings::{Settings, WindowGeometry};

use crate::idle::IdleWatch;
use crate::netwatch::NetWatch;
use crate::sip::{Sip, Status};
use crate::theme::{self, Palette};
use crate::tray;
use crate::ui::UiState;

/// Where the app keeps its files.
pub struct Paths {
    pub settings: PathBuf,
    /// Where the generated sound files go.
    pub sounds: PathBuf,
    /// The opt-in full SIP log.
    pub sip_log: PathBuf,
    /// `softcaller.db`: contacts and call history.
    pub database: PathBuf,
}

const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
/// Credential Manager entries are `<account key>.Softcaller`.
const CREDENTIAL_SERVICE: &str = "Softcaller";

/// The notification service, under the same ID as the single-instance lock.
fn notifier() -> Option<fastframe_notifications::Client> {
    let app = fastframe_notifications::App::new("io.github.habitual69.softcaller", "Softcaller").desktop_entry(crate::DESKTOP_ENTRY).logo(&crate::icons::logo_png(256), crate::icons::LOGO_TILE);
    fastframe_notifications::Client::new(app).map_err(|e| log::warn!("notifications unavailable: {e}")).ok()
}

/// Requests from other launches (`fastframe-instance`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Show,
    /// A missed-call notification was clicked.
    ShowCalls,
    /// A message notification was clicked: open that conversation.
    ShowConversation(String),
    /// A launch with arguments or a link (`msip:`, `sip:`, `tel:`…).
    Verb(softcaller_core::command::Verb),
}

pub type Requests = Arc<Mutex<Vec<Request>>>;

/// New SIP credentials being fetched from a JioFiber/AirFiber router.
struct RouterRefresh {
    account: u32,
    job: crate::jiofiber::Job<Result<softcaller_core::jiofiber::Credentials, String>>,
}

pub struct App {
    pub settings: Settings,
    settings_path: PathBuf,
    settings_dirty: bool,
    last_save: Instant,
    files: fastframe_file::Writer,
    pub tray: Option<Tray>,
    /// The status (and missed calls) the tray currently shows.
    tray_status: Option<(Status, bool)>,
    tray_toggles: Option<tray::Toggles>,
    requests: Requests,
    pub sip: Sip,
    pub phonebook: crate::phonebook::Phonebook,
    pub directory: crate::directory::UsersDirectory,
    /// Missed calls since the Calls page was last open.
    pub missed: u32,
    notifier: Option<fastframe_notifications::Client>,
    /// The latest missed-call toast, replaced by the next.
    missed_toast: Option<fastframe_notifications::Handle>,
    /// The latest message toast per conversation.
    message_toasts: std::collections::HashMap<String, fastframe_notifications::Handle>,
    waker: Waker,
    /// New SIP credentials being fetched for a JioFiber/AirFiber account.
    router_refresh: Option<RouterRefresh>,
    /// When the last automatic refresh started (at most one a minute).
    router_refresh_at: Option<Instant>,
    /// Why the last refresh failed, and for which account.
    pub router_refresh_error: Option<(u32, String)>,
    /// The missed-call count on the window's taskbar button.
    taskbar: fastframe_dock::Taskbar,
    secrets: Box<dyn SecretStore>,
    power: fastframe_power::Power,
    net: NetWatch,
    idle: IdleWatch,
    /// Starting with Windows.
    pub start_at_login: crate::autostart::StartAtLogin,
    /// Media keys during calls, while that's on.
    media_keys: Option<crate::media_keys::MediaKeys>,
    /// The USB headset worker, while headsets are on.
    headset: Option<crate::headset::Headset>,
    /// The recent numbers last put in the taskbar jump list.
    jump_list: Option<Vec<(String, String)>>,
    rendering: fastframe_text::TextRendering,

    /// What happens when the window goes away.
    next: Closed,
    quit_requested: bool,
    wants_show: bool,
    /// Bring the window forward at the next chance (incoming call).
    show_requested: bool,

    /// Per-window state, reset in `window_gone`.
    pub ui: UiState,
    /// The bottom-right call card while the main window is in the background.
    pub caller_popup: crate::ui::caller_popup::State,
    pub debug: crate::ui::debug::DebugState,
    applied_palette: Option<Palette>,
    transition: fastframe_theme::Transition,
    recovery_checked: bool,
    /// The native window decorations were matched to the title bar setting.
    titlebar_checked: bool,
}

impl App {
    pub fn new(mut settings: Settings, paths: Paths, requests: Requests, waker: &Waker) -> Self {
        let secrets: Box<dyn SecretStore> = match Keyring::init(CREDENTIAL_SERVICE) {
            Ok(store) => Box::new(store),
            Err(e) => {
                log::warn!("credential store unavailable ({e}); passwords won't be remembered");
                Box::new(MemoryStore::default())
            }
        };
        for account in &mut settings.accounts {
            match secrets.get(&account.secret_key()) {
                Ok(password) => account.password = password.unwrap_or_default(),
                Err(e) => log::warn!("cannot read the password for account {}: {e}", account.id),
            }
        }
        let sounds = match softcaller_core::sounds::install(&paths.sounds) {
            Ok(files) => {
                use softcaller_core::sounds::Sound;
                crate::calls::SoundPaths {
                    ringtone: files.path(Sound::Ringtone),
                    ringback: files.path(Sound::Ringback),
                    call_waiting: files.path(Sound::CallWaiting),
                    hangup: files.path(Sound::Hangup),
                    message_in: files.path(Sound::MessageIn),
                    message_out: files.path(Sound::MessageOut),
                }
            }
            Err(e) => {
                log::warn!("cannot write sound files: {e}");
                crate::calls::SoundPaths::default()
            }
        };
        let sip = Sip::spawn(&settings, paths.sip_log, sounds, waker);
        let (power_wake, net_wake) = (waker.clone(), waker.clone());
        let wake = waker.clone();
        Self {
            settings,
            settings_path: paths.settings,
            settings_dirty: false,
            last_save: Instant::now(),
            files: fastframe_file::Writer::new(),
            tray: tray::spawn(move || wake.wake()),
            tray_status: None,
            tray_toggles: None,
            requests,
            sip,
            phonebook: crate::phonebook::Phonebook::open(&paths.database),
            directory: crate::directory::UsersDirectory::default(),
            missed: 0,
            notifier: notifier(),
            missed_toast: None,
            message_toasts: std::collections::HashMap::new(),
            waker: waker.clone(),
            router_refresh: None,
            router_refresh_at: None,
            router_refresh_error: None,
            #[allow(clippy::default_constructed_unit_structs, reason = "Taskbar is a unit struct only on some platforms")]
            taskbar: fastframe_dock::Taskbar::default(),
            secrets,
            power: fastframe_power::Power::spawn(move || power_wake.wake()),
            net: NetWatch::spawn(move || net_wake.wake()),
            media_keys: None,
            headset: None,
            jump_list: None,
            start_at_login: {
                let wake = waker.clone();
                let start = crate::autostart::StartAtLogin::new(move || wake.wake());
                start.refresh();
                start
            },
            idle: IdleWatch::spawn({
                let wake = waker.clone();
                move || wake.wake()
            }),
            rendering: fastframe_text::detect(),
            next: Closed::Quit,
            quit_requested: false,
            wants_show: false,
            show_requested: false,
            ui: UiState::default(),
            caller_popup: crate::ui::caller_popup::State::default(),
            debug: crate::ui::debug::DebugState::default(),
            applied_palette: None,
            transition: fastframe_theme::Transition::default(),
            recovery_checked: false,
            titlebar_checked: false,
        }
    }

    /// Fonts, image loaders and icons for a new window's context.
    pub fn attach(&mut self, ctx: &egui::Context) {
        // The popup viewport belongs to the context; a new window needs a new one.
        self.caller_popup.reset();
        self.titlebar_checked = false;
        fastframe_titlebar::set_enabled(self.settings.ui.custom_titlebar);
        let mut fonts = fastframe_fonts::FontSetup::default().definitions();
        self.rendering.apply_to(&mut fonts);
        ctx.set_fonts(fonts);
        egui_extras::install_image_loaders(ctx);
        fastframe_icons::install::<crate::icons::Icon>(ctx);
        fastframe_titlebar::install(ctx);
        if let Some(tray) = &mut self.tray {
            tray.attach();
        }
    }

    pub fn palette(&self) -> &Palette {
        self.applied_palette.as_ref().unwrap_or(&const { Palette::dark() })
    }

    pub fn mark_settings_dirty(&mut self) {
        self.settings_dirty = true;
    }

    /// Media keys and headset buttons:
    /// shows the call in the desktop's media controls and acts on the keys.
    /// Returns true when the window should come forward.
    fn media_keys(&mut self) -> bool {
        use crate::media_keys::{CallKey, MediaKeys, Shown};
        if !self.settings.calls.media_keys {
            if let Some(mut controls) = self.media_keys.take() {
                controls.show(None);
            }
            return false;
        }
        let calls = &self.sip.calls;
        let ringing = calls.visible().find(|c| c.is_ringing_in()).cloned();
        let current = calls.current_call().filter(|c| c.is_live()).cloned();
        let shown = ringing.as_ref().or(current.as_ref()).map(|c| Shown {
            id: c.info.sip_call_id.clone(),
            caller: crate::ui::call::caller(self, c),
            status: c.status_text(),
            playing: c.is_ringing_in() || c.info.media == sip_engine::MediaState::Active,
        });
        // Time changes the status every second; show the state, not the clock.
        let shown = shown.map(|mut s| {
            if s.playing && s.status.contains(':') {
                s.status = "In a call".into();
            }
            s
        });
        // Not until there's a call; after that, stopped between calls.
        if shown.is_none() && self.media_keys.is_none() {
            return false;
        }
        let waker = self.waker.clone();
        let controls = self.media_keys.get_or_insert_with(|| MediaKeys::start(move || waker.wake()));
        controls.show(shown);
        let pressed = controls.keys();
        let mut raise = false;
        for key in pressed {
            match (key, &ringing, &current) {
                (CallKey::AnswerOrHold, Some(r), _) => {
                    let (id, video) = (r.id(), r.info.remote_video && self.settings.features.video);
                    self.act(|c| c.answer(id, video));
                }
                (CallKey::AnswerOrHold, None, Some(c)) => {
                    let id = c.id();
                    self.act(|b| b.toggle_hold(id));
                }
                (CallKey::DeclineOrHangUp, Some(r), _) => {
                    let id = r.id();
                    self.act(|c| c.decline(id));
                }
                (CallKey::DeclineOrHangUp, None, Some(c)) => {
                    let id = c.id();
                    self.act(|b| b.hangup(id));
                }
                (CallKey::Raise, ..) => raise = true,
                _ => {}
            }
        }
        raise
    }

    /// The taskbar jump list: recent numbers to
    /// call, and Hang up all calls and Exit tasks. Rebuilt off the UI thread
    /// when the recent numbers change.
    fn update_jump_list(&mut self) {
        const RECENT: usize = 8;
        let mut recent: Vec<(String, String)> = Vec::new();
        for record in &self.phonebook.calls {
            let number = record.number.trim();
            if number.is_empty() || recent.iter().any(|(n, _)| n == number) {
                continue;
            }
            let name = self.phonebook.lookup(number).map(softcaller_core::contacts::Contact::title).unwrap_or_else(|| record.name.clone());
            let title = if name.trim().is_empty() || name == number { number.to_owned() } else { format!("{name} ({number})") };
            recent.push((number.to_owned(), title));
            if recent.len() == RECENT {
                break;
            }
        }
        if self.jump_list.as_ref() == Some(&recent) {
            return;
        }
        self.jump_list = Some(recent.clone());
        let Ok(exe) = std::env::current_exe() else { return };
        let spawned = std::thread::Builder::new().name("jumplist".into()).spawn(move || {
            use softcaller_win::Link;
            let tasks = [
                Link { title: "Hang up all calls".into(), arguments: "/hangupall".into() },
                Link { title: "Exit Softcaller".into(), arguments: "/exit".into() },
            ];
            let recent: Vec<Link> = recent.into_iter().map(|(number, title)| Link { title, arguments: number }).collect();
            if let Err(e) = softcaller_win::set_jump_list(&exe, &tasks, "Recent calls", &recent) {
                log::warn!("cannot set the jump list: {e}");
            }
        });
        if let Err(e) = spawned {
            log::warn!("cannot set the jump list: {e}");
        }
    }

    /// A USB headset's buttons and lights.
    fn headset(&mut self) {
        use crate::headset::{Button, Headset, Lights};
        if !self.settings.calls.headset {
            self.headset = None;
            return;
        }
        let waker = self.waker.clone();
        let headset = self.headset.get_or_insert_with(|| Headset::start(move || waker.wake()));
        let calls = &self.sip.calls;
        let ringing: Vec<_> = calls.visible().filter(|c| c.is_ringing_in()).map(crate::calls::Call::id).collect();
        let talking: Vec<_> = calls.visible().filter(|c| c.is_live() && !c.is_ringing_in()).map(crate::calls::Call::id).collect();
        headset.show(Lights { off_hook: !talking.is_empty(), ring: !ringing.is_empty(), mute: calls.mic_muted });
        for button in headset.buttons() {
            match button {
                // Picked up: answer, or call what's on the dialer.
                Button::Hook(true) => {
                    if let Some(&id) = ringing.first() {
                        self.act(|c| c.answer(id, false));
                    } else if talking.is_empty() && !self.ui.dialer.number.trim().is_empty() {
                        let number = self.ui.dialer.number.trim().to_owned();
                        crate::ui::dial(self, &number, false);
                    }
                }
                // Put down: hang up the calls, or decline a ringing one.
                Button::Hook(false) => {
                    if !talking.is_empty() {
                        self.sip.run(talking.iter().copied().map(sip_engine::Command::Hangup).collect());
                    } else if let Some(&id) = ringing.first() {
                        self.act(|c| c.decline(id));
                    }
                }
                Button::Mute(mute) => {
                    if mute != self.sip.calls.mic_muted {
                        self.act(crate::calls::CallBook::toggle_mic);
                    }
                }
                Button::Redial => {
                    let last = self.phonebook.calls.iter().find(|r| r.kind == softcaller_core::history::CallKind::Outgoing).map(|r| r.number.clone());
                    if let Some(number) = last.filter(|_| talking.is_empty()) {
                        crate::ui::dial(self, &number, false);
                    }
                }
            }
        }
    }

    /// Acts on a command-line verb. Returns true
    /// when the window should come forward.
    fn run_verb(&mut self, verb: softcaller_core::command::Verb, ctx: Option<&egui::Context>) -> bool {
        use softcaller_core::command::Verb;
        let calls = &self.sip.calls;
        let current = calls.current;
        let ringing: Vec<_> = calls.visible().filter(|c| c.is_ringing_in()).map(crate::calls::Call::id).collect();
        match verb {
            Verb::Dial(target) => {
                self.ui.page = crate::ui::Page::Dialer;
                crate::ui::dial(self, &target, false);
                return true;
            }
            Verb::Answer => {
                if let Some(&id) = ringing.first() {
                    self.act(|c| c.answer(id, false));
                }
            }
            Verb::HangupAll => {
                // Hanging up all but incoming: ringing calls stay.
                let ids: Vec<_> = calls.visible().filter(|c| c.is_live() && !c.is_ringing_in()).map(crate::calls::Call::id).collect();
                self.sip.run(ids.into_iter().map(sip_engine::Command::Hangup).collect());
            }
            Verb::HangupIncoming => {
                for id in ringing {
                    self.act(|c| c.decline(id));
                }
            }
            Verb::Hold => {
                if let Some(id) = current {
                    self.act(|c| c.toggle_hold(id));
                }
            }
            Verb::Transfer(target) => {
                if let Some(id) = current
                    && let Ok(target) = self.format_number(&target)
                {
                    let cfg = self.settings.calls.clone();
                    self.act(|c| c.transfer(id, &target, &cfg));
                }
            }
            Verb::MicMute(want) => {
                if want.is_none_or(|w| w != self.sip.calls.mic_muted) {
                    self.act(crate::calls::CallBook::toggle_mic);
                }
            }
            Verb::SpeakerMute(want) => {
                if want.is_none_or(|w| w != self.sip.calls.speaker_muted) {
                    self.act(crate::calls::CallBook::toggle_speaker);
                }
            }
            Verb::MicLevel(step) => {
                let level = &mut self.settings.audio.volume_input;
                *level = level.saturating_add_signed(step).min(100);
                self.mark_settings_dirty();
            }
            Verb::SpeakerLevel(step) => {
                let level = &mut self.settings.audio.volume_output;
                *level = level.saturating_add_signed(step).min(100);
                self.mark_settings_dirty();
            }
            Verb::Minimize => {
                if let Some(ctx) = ctx {
                    if self.can_hide() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    } else {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                }
            }
            Verb::Exit => self.quit(ctx),
        }
        false
    }

    pub fn quit(&mut self, ctx: Option<&egui::Context>) {
        self.quit_requested = true;
        if let Some(ctx) = ctx {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Whether closing the window should keep Softcaller running in the tray.
    fn can_hide(&self) -> bool {
        self.settings.ui.close_to_tray && self.tray.as_ref().is_some_and(Tray::is_shown)
    }

    pub fn status(&self) -> Status {
        self.sip.status(&self.settings)
    }

    /// Counts a missed call and shows a notification for it, replacing the
    /// previous one; clicking it opens the Calls page.
    fn missed_call(&mut self, who: &str) {
        self.missed += 1;
        let Some(client) = &self.notifier else { return };
        let body = if self.missed > 1 { format!("{who} (and {} more)", self.missed - 1) } else { who.to_owned() };
        let toast = fastframe_notifications::Notification::new("Missed call").body(body).key("missed-call");
        let (requests, waker) = (self.requests.clone(), self.waker.clone());
        match client.show_with(toast, move |event| {
            if event == fastframe_notifications::Event::Activated {
                requests.lock().unwrap_or_else(|e| e.into_inner()).push(Request::ShowCalls);
                waker.wake();
            }
        }) {
            Ok(handle) => self.missed_toast = Some(handle),
            Err(e) => log::warn!("cannot show the missed-call notification: {e}"),
        }
    }

    /// Plays a message sound, unless a call is up (it would cut off the
    /// ringtone or ringback).
    fn message_sound(&self, incoming: bool) {
        if self.sip.calls.has_calls() {
            return;
        }
        let sounds = &self.sip.calls.sounds;
        let path = if incoming { sounds.message_in.clone() } else { sounds.message_out.clone() };
        let level = self.sip.calls.ring_level;
        self.sip.send(sip_engine::Command::PlaySound(sip_engine::Sound { path, looping: false, level, ring: true }));
    }

    /// A message for `peer`: the sound (for every message)
    /// and a notification per conversation, unless it's on screen.
    fn message_arrived(&mut self, peer: &str, who: &str, text: &str) {
        let on_screen = self.ui.page == crate::ui::Page::Messages
            && self.ui.messages.seen.as_ref().is_some_and(|(seen, at)| seen == peer && at.elapsed() < Duration::from_millis(1500));
        if on_screen {
            return;
        }
        self.message_sound(true);
        let Some(client) = &self.notifier else { return };
        let preview: String = text.chars().take(200).collect();
        let toast = fastframe_notifications::Notification::new(format!("Message from {who}")).body(preview).key(format!("message-{peer}"));
        let (requests, waker, peer_owned) = (self.requests.clone(), self.waker.clone(), peer.to_owned());
        match client.show_with(toast, move |event| {
            if event == fastframe_notifications::Event::Activated {
                requests.lock().unwrap_or_else(|e| e.into_inner()).push(Request::ShowConversation(peer_owned.clone()));
                waker.wake();
            }
        }) {
            Ok(handle) => {
                self.message_toasts.insert(peer.to_owned(), handle);
            }
            Err(e) => log::warn!("cannot show the message notification: {e}"),
        }
    }

    /// A conversation is on screen: its notification goes.
    pub fn conversation_seen(&mut self, peer: &str) {
        if self.message_toasts.remove(peer).is_some()
            && let Some(client) = &self.notifier
        {
            let _ = client.withdraw(format!("message-{peer}"));
        }
    }

    /// The Calls page is open: missed calls have been seen.
    pub fn missed_seen(&mut self) {
        if self.missed > 0 {
            self.missed = 0;
            if let Some(client) = &self.notifier {
                let _ = client.withdraw("missed-call");
            }
            self.missed_toast = None;
        }
    }

    /// The active account's registrar, domain and proxies: numbers there are
    /// shown without the host.
    fn own_hosts(&self) -> Vec<String> {
        self.settings
            .active()
            .map(|a| [a.server.trim().to_owned(), a.domain.trim().to_owned()].into_iter().chain(a.proxies()).filter(|h| !h.is_empty()).collect())
            .unwrap_or_default()
    }

    /// Whether to reach `target` through the local account: IP addresses
    /// when it's on, or anything when there's no usable main account.
    pub fn use_local_account(&self, target: &str) -> bool {
        let looks_like_ip = target.split([':', '@']).any(|part| part.parse::<std::net::IpAddr>().is_ok());
        self.settings.local_account.enabled && (looks_like_ip || !self.status().is_online())
    }

    /// Sends a text message to `target`, saving it first; its answer
    /// updates it. Returns the conversation it went into.
    pub fn send_message(&mut self, target: &str, text: &str) -> String {
        let peer = crate::phonebook::conversation_key(target);
        let record = softcaller_core::messages::MessageRecord {
            id: 0,
            peer: peer.clone(),
            name: String::new(),
            outgoing: true,
            time: jiff::Timestamp::now().as_second(),
            text: text.to_owned(),
            state: softcaller_core::messages::MessageState::Sending,
            reason: String::new(),
            unread: false,
        };
        let id = self.phonebook.add_message(&record);
        self.dispatch_message(id, &peer, text);
        // Played with sound events on.
        if self.settings.calls.local_dtmf {
            self.message_sound(false);
        }
        peer
    }

    /// Sends a failed message again, in place.
    pub fn retry_message(&mut self, id: i64, peer: &str, text: &str) {
        self.phonebook.set_message_state(id, softcaller_core::messages::MessageState::Sending, "");
        self.dispatch_message(id, peer, text);
    }

    fn dispatch_message(&mut self, id: i64, peer: &str, text: &str) {
        let local = self.use_local_account(peer);
        self.sip.send(sip_engine::Command::SendMessage { id: id as u64, target: peer.to_owned(), text: text.to_owned(), local });
    }

    /// The users-directory URL's placeholder values, from the active account.
    fn directory_values(&self) -> softcaller_core::directory::UrlValues {
        let mut v = softcaller_core::directory::UrlValues {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            time: jiff::Timestamp::now().as_second(),
            ..Default::default()
        };
        if let Some(a) = self.settings.active() {
            v.username = a.username.trim().to_owned();
            v.password = a.password.clone();
            v.server = a.server.trim().to_owned();
            v.domain = a.domain.trim().to_owned();
            v.label = a.title().to_owned();
        }
        v
    }

    /// Whether a JioFiber/AirFiber account's credentials need renewing: the
    /// router rejects them (the active account's registration fails with
    /// 401/403/407) or they have expired.
    pub fn router_credentials_stale(&self, id: u32) -> bool {
        let Some(provisioning) = self.settings.account(id).and_then(|a| a.provisioning.as_ref()) else { return false };
        let expired = provisioning.expires_at.is_some_and(|t| t <= jiff::Timestamp::now().as_second());
        expired || (self.settings.active_account == Some(id) && self.credentials_rejected())
    }

    /// The registrar turned down the active account's user or password.
    fn credentials_rejected(&self) -> bool {
        matches!(self.sip.reg, sip_engine::RegState::Failed { code: 401 | 403 | 407, .. })
    }

    pub fn router_refresh_running(&self, id: u32) -> bool {
        self.router_refresh.as_ref().is_some_and(|r| r.account == id)
    }

    /// Asks the account's router for new credentials of the same kind; they
    /// are saved into the account when they arrive.
    pub fn refresh_router_credentials(&mut self, id: u32) {
        if self.router_refresh.is_some() {
            return;
        }
        let Some(provisioning) = self.settings.account(id).and_then(|a| a.provisioning.clone()) else { return };
        let (host, mode, hostname) = (provisioning.router, provisioning.mode, crate::jiofiber::hostname());
        log::info!("refreshing the {} SIP credentials of account {id} from {host}", mode.label());
        self.router_refresh_error = None;
        let job = crate::jiofiber::Job::start(&self.waker, move || crate::jiofiber::renew_credentials(&host, &hostname, mode));
        self.router_refresh = Some(RouterRefresh { account: id, job });
    }

    /// Takes in refreshed credentials, and starts the automatic refresh of
    /// the active account's temporary credentials when they stop working or
    /// are about to expire.
    fn tick_router_refresh(&mut self) {
        if let Some(result) = self.router_refresh.as_ref().and_then(|r| r.job.poll()) {
            let id = self.router_refresh.take().map_or(0, |r| r.account);
            match result {
                Ok(credentials) => self.renewed_router_credentials(id, &credentials),
                Err(e) => {
                    log::warn!("cannot refresh the SIP credentials of account {id}: {e}");
                    self.notify(format!("Couldn't refresh the SIP credentials: {e}"));
                    self.router_refresh_error = Some((id, e));
                }
            }
        }
        let Some(account) = self.settings.active() else { return };
        let Some(provisioning) = &account.provisioning else { return };
        if provisioning.mode != softcaller_core::jiofiber::Mode::Temporary || !provisioning.auto_refresh || self.router_refresh.is_some() {
            return;
        }
        let expiring = provisioning.expires_at.is_some_and(|t| t - 60 <= jiff::Timestamp::now().as_second());
        if !(expiring || self.credentials_rejected()) || self.router_refresh_at.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) {
            return;
        }
        self.router_refresh_at = Some(Instant::now());
        self.refresh_router_credentials(account.id);
    }

    /// Saves new router credentials into account `id` (and into its open
    /// editor); registration then retries with them.
    fn renewed_router_credentials(&mut self, id: u32, credentials: &softcaller_core::jiofiber::Credentials) {
        let Some(mut account) = self.settings.account(id).cloned() else { return };
        let Some(router) = account.provisioning.as_ref().map(|p| p.router.clone()) else { return };
        softcaller_core::jiofiber::apply_credentials(&mut account, credentials, &router);
        let expires_at = credentials.validity.map(|s| jiff::Timestamp::now().as_second() + i64::try_from(s).unwrap_or(i64::MAX / 2));
        if let Some(provisioning) = &mut account.provisioning {
            provisioning.expires_at = expires_at;
        }
        if let Some(draft) = &mut self.ui.account_draft
            && draft.account.id == id
        {
            let d = &mut draft.account;
            (d.server, d.proxy, d.domain, d.username, d.auth_id, d.password, d.transport) =
                (account.server.clone(), account.proxy.clone(), account.domain.clone(), account.username.clone(), account.auth_id.clone(), account.password.clone(), account.transport);
            if let Some(p) = &mut d.provisioning {
                p.expires_at = expires_at;
            }
        }
        self.save_account(account);
        log::info!("saved new SIP credentials for account {id}");
        self.notify("New SIP credentials from the router");
    }

    /// Wakes the shell, for work finishing on other threads.
    pub fn waker(&self) -> &Waker {
        &self.waker
    }

    /// Shows `text` briefly at the bottom of the window.
    pub fn notify(&mut self, text: impl Into<String>) {
        self.sip.calls.notice = Some((text.into(), Instant::now()));
    }

    /// Runs a call action and sends the commands it produces.
    /// `number` with the main account's dialling prefix and dial plan;
    /// the notice to show when the dial plan blocks it.
    pub fn format_number(&self, number: &str) -> Result<String, String> {
        let Some(account) = self.settings.active() else { return Ok(number.to_owned()) };
        softcaller_core::dialplan::format_number(number, &account.dialing_prefix, &account.dial_plan).map_err(|_| format!("The dial plan doesn't allow {number}"))
    }

    pub fn act(&mut self, action: impl FnOnce(&mut crate::calls::CallBook) -> Vec<sip_engine::Command>) {
        let commands = action(&mut self.sip.calls);
        self.sip.run(commands);
    }

    /// Saves an account (new when its id is 0) and its password. Returns its id.
    pub fn save_account(&mut self, mut account: softcaller_core::account::Account) -> u32 {
        let id = if account.id == 0 {
            self.settings.add_account(account.clone())
        } else {
            self.settings.update_account(account.clone());
            account.id
        };
        account.id = id;
        if let Err(e) = self.secrets.set(&account.secret_key(), &account.password) {
            log::warn!("cannot store the password for account {id}: {e}");
        }
        // The stored copy keeps the password in memory for the engine.
        if let Some(slot) = self.settings.accounts.iter_mut().find(|a| a.id == id) {
            slot.password = account.password;
        }
        self.mark_settings_dirty();
        id
    }

    pub fn delete_account(&mut self, id: u32) {
        if let Some(account) = self.settings.remove_account(id) {
            if let Err(e) = self.secrets.delete(&account.secret_key()) {
                log::warn!("cannot remove the password for account {id}: {e}");
            }
            self.mark_settings_dirty();
        }
    }

    /// Pushes settings changes to the engine and engine events to the tray.
    fn update_sip(&mut self) {
        for event in self.power.events() {
            match event {
                // pjsua is destroyed before sleep and recreated after.
                fastframe_power::Event::Suspending(ack) => {
                    log::info!("system suspending; stopping the SIP engine");
                    self.sip.send(sip_engine::Command::Suspend);
                    self.save_settings(true);
                    self.files.flush(Duration::from_millis(1500));
                    ack.done();
                }
                fastframe_power::Event::EndingSession(ack) => {
                    self.save_settings(true);
                    self.files.flush(Duration::from_millis(1500));
                    ack.done();
                }
                fastframe_power::Event::Resumed => {
                    log::info!("system resumed; starting the SIP engine");
                    self.sip.send(sip_engine::Command::Resume);
                }
            }
        }
        if self.net.take_changed() && self.settings.sip.network_changes {
            self.sip.send(sip_engine::Command::IpChanged);
        }
        // Busy for DND, Away after 10 idle minutes.
        self.sip.online_status = if self.settings.calls.dnd {
            sip_engine::OnlineStatus::Busy
        } else if self.idle.is_away() {
            sip_engine::OnlineStatus::Away
        } else {
            sip_engine::OnlineStatus::Online
        };
        self.sip.sync(&self.settings);
        // Without the presence feature nothing is subscribed; contacts keep their choice.
        let buddies = if self.settings.features.presence {
            // Contacts and speed-dial buttons with presence on (BLF).
            let mut numbers = self.phonebook.watched_numbers();
            numbers.extend(self.settings.shortcuts.iter().filter(|s| s.presence).map(|s| s.number.trim().to_owned()).filter(|n| !n.is_empty()));
            numbers.sort();
            numbers.dedup();
            numbers
        } else {
            Vec::new()
        };
        self.sip.set_buddies(buddies);
        let values = self.directory_values();
        let ready = matches!(self.sip.reg, sip_engine::RegState::Registered { .. } | sip_engine::RegState::NoRegistrar);
        let template = self.settings.sip.users_directory.clone();
        if let Some(message) = self.directory.tick(&template, &values, ready, &mut self.phonebook, &self.waker) {
            self.notify(message);
        }
        let polled = self.sip.poll(&self.settings);
        self.tick_router_refresh();
        if !polled.ended.is_empty() {
            let own_hosts = self.own_hosts();
            for call in &polled.ended {
                self.phonebook.record(call, &own_hosts);
                if call.is_missed() {
                    let record = crate::phonebook::record_for(call, &own_hosts);
                    let who = match self.phonebook.lookup(&record.number) {
                        Some(contact) => contact.title(),
                        None if !record.name.is_empty() => record.name.clone(),
                        None => record.number.clone(),
                    };
                    self.missed_call(&who);
                }
            }
        }
        let own_hosts = self.own_hosts();
        for message in polled.messages {
            if !self.settings.features.messages {
                continue;
            }
            let (number, name) = crate::phonebook::peer_for(&message.from, &own_hosts);
            let peer = crate::phonebook::conversation_key(&number);
            let who = match self.phonebook.lookup(&peer) {
                Some(contact) => contact.title(),
                None if !name.is_empty() => name.clone(),
                None => number.clone(),
            };
            self.message_arrived(&peer, &who, &message.text);
            let record = softcaller_core::messages::MessageRecord {
                id: 0,
                peer,
                name,
                outgoing: false,
                time: jiff::Timestamp::now().as_second(),
                text: message.text,
                state: softcaller_core::messages::MessageState::Received,
                reason: String::new(),
                unread: true,
            };
            self.phonebook.add_message(&record);
        }
        for status in polled.message_statuses {
            use softcaller_core::messages::MessageState;
            let (state, reason) = match (status.delivered(), status.code) {
                (true, _) => (MessageState::Sent, String::new()),
                (false, 0) => (MessageState::Failed, status.reason),
                (false, code) => (MessageState::Failed, format!("{code} {}", status.reason).trim().to_owned()),
            };
            self.phonebook.set_message_state(status.id as i64, state, &reason);
        }
        if polled.show_window {
            self.ui.page = crate::ui::Page::Dialer;
            self.ui.adding_call = false;
            self.show_requested = true;
        }
        let status = self.status();
        let shown = (status, self.missed > 0);
        if self.tray_status.as_ref() != Some(&shown) {
            if let Some(tray) = &mut self.tray {
                tray::show_status(tray, &shown.0, self.settings.active().map(|a| a.title()), shown.1);
            }
            self.tray_status = Some(shown);
        }
        let toggles = tray::Toggles::of(&self.settings.calls);
        if self.tray_toggles != Some(toggles) {
            if let Some(tray) = &mut self.tray {
                tray::show_toggles(tray, toggles);
            }
            self.tray_toggles = Some(toggles);
        }
    }

    /// Handles tray clicks and requests from other launches. `ctx` is the
    /// window's context, or `None` while running in the tray.
    fn poll(&mut self, ctx: Option<&egui::Context>) {
        self.update_sip();
        let mut show = std::mem::take(&mut self.show_requested);
        // A tray click or another launch asks for the window itself; a call
        // can be answered in the bottom-right card instead.
        let mut force_show = false;
        let events = self.tray.as_ref().map(Tray::events).unwrap_or_default();
        for event in events {
            match event {
                Event::Show | Event::Toggle | Event::Menu(tray::SHOW) => {
                    show = true;
                    force_show = true;
                    // Missed calls open the Calls tab.
                    if self.missed > 0 {
                        self.ui.page = crate::ui::Page::Calls;
                    }
                }
                Event::Menu(tray::QUIT) => self.quit(ctx),
                Event::Menu(tray::DND) => {
                    self.settings.calls.dnd = !self.settings.calls.dnd;
                    self.mark_settings_dirty();
                }
                Event::Menu(tray::FORWARD) => {
                    self.settings.calls.forwarding = !self.settings.calls.forwarding;
                    self.mark_settings_dirty();
                }
                Event::Menu(tray::AUTO_ANSWER) => {
                    self.settings.calls.auto_answer = !self.settings.calls.auto_answer;
                    self.mark_settings_dirty();
                }
                Event::Menu(tray::DEBUG) => {
                    show = true;
                    force_show = true;
                    crate::ui::debug::set_open(self, true);
                }
                Event::Menu(_) => {}
            }
        }
        show |= self.media_keys();
        self.headset();
        self.update_jump_list();
        let requests = std::mem::take(&mut *self.requests.lock().unwrap_or_else(|e| e.into_inner()));
        for request in requests {
            match request {
                Request::Show => {
                    show = true;
                    force_show = true;
                }
                Request::ShowConversation(peer) => {
                    show = true;
                    force_show = true;
                    self.ui.page = crate::ui::Page::Messages;
                    self.ui.messages.open = Some(peer);
                    self.ui.adding_call = false;
                }
                Request::Verb(verb) => show |= self.run_verb(verb, ctx),
                Request::ShowCalls => {
                    show = true;
                    force_show = true;
                    self.ui.page = crate::ui::Page::Calls;
                    self.ui.adding_call = false;
                }
            }
        }
        if show {
            let popup_takes_over = !force_show
                && ctx.is_some_and(crate::ui::caller_popup::background)
                && self.settings.calls.caller_popup
                && self.caller_popup.registered;
            if !popup_takes_over {
                match ctx {
                    Some(ctx) => bring_forward(ctx),
                    None => self.wants_show = true,
                }
            }
        }
    }

    fn save_settings(&mut self, force: bool) {
        if self.settings_dirty && (force || self.last_save.elapsed() >= SAVE_DEBOUNCE) {
            self.files.write(&self.settings_path, self.settings.to_toml(), fastframe_file::Access::Default);
            self.settings_dirty = false;
            self.last_save = Instant::now();
        }
    }

    /// Runs every frame of the window, including when eframe skips `ui`
    /// (minimised or occluded windows).
    pub fn logic(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        self.poll(Some(ctx));
        crate::ui::caller_popup::update(ctx, self);
        if !std::mem::replace(&mut self.titlebar_checked, true) {
            // The builder already asked for the right decorations; this also
            // repairs a window opened before the setting was read.
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(!self.settings.ui.custom_titlebar));
        }
        let label = match self.missed {
            0 => String::new(),
            1 => "1 missed call".to_owned(),
            n => format!("{n} missed calls"),
        };
        if let Some(retry) = self.taskbar.show(frame, self.missed, &label) {
            ctx.request_repaint_after(retry.saturating_duration_since(Instant::now()));
        }
        if let Some(due) = self.directory.next_load() {
            ctx.request_repaint_after(due.saturating_duration_since(Instant::now()));
        }

        // Decide here, not in `ui`: closing a minimised window from the
        // taskbar must hide it too.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_requested && self.next != Closed::Reopen {
            if self.can_hide() {
                // Keep the eframe window (and the caller viewport) alive but
                // hidden, so calls can still show the bottom-right card while
                // the app is in the tray.
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            } else {
                self.next = Closed::Quit;
            }
        }

        if !std::mem::replace(&mut self.recovery_checked, true) {
            fastframe_shell::window::recover_offscreen(ctx, frame);
        }

        self.update_theme(ctx);

        let mut geometry = to_shell_geometry(self.settings.ui.window);
        if geometry.remember(ctx) {
            self.settings.ui.window = from_shell_geometry(geometry);
            self.mark_settings_dirty();
        }
        if let Some(zoom) = fastframe_shell::window::keep_zoom(ctx, self.settings.ui.zoom) {
            self.settings.ui.zoom = zoom;
            self.mark_settings_dirty();
        }

        self.save_settings(false);
        if self.settings_dirty {
            ctx.request_repaint_after(SAVE_DEBOUNCE);
        }
        // Ctrl+Shift+D toggles the debug window.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::D)) {
            let open = !self.debug.open;
            crate::ui::debug::set_open(self, open);
        }
        // Drawn from `logic` so it keeps updating while the main window is minimised.
        crate::ui::debug::show(ctx, self);

        // Call timers and ended-call cleanup.
        if self.sip.calls.has_calls() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn update_theme(&mut self, ctx: &egui::Context) {
        let base = theme::wanted_base(self.settings.ui.theme, ctx.system_theme());
        let wanted = Palette::for_base(base);
        if self.applied_palette.as_ref() == Some(&wanted) {
            return;
        }
        // The first frame has nothing to reveal from.
        if self.applied_palette.is_some() {
            self.transition.begin(ctx);
            if self.transition.holding(ctx) {
                return;
            }
        }
        theme::apply(ctx, &wanted, &self.rendering);
        self.applied_palette = Some(wanted);
    }

    /// Draws the old colours over the new ones during a theme change. Last in the frame.
    pub fn paint_transition(&mut self, ctx: &egui::Context) {
        self.transition.paint(ctx);
    }
}

fn bring_forward(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

fn to_shell_geometry(g: WindowGeometry) -> fastframe_shell::window::Geometry {
    fastframe_shell::window::Geometry { size: g.size, position: g.position, maximized: g.maximized }
}

fn from_shell_geometry(g: fastframe_shell::window::Geometry) -> WindowGeometry {
    WindowGeometry { size: g.size, position: g.position, maximized: g.maximized }
}

impl App {
    pub fn window_geometry(&self) -> fastframe_shell::window::Geometry {
        to_shell_geometry(self.settings.ui.window)
    }
}

impl Resident for App {
    fn closed(&self) -> Closed {
        if self.quit_requested { Closed::Quit } else { self.next }
    }

    #[allow(clippy::default_constructed_unit_structs, reason = "Taskbar is a unit struct only on some platforms")]
    fn window_gone(&mut self) {
        // Video textures belonged to the old window.
        self.ui.video = crate::ui::video::VideoView::default();
        self.taskbar = fastframe_dock::Taskbar::default();
        // The debug window is a child of the main window.
        crate::ui::debug::set_open(self, false);
        self.caller_popup = crate::ui::caller_popup::State::default();
        self.next = Closed::Quit;
        self.wants_show = false;
        self.applied_palette = None;
        self.transition = fastframe_theme::Transition::default();
        self.recovery_checked = false;
        self.save_settings(true);
    }

    fn headless_frame(&mut self, _ctx: &egui::Context) -> Headless {
        self.poll(None);
        self.save_settings(true);
        if self.quit_requested {
            Headless::Quit
        } else if std::mem::take(&mut self.wants_show) {
            Headless::Show
        } else if self.sip.calls.has_calls() {
            // Keep ended calls expiring while hidden.
            Headless::WaitFor(Duration::from_millis(500))
        } else {
            Headless::Wait
        }
    }

    fn start_hidden(&mut self) -> bool {
        // Without a tray there would be no way back to the window.
        match &mut self.tray {
            Some(tray) => {
                tray.create_item();
                true
            }
            None => false,
        }
    }

    fn shutdown(&mut self) {
        self.save_settings(true);
        self.sip.shutdown();
        self.files.flush(fastframe_file::FLUSH_TIMEOUT);
        log::info!("Softcaller exiting");
    }
}
