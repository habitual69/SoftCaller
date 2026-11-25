//! A small always-on-top call card for the bottom-right corner of the
//! desktop. It appears while Softcaller is minimised, hidden to the tray or
//! behind another window, rings like the main call screen, and expands into
//! the in-call controls once the call is answered.
//!
//! The card is an egui viewport of its own. The main window publishes a
//! snapshot into `Shared` whenever its logic runs (which it also does while
//! minimised), and the viewport draws from that snapshot. Buttons only queue
//! `Action`s; the app applies them on the UI thread, so the popup never
//! mutates the call state behind the rest of the app's back.

use std::sync::{Arc, Mutex};

use egui::{Align, Layout, Sense, Ui, Vec2, vec2};
use fastframe_fonts::Weight;
use sip_engine::{CallId, CallState, Direction, MediaState};

use crate::app::App;
use crate::calls::Call;
use crate::icons::Icon;
use crate::theme::Palette;

use super::widgets;

/// Room around the card for its shadow, inside the window's margin.
const SHADOW: f32 = 12.0;
/// Distance from the screen's right edge (before the shadow margin).
const EDGE: f32 = 10.0;
/// Room left under the window for the taskbar and a small gap.
const BOTTOM: f32 = 54.0;
const RINGING_SIZE: Vec2 = vec2(366.0, 158.0);
const ACTIVE_SIZE: Vec2 = vec2(382.0, 192.0);
const POPUP_TITLE: &str = "Softcaller call";

/// The card can close itself only through the app, which owns the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Answer { id: CallId, video: bool },
    Decline(CallId),
    Hangup(CallId),
    ToggleMic,
    ToggleSpeaker,
    ToggleHold(CallId),
    ToggleRecording(CallId),
    Open,
}

/// Everything the popup needs to draw a frame, copied from the app while it
/// runs its logic. No call state or engine handle crosses the thread/UI
/// boundary.
#[derive(Debug, Clone)]
pub struct Model {
    pub open: bool,
    pub id: CallId,
    /// An incoming call still ringing.
    pub ringing: bool,
    /// An outgoing call still ringing.
    pub outgoing: bool,
    /// The caller offered video and video calls are enabled.
    pub video_offer: bool,
    pub title: String,
    pub number: String,
    pub status: String,
    pub mic_muted: bool,
    pub speaker_muted: bool,
    pub held: bool,
    pub recording: bool,
    pub can_record: bool,
    pub can_hold: bool,
    pub palette: Palette,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            open: false,
            id: 0,
            ringing: false,
            outgoing: false,
            video_offer: false,
            title: String::new(),
            number: String::new(),
            status: String::new(),
            mic_muted: false,
            speaker_muted: false,
            held: false,
            recording: false,
            can_record: false,
            can_hold: false,
            palette: Palette::dark(),
        }
    }
}

impl Model {
    fn size(&self) -> Vec2 {
        if self.ringing || self.outgoing { RINGING_SIZE } else { ACTIVE_SIZE }
    }
}

/// The snapshot and the button queue, shared with the viewport callback.
#[derive(Default)]
pub struct Shared {
    model: Mutex<Model>,
    actions: Mutex<Vec<Action>>,
}

