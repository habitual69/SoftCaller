//! "Connect JioFiber/AirFiber" (Settings › Accounts): finds the router, shows
//! its voice line, gets SIP credentials for this computer (temporary, or
//! permanent after an OTP), and opens the account editor with them filled in
//! and locked. Nothing is saved until the user saves the account.

use egui::Ui;
use fastframe_fonts::Weight;
use softcaller_core::account::Account;
use softcaller_core::jiofiber::{self as router, Credentials, Mode, Provisioning, RouterAccount};

use super::widgets::{self, card, section_title};
use crate::app::App;
use crate::icons::Icon;
use crate::jiofiber::{self, CredentialsAnswer, Job, OtpSession};
use crate::theme::Palette;

/// OTP attempts before starting over, as the router's own tool allows.
const OTP_ATTEMPTS: u8 = 3;

/// The connect page's progress.
#[derive(Debug)]
pub struct Connect {
    hostname: String,
    step: Step,
    /// The router address typed by the user.
    host_input: String,
    otp_input: String,
    error: Option<String>,
}

#[derive(Debug)]
enum Step {
    Discovering(Job<Option<(String, RouterAccount)>>),
    /// No router answered by itself: ask for its address.
    AskHost,
    Checking { host: String, job: Job<Result<RouterAccount, String>> },
    Found { host: String, account: RouterAccount },
    Requesting { host: String, account: RouterAccount, mode: Mode, job: Job<Result<CredentialsAnswer, String>> },
    /// Permanent credentials need this device registered first.
    NotRegistered { host: String, account: RouterAccount },
    Registering { host: String, account: RouterAccount, job: Job<Result<OtpSession, String>> },
    Otp { host: String, account: RouterAccount, session: OtpSession, attempts: u8 },
    Verifying { host: String, account: RouterAccount, session: OtpSession, attempts: u8, job: Job<Result<(), String>> },
}

impl Connect {
    pub fn start(app: &App) -> Self {
        Self { hostname: jiofiber::hostname(), step: Step::Discovering(Job::start(app.waker(), jiofiber::discover)), host_input: String::new(), otp_input: String::new(), error: None }
    }

    /// Moves on when a running request has finished. Returns the account to
    /// open in the editor once credentials arrived.
    fn advance(&mut self, app: &App) -> Option<Account> {
        let waker = app.waker();
        let step = std::mem::replace(&mut self.step, Step::AskHost);
        self.step = match step {
            Step::Discovering(job) => match job.poll() {
                None => Step::Discovering(job),
                Some(Some((host, account))) => Step::Found { host, account },
                Some(None) => Step::AskHost,
            },
            Step::Checking { host, job } => match job.poll() {
                None => Step::Checking { host, job },
                Some(Ok(account)) => Step::Found { host, account },
                Some(Err(e)) => {
                    self.error = Some(e);
                    Step::AskHost
                }
            },
            Step::Requesting { host, account, mode, job } => match job.poll() {
                None => Step::Requesting { host, account, mode, job },
                Some(Ok(CredentialsAnswer::Credentials(credentials))) => return Some(new_account(&host, mode, &credentials)),
                Some(Ok(CredentialsAnswer::Refused(_))) if mode == Mode::Permanent => Step::NotRegistered { host, account },
                Some(Ok(CredentialsAnswer::Refused(status))) => {
                    self.error = Some(format!("The router refused temporary credentials ({status})."));
                    Step::Found { host, account }
                }
                Some(Err(e)) => {
                    self.error = Some(e);
                    Step::Found { host, account }
                }
            },
            Step::Registering { host, account, job } => match job.poll() {
                None => Step::Registering { host, account, job },
                Some(Ok(session)) => Step::Otp { host, account, session, attempts: 0 },
                Some(Err(e)) => {
                    self.error = Some(e);
                    Step::NotRegistered { host, account }
                }
            },
            Step::Verifying { host, account, session, attempts, job } => match job.poll() {
                None => Step::Verifying { host, account, session, attempts, job },
                Some(Ok(())) => {
                    // Registered: now the permanent credentials come.
                    let (h, name) = (host.clone(), self.hostname.clone());
                    Step::Requesting { host, account, mode: Mode::Permanent, job: Job::start(waker, move || jiofiber::request_credentials(&h, &name, Mode::Permanent)) }
                }
                Some(Err(e)) if attempts + 1 >= OTP_ATTEMPTS => {
                    self.error = Some(format!("{e}. Too many attempts; send a new OTP."));
                    Step::NotRegistered { host, account }
                }
                Some(Err(e)) => {
                    self.error = Some(e);
                    Step::Otp { host, account, session, attempts: attempts + 1 }
                }
            },
            other => other,
        };
        None
    }
}

