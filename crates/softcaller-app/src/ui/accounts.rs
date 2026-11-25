//! The account list (in Settings) and the account editor.

use egui::{Sense, Ui, Vec2, vec2};
use fastframe_fonts::Weight;
use softcaller_core::account::{Account, SrtpMode, Transport};
use softcaller_core::jiofiber::{Mode, Provisioning};

use super::widgets::{self, card, section_title, setting_row};
use crate::app::App;
use crate::icons::Icon;
use crate::sip::Status;
use crate::theme::Palette;

/// An account being edited. `account.id == 0` means a new account.
#[derive(Debug, Default)]
pub struct Draft {
    pub account: Account,
    pub show_password: bool,
    pub confirm_delete: bool,
    pub error: Option<String>,
    /// Scroll the editor to the top on this frame (when it just opened).
    pub scroll_to_top: bool,
}

/// What the Save button should do with a draft.
#[derive(Debug, PartialEq, Eq)]
enum SaveCheck {
    Save,
    /// An existing account saved without changes: nothing to write.
    Unchanged,
}

impl Draft {
    /// Whether the draft can be saved, given the saved `accounts`: it needs
    /// a user and a host, and may not repeat another account exactly.
    fn validate(&self, accounts: &[Account]) -> Result<SaveCheck, String> {
        let a = &self.account;
        if a.username.trim().is_empty() {
            return Err("Enter a username.".into());
        }
        if a.server.trim().is_empty() && a.domain.trim().is_empty() {
            return Err("Enter a SIP server or a domain.".into());
        }
        if accounts.iter().any(|other| other.id != a.id && other.same_as(a)) {
            return Err("An account with exactly these settings already exists.".into());
        }
        if a.id != 0 && accounts.iter().any(|saved| saved.id == a.id && saved.same_as(a)) {
            return Ok(SaveCheck::Unchanged);
        }
        Ok(SaveCheck::Save)
    }
}

/// The Accounts section of the Settings page.
pub fn list(ui: &mut Ui, app: &mut App, p: &Palette) {
    section_title(ui, p, "Accounts");
    let status = app.status();
    card(ui, p, |ui| {
        let mut select = None;
        let mut edit = None;
        for account in &app.settings.accounts {
            let active = app.settings.active_account == Some(account.id);
            match account_row(ui, p, account, active, active.then_some(&status)) {
                RowAction::Select => select = Some(account.id),
                RowAction::Edit => edit = Some(account.id),
                RowAction::None => {}
            }
        }
        if let Some(id) = select {
            app.settings.active_account = Some(id);
            app.mark_settings_dirty();
        }
        if let Some(id) = edit
            && let Some(account) = app.settings.account(id)
        {
            app.ui.account_draft = Some(Draft { account: account.clone(), scroll_to_top: true, ..Draft::default() });
        }
        if app.settings.accounts.is_empty() {
            ui.label(egui::RichText::new("No accounts yet. Add your SIP account to make and receive calls.").size(13.0).color(p.dim));
        }
        ui.add_space(4.0);
        if ui.add(egui::Button::new(egui::RichText::new("＋  Add account").font(Weight::Medium.font_id(14.0)))).clicked() {
            app.ui.account_draft = Some(Draft { scroll_to_top: true, ..Draft::default() });
        }
        ui.add_space(2.0);
        let connect = egui::Button::image_and_text(Icon::Router.image(p.text, 16.0), egui::RichText::new("Connect JioFiber/AirFiber").font(Weight::Medium.font_id(14.0)));
        if ui.add(connect).clicked() {
            app.ui.jiofiber = Some(super::jiofiber::Connect::start(app));
        }
    });

    ui.add_space(12.0);
    card(ui, p, |ui| {
        let mut enabled = app.settings.local_account.enabled;
        setting_row(ui, p, "Call by IP address", Some("A local account without a server, listening on port 5060"), |ui| {
            if widgets::toggle(ui, p, &mut enabled, "Call by IP address").changed() {
                app.settings.local_account.enabled = enabled;
                app.mark_settings_dirty();
            }
        });
    });
}