impl Shared {
    fn snapshot(&self) -> Model {
        self.model.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn publish(&self, model: Model) {
        *self.model.lock().unwrap_or_else(|e| e.into_inner()) = model;
    }

    fn push(&self, action: Action) {
        self.actions.lock().unwrap_or_else(|e| e.into_inner()).push(action);
    }

    fn take_actions(&self) -> Vec<Action> {
        std::mem::take(&mut *self.actions.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// Per-window bookkeeping for the popup viewport.
#[derive(Clone, Default)]
pub struct State {
    shared: Arc<Shared>,
    /// The popup should be on screen for the current call/background state.
    want_open: bool,
    /// The viewport has been registered with this egui context.
    pub registered: bool,
    /// The native window has been given rounded corners for this opening.
    rounded: bool,
    /// The inner size last sent to the viewport.
    size: Vec2,
}

impl State {
    /// A new egui context means a new viewport: forget the old registration
    /// but keep the shared snapshot, so a call that is already ringing is
    /// visible on the first frame.
    pub fn reset(&mut self) {
        self.want_open = false;
        self.registered = false;
        self.rounded = false;
        self.size = Vec2::ZERO;
    }
}

fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("softcaller-caller-popup")
}

/// Reads the window's frame to decide whether the card should stand in for
/// the main window. Called from both `App::logic` and the UI.
pub(crate) fn background(ctx: &egui::Context) -> bool {
    ctx.input(|i| {
        let viewport = i.viewport();
        // Focus is `None` until the window receives a focus event; a window
        // that has never been focused is in the background, not in front.
        viewport.minimized.unwrap_or(false) || viewport.occluded.unwrap_or(false) || !viewport.focused.unwrap_or(false)
    })
}

/// Finds the call the card should show: a ringing call first, then the
/// current one.
fn target(app: &App) -> Option<Call> {
    app.sip
        .calls
        .visible()
        .find(|c| c.is_ringing_in())
        .or_else(|| app.sip.calls.current_call().filter(|c| c.is_live()))
        .cloned()
}

fn snapshot(app: &App, call: &Call) -> Model {
    let party = call.party();
    let title = super::call::caller(app, call);
    let number = if party.user.trim().is_empty() { call.info.remote.clone() } else { party.user.clone() };
    let confirmed = call.info.state == CallState::Confirmed;
    Model {
        open: true,
        id: call.id(),
        ringing: call.is_ringing_in(),
        outgoing: call.info.direction == Direction::Outgoing && matches!(call.info.state, CallState::Calling | CallState::Early),
        video_offer: call.info.remote_video && app.settings.features.video,
        title,
        number,
        status: call.status_text(),
        mic_muted: app.sip.calls.mic_muted,
        speaker_muted: app.sip.calls.speaker_muted,
        held: confirmed && matches!(call.info.media, MediaState::LocalHold | MediaState::None),
        recording: call.info.recording,
        can_record: app.settings.recording.button,
        can_hold: confirmed,
        palette: app.palette().clone(),
    }
}

/// Applies a queued button press. Runs on the main thread, in `logic`, so
/// the popup stays a thin view over the same call policy as the main window.
fn apply(ctx: &egui::Context, app: &mut App, action: Action) {
    match action {
        Action::Answer { id, video } => app.act(|c| c.answer(id, video)),
        Action::Decline(id) => app.act(|c| c.decline(id)),
        Action::Hangup(id) => app.act(|c| c.hangup(id)),
        Action::ToggleMic => app.act(|c| c.toggle_mic()),
        Action::ToggleSpeaker => app.act(|c| c.toggle_speaker()),
        Action::ToggleHold(id) => app.act(|c| c.toggle_hold(id)),
        Action::ToggleRecording(id) => app.act(|c| c.toggle_recording(id)),
        Action::Open => {
            app.ui.page = super::Page::Dialer;
            app.ui.adding_call = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }
}

/// Publishes a fresh snapshot and applies any button presses. Called from
/// `App::logic`, which keeps running while the main window is minimised.
pub fn update(ctx: &egui::Context, app: &mut App) {
    for action in app.caller_popup.shared.take_actions() {
        apply(ctx, app, action);
    }

    let model = if app.settings.calls.caller_popup && background(ctx) {
        target(app).map_or_else(Model::default, |call| snapshot(app, &call))
    } else {
        Model::default()
    };
    let open = model.open;
    app.caller_popup.shared.publish(model);
    app.caller_popup.want_open = open;

    // The viewport may not have existed when the last command was sent (the
    // first call can arrive on the same frame the window is registered), so
    // retry until the window's own state agrees.
    let visible = ctx.input_for(viewport_id(), |i| i.viewport().visible().unwrap_or(false));
    if app.caller_popup.registered && visible != open {
        ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::Visible(open));
    }
    if !visible {
        // A closed/recreated viewport needs the DWM preference again.
        app.caller_popup.rounded = false;
    }
    if open {
        if !app.caller_popup.rounded {
            // The native window can appear a frame after it is registered.
            app.caller_popup.rounded = softcaller_win::round_window(POPUP_TITLE).is_ok();
        }
        // Keep the duration and the ringing pulse moving.
        ctx.request_repaint_after_for(std::time::Duration::from_millis(500), viewport_id());
    }
}

/// Registers the viewport and matches its visibility and size to the
/// snapshot. Called from the root UI every frame; once registered, `update`
/// can open it even while the root window is minimised.
pub fn ensure(ctx: &egui::Context, app: &mut App) {
    let shared = app.caller_popup.shared.clone();
    let model = shared.snapshot();
    let size = if model.open { model.size() } else { RINGING_SIZE };
    let pos = position(ctx, size);

    let builder = egui::ViewportBuilder::default()
        .with_title(POPUP_TITLE)
        .with_app_id(crate::DESKTOP_ENTRY)
        .with_inner_size(size)
        .with_min_inner_size(RINGING_SIZE)
        .with_resizable(false)
        .with_decorations(false)
        .with_has_shadow(true)
        .with_always_on_top()
        .with_taskbar(false)
        .with_active(false)
        .with_visible(app.caller_popup.want_open)
        .with_position(pos);

    ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
        let model = shared.snapshot();
        if model.open {
            draw(ui, &model, &shared);
        }
    });
    app.caller_popup.registered = true;

    let visible = ctx.input_for(viewport_id(), |i| i.viewport().visible().unwrap_or(false));
    if app.caller_popup.want_open != visible {
        ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::Visible(app.caller_popup.want_open));
    }
    if app.caller_popup.want_open && app.caller_popup.size != size {
        ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::InnerSize(size));
        ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::OuterPosition(pos));
        app.caller_popup.size = size;
    }
}