/// A new, unsaved account with the router's credentials.
fn new_account(host: &str, mode: Mode, credentials: &Credentials) -> Account {
    let mut account = Account { label: "JioFiber".into(), register_refresh: 3600, ..Account::default() };
    router::apply_credentials(&mut account, credentials, host);
    let expires_at = credentials.validity.map(|s| jiff::Timestamp::now().as_second() + i64::try_from(s).unwrap_or(i64::MAX / 2));
    account.provisioning = Some(Provisioning { router: host.to_owned(), mode, auto_refresh: mode == Mode::Temporary, expires_at });
    account
}

/// The connect page, inside the Settings page.
pub fn page(ui: &mut Ui, app: &mut App, p: &Palette) {
    let Some(mut connect) = app.ui.jiofiber.take() else { return };
    if let Some(account) = connect.advance(app) {
        // Credentials in hand: on to the account editor (not saved yet).
        app.ui.account_draft = Some(super::accounts::Draft { account, scroll_to_top: true, ..Default::default() });
        return;
    }
    let mut close = false;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if widgets::icon_button(ui, p, Icon::Back, p.text, "Back to settings").clicked() {
                close = true;
            }
            ui.heading("Connect JioFiber/AirFiber");
        });
        ui.add_space(8.0);
        body(ui, app, p, &mut connect);
    });
    if !close {
        app.ui.jiofiber = Some(connect);
    }
}

/// The current step's content.
fn body(ui: &mut Ui, app: &App, p: &Palette, c: &mut Connect) {
    let waker = app.waker();
    let hostname = c.hostname.clone();
    let mut next = None;
    match &c.step {
        Step::Discovering(_) => busy(ui, p, "Looking for your JioFiber/AirFiber router…"),
        Step::Checking { host, .. } => busy(ui, p, &format!("Checking {host}…")),
        Step::AskHost => {
            card(ui, p, |ui| {
                ui.label(egui::RichText::new("No JioFiber/AirFiber router answered on its own. Enter its address (its domain or IP), as in your browser's address bar for the router.").size(13.0).color(p.secondary));
                ui.add_space(4.0);
                let response = widgets::text_field(ui, p, "Router address", "e.g. 192.168.29.1", &mut c.host_input);
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(6.0);
                if widgets::primary_button(ui, p, "Check", p.accent).clicked() || enter {
                    let host = c.host_input.trim().to_owned();
                    c.error = None;
                    let h = host.clone();
                    next = Some(Step::Checking { host, job: Job::start(waker, move || jiofiber::request_account(&h)) });
                }
            });
        }
        Step::Found { host, account } => {
            line_details(ui, p, host, account);
            ui.add_space(12.0);
            section_title(ui, p, "Register this device");
            card(ui, p, |ui| {
                ui.label(egui::RichText::new(format!("The router gives this computer ({hostname}) its own SIP credentials.")).size(13.0).color(p.secondary));
                ui.add_space(6.0);
                for (mode, title, note) in [
                    (Mode::Permanent, "Register this device permanently", "An OTP is sent to the registered mobile number"),
                    (Mode::Temporary, "Register this device temporarily", "No OTP; the credentials expire and can be renewed"),
                ] {
                    if widgets::primary_button(ui, p, title, p.accent).clicked() {
                        c.error = None;
                        let (h, name) = (host.clone(), hostname.clone());
                        next = Some(Step::Requesting { host: host.clone(), account: account.clone(), mode, job: Job::start(waker, move || jiofiber::request_credentials(&h, &name, mode)) });
                    }
                    ui.label(egui::RichText::new(note).size(12.0).color(p.dim));
                    ui.add_space(6.0);
                }
            });
        }
        Step::Requesting { mode, .. } => busy(ui, p, &format!("Getting {} SIP credentials…", mode.label())),
        Step::NotRegistered { host, account } => {
            line_details(ui, p, host, account);
            ui.add_space(12.0);
            card(ui, p, |ui| {
                ui.label(egui::RichText::new("This device isn't registered for permanent credentials yet. The router will send an OTP to the line's registered mobile number.").size(13.0).color(p.secondary));
                ui.add_space(6.0);
                if widgets::primary_button(ui, p, "Send OTP", p.accent).clicked() {
                    c.error = None;
                    let (h, name) = (host.clone(), hostname.clone());
                    next = Some(Step::Registering { host: host.clone(), account: account.clone(), job: Job::start(waker, move || jiofiber::register_device(&h, &name)) });
                }
                if ui.button("Back").clicked() {
                    c.error = None;
                    next = Some(Step::Found { host: host.clone(), account: account.clone() });
                }
            });
        }
        Step::Registering { .. } => busy(ui, p, "Asking the router to send an OTP…"),
        Step::Otp { host, account, session, attempts } => {
            card(ui, p, |ui| {
                let to = session.sent_to.as_deref().map_or_else(|| "the registered mobile number".to_owned(), |n| n.to_owned());
                ui.label(egui::RichText::new(format!("Enter the OTP sent to {to}.")).size(13.0).color(p.secondary));
                ui.add_space(4.0);
                let response = widgets::text_field(ui, p, "OTP", "Digits from the SMS", &mut c.otp_input);
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if *attempts > 0 {
                    ui.label(egui::RichText::new(format!("{} of {OTP_ATTEMPTS} attempts left", OTP_ATTEMPTS - attempts)).size(12.0).color(p.dim));
                }
                ui.add_space(6.0);
                if widgets::primary_button(ui, p, "Verify", p.accent).clicked() || enter {
                    c.error = None;
                    let (h, s, otp) = (host.clone(), session.clone(), c.otp_input.clone());
                    next = Some(Step::Verifying { host: host.clone(), account: account.clone(), session: session.clone(), attempts: *attempts, job: Job::start(waker, move || jiofiber::verify_otp(&h, &s, &otp)) });
                }
            });
        }
        Step::Verifying { .. } => busy(ui, p, "Checking the OTP…"),
    }
    if let Some(error) = &c.error {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(error).color(p.danger));
    }
    if let Some(step) = next {
        if matches!(step, Step::Verifying { .. }) {
            c.otp_input.clear();
        }
        c.step = step;
    }
}