enum RowAction {
    None,
    Select,
    Edit,
}

fn account_row(ui: &mut Ui, p: &Palette, account: &Account, active: bool, status: Option<&Status>) -> RowAction {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), Sense::click());
    let label = format!("{}{}", account.title(), if active { " (active)" } else { "" });
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, active, &label));
    let hover = ui.ctx().animate_bool_with_time(response.id, response.hovered(), 0.12);
    let painter = ui.painter();
    if hover > 0.0 {
        painter.rect_filled(rect.expand2(vec2(6.0, 0.0)), egui::CornerRadius::same(8), p.surface_hover.gamma_multiply(hover));
    }
    // Radio mark.
    let radio = egui::pos2(rect.left() + 10.0, rect.center().y);
    painter.circle_stroke(radio, 8.0, egui::Stroke::new(1.5, if active { p.accent } else { p.dim }));
    if active {
        painter.circle_filled(radio, 4.5, p.accent);
    }
    let text_x = rect.left() + 28.0;
    painter.text(egui::pos2(text_x, rect.top() + 15.0), egui::Align2::LEFT_CENTER, account.title(), Weight::Medium.font_id(14.0), p.text);
    let transport = match account.transport {
        Transport::Udp => "UDP",
        Transport::Tcp => "TCP",
        Transport::Tls => "TLS",
    };
    let host = if account.server.is_empty() { &account.domain } else { &account.server };
    let (detail, color) = match status {
        Some(Status::Error(e)) => (e.clone(), p.danger),
        Some(s) => (format!("{} · {transport} · {host}", s.label()), if s.is_online() { p.success } else { p.dim }),
        None => (format!("{transport} · {host}"), p.dim),
    };
    let detail_rect = egui::Rect::from_min_max(egui::pos2(text_x, rect.top() + 26.0), egui::pos2(rect.right() - 44.0, rect.bottom() - 4.0));
    let galley = widgets::elided(ui, detail, Weight::Regular.font_id(12.0), color, detail_rect.width());
    ui.painter().galley(detail_rect.min, galley, color);

    let edit_rect = egui::Rect::from_center_size(egui::pos2(rect.right() - 18.0, rect.center().y), Vec2::splat(34.0));
    let edited = ui.put(edit_rect, |ui: &mut Ui| widgets::icon_button(ui, p, Icon::Pencil, p.secondary, &format!("Edit {}", account.title()))).clicked();
    if edited {
        RowAction::Edit
    } else if response.clicked() && !active {
        RowAction::Select
    } else {
        RowAction::None
    }
}