/// Bottom-right of the monitor, inside the taskbar.
fn position(ctx: &egui::Context, size: Vec2) -> egui::Pos2 {
    let monitor = ctx.input(|i| i.viewport().monitor_size).unwrap_or_else(|| vec2(1920.0, 1080.0));
    egui::pos2(monitor.x - size.x - EDGE, monitor.y - size.y - BOTTOM)
}

/// The visible card. The window's fill matches the desktop theme, with the
/// card's rounded panel and shadow drawn inside it.
fn draw(ui: &mut Ui, model: &Model, shared: &Arc<Shared>) {
    let p = &model.palette;
    crate::theme::apply_to_ui(ui, p);
    let size = model.size();
    let inner = ui.max_rect().shrink(SHADOW);
    // Keep the right edge fixed when the card grows after answering.
    let target = position(ui.ctx(), size);
    if ui.ctx().input(|i| i.viewport().inner_rect.map(|r| (r.size() - size).length() > 1.0).unwrap_or(true)) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::OuterPosition(target));
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(p.window))
        .show(ui, |ui| {
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)), |ui| {
                egui::Frame::new()
                    .fill(p.panel)
                    .stroke(egui::Stroke::new(1.0, p.outline))
                    .corner_radius(egui::CornerRadius::same(18))
                    .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: p.shadow })
                    .inner_margin(egui::Margin::symmetric(16, 14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        // Explicit gaps below; the default 8 px vertical item
                        // spacing would push the buttons out of the card.
                        ui.spacing_mut().item_spacing.y = 0.0;
                        header(ui, model, shared);
                        ui.add_space(12.0);
                        if model.ringing {
                            incoming_buttons(ui, model, shared);
                        } else if model.outgoing {
                            outgoing_buttons(ui, p, model, shared);
                        } else {
                            active_controls(ui, model, shared);
                        }
                    });
            });
        });
}

fn header(ui: &mut Ui, model: &Model, shared: &Arc<Shared>) {
    let p = &model.palette;
    let (eyebrow, tint) = if model.ringing {
        ("INCOMING CALL", p.accent)
    } else if model.outgoing {
        ("CALLING", p.warning)
    } else if model.held {
        ("ON HOLD", p.warning)
    } else {
        ("IN CALL", p.secondary)
    };

    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::hover());
        widgets::avatar_at(ui, p, rect, &model.title);
        ui.add_space(2.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(egui::RichText::new(eyebrow).font(Weight::SemiBold.font_id(10.0)).color(tint));
            ui.add(
                egui::Label::new(egui::RichText::new(if model.title.is_empty() { &model.number } else { &model.title }).font(Weight::SemiBold.font_id(16.0)).color(p.text))
                    .truncate(),
            );
            let sub = if model.title == model.number || model.number.is_empty() {
                model.status.clone()
            } else {
                format!("{} · {}", model.number, model.status)
            };
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(sub).size(12.0).color(p.dim)).truncate());
                if model.recording && !model.ringing {
                    ui.label(egui::RichText::new("● REC").font(Weight::SemiBold.font_id(10.0)).color(p.danger));
                }
            });
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button(ui, p, Icon::ExternalLink, p.secondary, "Open Softcaller").clicked() {
                shared.push(Action::Open);
            }
        });
    });
}