fn busy(ui: &mut Ui, p: &Palette, text: &str) {
    card(ui, p, |ui| {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(egui::RichText::new(text).size(13.0).color(p.secondary));
        });
    });
}

/// The router's voice line, from `request_account`.
fn line_details(ui: &mut Ui, p: &Palette, host: &str, account: &RouterAccount) {
    section_title(ui, p, "Your JioFiber/AirFiber line");
    card(ui, p, |ui| {
        let rows = [
            ("Phone number", account.msisdn.as_str()),
            ("Router", host),
            ("Router type", account.mode.as_str()),
            ("Serial number", account.serial_number.as_str()),
            ("MAC address", account.mac_address.as_str()),
        ];
        egui::Grid::new("jiofiber-line").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
            for (name, value) in rows.into_iter().filter(|(_, v)| !v.is_empty()) {
                ui.label(egui::RichText::new(name).size(13.0).color(p.secondary));
                ui.label(egui::RichText::new(value).font(Weight::Medium.font_id(13.0)).color(p.text));
                ui.end_row();
            }
            if !account.services.is_empty() {
                ui.label(egui::RichText::new("Services").size(13.0).color(p.secondary));
                ui.label(egui::RichText::new(account.services.join(", ")).size(13.0).color(p.text));
                ui.end_row();
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accounts_carry_the_routers_details() {
        let credentials = Credentials {
            username: "910000000000@ims.example".into(),
            password: "pw".into(),
            home_domain: "ims.example".into(),
            realm: String::new(),
            validity: Some(3600),
        };
        let account = new_account("192.168.0.1", Mode::Temporary, &credentials);
        assert_eq!(account.id, 0, "not saved yet");
        assert_eq!((account.label.as_str(), account.proxy.as_str()), ("JioFiber", "192.168.0.1:5068"));
        let provisioning = account.provisioning.expect("from a router");
        assert_eq!((provisioning.router.as_str(), provisioning.mode, provisioning.auto_refresh), ("192.168.0.1", Mode::Temporary, true));
        assert!(provisioning.expires_at.is_some_and(|t| t > jiff::Timestamp::now().as_second()));
        let permanent = new_account("192.168.0.1", Mode::Permanent, &Credentials { validity: None, ..credentials });
        let provisioning = permanent.provisioning.unwrap();
        assert!(!provisioning.auto_refresh && provisioning.expires_at.is_none());
    }
}