/// The account editor page.
pub fn editor(ui: &mut Ui, app: &mut App, p: &Palette) {
    let Some(mut draft) = app.ui.account_draft.take() else { return };
    let mut close = false;
    let scroll_to_top = std::mem::take(&mut draft.scroll_to_top);
    let mut area = egui::ScrollArea::vertical().auto_shrink(false).id_salt("account-editor");
    if scroll_to_top {
        area = area.vertical_scroll_offset(0.0);
    }

    area.show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if widgets::icon_button(ui, p, Icon::Back, p.text, "Back to settings").clicked() {
                close = true;
            }
            ui.heading(if draft.account.id == 0 { "New account" } else { "Edit account" });
        });
        ui.add_space(8.0);
        // A saved JioFiber/AirFiber account: whether its credentials need
        // renewing, and how that's going.
        let id = draft.account.id;
        let refresh = (id != 0 && draft.account.provisioning.is_some()).then(|| Refresh {
            stale: app.router_credentials_stale(id),
            running: app.router_refresh_running(id),
            error: app.router_refresh_error.as_ref().filter(|(account, _)| *account == id).map(|(_, e)| e.clone()),
        });
        let a = &mut draft.account;
        // Set up from a JioFiber/AirFiber router: the router decides the
        // server, user, password and transport.
        let locked = a.provisioning.is_some();
        if let Some(provisioning) = &mut a.provisioning {
            if router_card(ui, p, provisioning, refresh.as_ref()) {
                app.refresh_router_credentials(id);
            }
            ui.add_space(12.0);
        }

        section_title(ui, p, "Account");
        card(ui, p, |ui| {
            let wide = ui.available_width() >= 560.0;
            pair(ui, wide, |ui| {
                widgets::text_field(ui, p, "Account name", "Optional, e.g. Office", &mut a.label);
            }, |ui| {
                widgets::text_field(ui, p, "Display name", "Optional", &mut a.display_name);
            });
            ui.add_space(6.0);
            pair(ui, wide, |ui| {
                lockable(ui, locked, |ui| {
                    widgets::text_field(ui, p, "SIP server", "Registrar, e.g. sip.example.com", &mut a.server);
                });
            }, |ui| {
                lockable(ui, locked, |ui| {
                    widgets::text_field(ui, p, "SIP proxy", "Optional outbound proxy(s), space-separated", &mut a.proxy);
                });
            });
            ui.add_space(6.0);
            pair(ui, wide, |ui| {
                lockable(ui, locked, |ui| {
                    widgets::text_field(ui, p, "Username", "User or number", &mut a.username);
                });
            }, |ui| {
                lockable(ui, locked, |ui| {
                    widgets::text_field(ui, p, "Domain", "Added to numbers without @", &mut a.domain);
                });
            });
            ui.add_space(6.0);
            pair(ui, wide, |ui| {
                widgets::password_field(ui, p, "Password", &mut a.password, &mut draft.show_password, locked);
            }, |ui| {
                lockable(ui, locked, |ui| {
                    widgets::text_field(ui, p, "Login", "Authentication user, if not the username", &mut a.auth_id);
                });
            });
        });

        ui.add_space(12.0);
        section_title(ui, p, "Connection");
        card(ui, p, |ui| {
            let wide = ui.available_width() >= 560.0;
            pair(ui, wide, |ui| {
                ui.label(egui::RichText::new("Transport").size(12.0).color(p.secondary));
                lockable(ui, locked, |ui| {
                    widgets::segmented(ui, p, &mut a.transport, &[(Transport::Udp, None, "UDP"), (Transport::Tcp, None, "TCP"), (Transport::Tls, Some(Icon::Lock), "TLS")]);
                });
            }, |ui| {
                ui.label(egui::RichText::new("Media encryption (SRTP)").size(12.0).color(p.secondary));
                egui::ComboBox::from_id_salt("srtp")
                    .width(ui.available_width())
                    .selected_text(srtp_label(a.srtp))
                    .show_ui(ui, |ui| {
                        for mode in [SrtpMode::Disabled, SrtpMode::Optional, SrtpMode::Mandatory, SrtpMode::Dtls, SrtpMode::DtlsSdes] {
                            ui.selectable_value(&mut a.srtp, mode, srtp_label(mode));
                        }
                    });
            });
            ui.add_space(6.0);
            widgets::text_field(ui, p, "Public address", "Optional, used when STUN is off", &mut a.public_addr);
            ui.add_space(6.0);
            pair(ui, wide, |ui| {
                widgets::number_field(ui, p, "Register refresh", &mut a.register_refresh, 10..=86400, " s");
            }, |ui| {
                widgets::number_field(ui, p, "Keep-alive", &mut a.keep_alive, 0..=3600, " s");
            });
            ui.separator();
            pair(ui, wide, |ui| {
                setting_row(ui, p, "ICE", Some("NAT traversal for media"), |ui| {
                    widgets::toggle(ui, p, &mut a.ice, "ICE");
                });
            }, |ui| {
                setting_row(ui, p, "Allow IP rewrite", Some("Use the address the server sees"), |ui| {
                    widgets::toggle(ui, p, &mut a.allow_rewrite, "Allow IP rewrite");
                });
            });
            pair(ui, wide, |ui| {
                setting_row(ui, p, "Disable session timers", None, |ui| {
                    widgets::toggle(ui, p, &mut a.disable_session_timer, "Disable session timers");
                });
            }, |ui| {
                setting_row(ui, p, "Publish presence", None, |ui| {
                    widgets::toggle(ui, p, &mut a.publish, "Publish presence");
                });
            });
        });

        ui.add_space(12.0);
        section_title(ui, p, "Calling");
        card(ui, p, |ui| {
            let wide = ui.available_width() >= 560.0;
            pair(ui, wide, |ui| {
                widgets::text_field(ui, p, "Voicemail number", "Dialled by the voicemail button; enables MWI", &mut a.voicemail_number);
            }, |ui| {
                widgets::text_field(ui, p, "Dialing prefix", "Added to dialled numbers", &mut a.dialing_prefix);
            });
            ui.add_space(6.0);
            widgets::text_field(ui, p, "Dial plan", "e.g. <9:>1xxxxxxxxxx|xxx (x digit, . repeat, <a:b> replace)", &mut a.dial_plan);
            ui.add_space(4.0);
            setting_row(ui, p, "Hide caller ID", None, |ui| {
                widgets::toggle(ui, p, &mut a.hide_cid, "Hide caller ID");
            });
        });

        ui.add_space(12.0);
        if let Some(error) = &draft.error {
            ui.label(egui::RichText::new(error).color(p.danger));
        }
        if ui.available_width() >= 360.0 {
            ui.columns(2, |cols| {
                if widgets::primary_button(&mut cols[0], p, "Save", p.accent).clicked() {
                    match draft.validate(&app.settings.accounts) {
                        Ok(SaveCheck::Save) => {
                            app.save_account(draft.account.clone());
                            close = true;
                        }
                        Ok(SaveCheck::Unchanged) => close = true,
                        Err(e) => draft.error = Some(e),
                    }
                }
                if cols[1].add_sized([cols[1].available_width(), 36.0], egui::Button::new("Cancel")).clicked() {
                    close = true;
                }
            });
        } else {
            if widgets::primary_button(ui, p, "Save", p.accent).clicked() {
                match draft.validate(&app.settings.accounts) {
                    Ok(SaveCheck::Save) => {
                        app.save_account(draft.account.clone());
                        close = true;
                    }
                    Ok(SaveCheck::Unchanged) => close = true,
                    Err(e) => draft.error = Some(e),
                }
            }
            ui.add_space(6.0);
            if ui.add_sized([ui.available_width(), 36.0], egui::Button::new("Cancel")).clicked() {
                close = true;
            }
        }
        if draft.account.id != 0 {
            ui.add_space(8.0);
            let text = if draft.confirm_delete { "Click again to delete this account" } else { "Delete account" };
            if widgets::primary_button(ui, p, text, p.danger).clicked() {
                if draft.confirm_delete {
                    app.delete_account(draft.account.id);
                    close = true;
                } else {
                    draft.confirm_delete = true;
                }
            }
        }
        ui.add_space(12.0);
    });

    if !close {
        app.ui.account_draft = Some(draft);
    }
}