/// Answer and decline, with a video answer when the caller offers it.
fn incoming_buttons(ui: &mut Ui, model: &Model, shared: &Arc<Shared>) {
    let p = &model.palette;
    let count = if model.video_offer { 3.0 } else { 2.0 };
    let gap = 8.0;
    let width = (ui.available_width() - gap * (count - 1.0)) / count;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        if pill(ui, Icon::PhoneOff, "Decline", p.danger, p.on_accent, width).clicked() {
            shared.push(Action::Decline(model.id));
        }
        if model.video_offer && pill(ui, Icon::Video, "Video", p.surface_hover, p.text, width).clicked() {
            shared.push(Action::Answer { id: model.id, video: true });
        }
        if pill(ui, Icon::Phone, "Answer", p.success, p.on_accent, width).clicked() {
            shared.push(Action::Answer { id: model.id, video: false });
        }
    });
}

fn outgoing_buttons(ui: &mut Ui, p: &Palette, model: &Model, shared: &Arc<Shared>) {
    let width = ui.available_width();
    if pill(ui, Icon::PhoneOff, "Cancel call", p.danger, p.on_accent, width).clicked() {
        shared.push(Action::Hangup(model.id));
    }
}

/// The expanded row: mute, speaker, hold, record and hang up.
fn active_controls(ui: &mut Ui, model: &Model, shared: &Arc<Shared>) {
    let p = &model.palette;
    // The button's label and the action are separate: "Unmute" still toggles
    // the microphone, and the hang-up button is always the last one.
    #[derive(Clone, Copy)]
    enum Press {
        Mic,
        Speaker,
        Hold,
        Record,
        Hangup,
    }
    let mut buttons: Vec<(Icon, &str, bool, egui::Color32, Press)> = vec![
        (if model.mic_muted { Icon::MicOff } else { Icon::Mic }, if model.mic_muted { "Unmute" } else { "Mute" }, model.mic_muted, p.accent, Press::Mic),
        (if model.speaker_muted { Icon::SpeakerOff } else { Icon::Speaker }, "Speaker", model.speaker_muted, p.accent, Press::Speaker),
    ];
    if model.can_hold {
        buttons.push((if model.held { Icon::Play } else { Icon::Pause }, if model.held { "Resume" } else { "Hold" }, model.held, p.accent, Press::Hold));
    }
    if model.can_record {
        buttons.push((Icon::Record, if model.recording { "Stop rec." } else { "Record" }, model.recording, p.danger, Press::Record));
    }
    buttons.push((Icon::PhoneOff, "Hang up", false, p.danger, Press::Hangup));
    let count = buttons.len() as f32;
    let gap = 2.0;
    let width = ((ui.available_width() - gap * (count - 1.0)) / count).min(62.0);
    let total = count * width + gap * (count - 1.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
        for (icon, label, active, color, press) in buttons {
            if control(ui, p, width, icon, label, active, color).clicked() {
                shared.push(match press {
                    Press::Mic => Action::ToggleMic,
                    Press::Speaker => Action::ToggleSpeaker,
                    Press::Hold => Action::ToggleHold(model.id),
                    Press::Record => Action::ToggleRecording(model.id),
                    Press::Hangup => Action::Hangup(model.id),
                });
            }
        }
    });
}

/// A wide pill button with an icon and a centred label.
fn pill(ui: &mut Ui, icon: Icon, label: &str, fill: egui::Color32, tint: egui::Color32, width: f32) -> egui::Response {
    let button = egui::Button::image_and_text(icon.image(tint, 16.0), egui::RichText::new(label).font(Weight::SemiBold.font_id(13.0)).color(tint))
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(10))
        .min_size(vec2(width, 38.0));
    ui.add(button)
}

/// A compact round in-call button with a label underneath. An active toggle
/// (mute, hold, record) is filled with its colour.
fn control(ui: &mut Ui, p: &Palette, width: f32, icon: Icon, label: &str, active: bool, color: egui::Color32) -> egui::Response {
    let mut response = None;
    ui.allocate_ui_with_layout(vec2(width, 66.0), Layout::top_down(Align::Center), |ui| {
        let (fill, tint) = if label == "Hang up" {
            (p.danger, p.on_accent)
        } else if active {
            (color, p.on_accent)
        } else {
            (p.surface, p.text)
        };
        response = Some(widgets::round_icon_button(ui, 42.0, fill, icon, tint, label));
        ui.add(egui::Label::new(egui::RichText::new(label).size(10.5).color(p.secondary)).truncate());
    });
    response.unwrap_or_else(|| ui.allocate_response(Vec2::ZERO, Sense::hover()))
}