/// Draws `add` read-only when `locked` (the router decides those fields).
fn lockable(ui: &mut Ui, locked: bool, add: impl FnOnce(&mut Ui)) {
    if locked {
        ui.add_enabled_ui(false, add);
    } else {
        add(ui);
    }
}

/// Two fields side by side when the card is wide enough, stacked when it
/// isn't. The narrow layout keeps every field inside the window.
fn pair(ui: &mut Ui, wide: bool, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui)) {
    if wide {
        ui.columns(2, |cols| {
            left(&mut cols[0]);
            right(&mut cols[1]);
        });
    } else {
        left(ui);
        ui.add_space(6.0);
        right(ui);
    }
}

/// A saved router-made account's credentials, for the refresh button.
struct Refresh {
    /// Rejected by the registrar, or expired.
    stale: bool,
    running: bool,
    error: Option<String>,
}

/// Where a router-made account's credentials come from, whether they are
/// renewed by themselves, and (when they no longer work) a button to renew
/// them. Returns whether that button was clicked.
fn router_card(ui: &mut Ui, p: &Palette, provisioning: &mut Provisioning, refresh: Option<&Refresh>) -> bool {
    section_title(ui, p, "JioFiber/AirFiber");
    card(ui, p, |ui| {
        let kind = match provisioning.mode {
            Mode::Temporary => "temporary",
            Mode::Permanent => "permanent",
        };
        ui.label(
            egui::RichText::new(format!(
                "Set up from the router at {} with {kind} SIP credentials. The router decides the server, user, password and transport.",
                provisioning.router
            ))
            .size(13.0)
            .color(p.secondary),
        );
        if let Some(expires_at) = provisioning.expires_at.and_then(|t| jiff::Timestamp::from_second(t).ok()) {
            let local = expires_at.to_zoned(jiff::tz::TimeZone::system());
            ui.label(egui::RichText::new(format!("The credentials expire on {}.", local.strftime("%-d %b %Y, %H:%M"))).size(12.0).color(p.dim));
        }
        if provisioning.mode == Mode::Temporary {
            ui.add_space(4.0);
            setting_row(ui, p, "Refresh SIP credentials automatically", Some("Ask the router for new ones when these expire or stop working"), |ui| {
                widgets::toggle(ui, p, &mut provisioning.auto_refresh, "Refresh SIP credentials automatically");
            });
        }
        let mut clicked = false;
        if let Some(refresh) = refresh {
            if refresh.running {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new("Getting new SIP credentials from the router…").size(13.0).color(p.secondary));
                });
            } else if refresh.stale {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("The router no longer accepts these credentials (incorrect username or password), or they have expired.").size(13.0).color(p.danger));
                clicked = widgets::primary_button(ui, p, "Refresh SIP credentials", p.accent).clicked();
            }
            if let Some(error) = &refresh.error {
                ui.label(egui::RichText::new(error).size(12.0).color(p.danger));
            }
        }
        clicked
    })
}

fn srtp_label(mode: SrtpMode) -> &'static str {
    match mode {
        SrtpMode::Disabled => "Disabled",
        SrtpMode::Optional => "Optional (SDES)",
        SrtpMode::Mandatory => "Mandatory (SDES)",
        SrtpMode::Dtls => "DTLS-SRTP",
        SrtpMode::DtlsSdes => "DTLS-SRTP, then SDES",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drafts_need_user_and_host() {
        let mut d = Draft::default();
        assert!(d.validate(&[]).is_err());
        d.account.username = "1001".into();
        assert!(d.validate(&[]).is_err());
        d.account.domain = "pbx.local".into();
        assert_eq!(d.validate(&[]), Ok(SaveCheck::Save));
    }

    #[test]
    fn an_identical_account_is_not_saved_twice() {
        let saved = Account { id: 1, username: "1001".into(), domain: "pbx.local".into(), password: "pw".into(), ..Account::default() };
        // The same settings again as a new account: refused.
        let again = Draft { account: Account { id: 0, ..saved.clone() }, ..Draft::default() };
        assert!(again.validate(std::slice::from_ref(&saved)).is_err());
        // Different in anything (here the name): a second account is fine.
        let other = Draft { account: Account { id: 0, label: "Second".into(), ..saved.clone() }, ..Draft::default() };
        assert_eq!(other.validate(std::slice::from_ref(&saved)), Ok(SaveCheck::Save));
        // The account itself, unchanged: nothing to save.
        let same = Draft { account: saved.clone(), ..Draft::default() };
        assert_eq!(same.validate(std::slice::from_ref(&saved)), Ok(SaveCheck::Unchanged));
        let edited = Draft { account: Account { display_name: "Ada".into(), ..saved.clone() }, ..Draft::default() };
        assert_eq!(edited.validate(std::slice::from_ref(&saved)), Ok(SaveCheck::Save));
    }
}
