//! SIP accounts.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    #[default]
    Udp,
    Tcp,
    Tls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SrtpMode {
    #[default]
    Disabled,
    Optional,
    Mandatory,
    Dtls,
    DtlsSdes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Account {
    /// Stable identifier (also names the stored password).
    pub id: u32,
    /// Name shown in menus; falls back to display name / username.
    pub label: String,
    /// Registrar host[:port]; empty means no registration.
    pub server: String,
    /// Outbound proxies, space-separated host[:port].
    pub proxy: String,
    /// Domain appended to numbers that have no `@`.
    pub domain: String,
    pub username: String,
    /// Kept in the OS credential store, never in settings.toml.
    #[serde(skip)]
    pub password: String,
    /// Authentication user if different from `username`.
    pub auth_id: String,
    pub display_name: String,
    /// Prefix added to dialled numbers.
    pub dialing_prefix: String,
    /// Dial plan rules, `|`-separated (see [`crate::dialplan`]).
    pub dial_plan: String,
    /// Hide our caller ID on calls: `Privacy: id` and an anonymous From.
    pub hide_cid: bool,
    /// Number dialled by the voicemail button; enables MWI.
    pub voicemail_number: String,
    pub srtp: SrtpMode,
    pub transport: Transport,
    /// Public address to advertise when STUN is off.
    pub public_addr: String,
    /// Registration expiry, seconds.
    pub register_refresh: u32,
    /// Keep-alive interval, seconds.
    pub keep_alive: u32,
    /// Publish presence.
    pub publish: bool,
    pub ice: bool,
    pub allow_rewrite: bool,
    pub disable_session_timer: bool,
    /// Set up from a JioFiber/AirFiber router ("Connect JioFiber/AirFiber"):
    /// the router decides the server, user, password and transport.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioning: Option<crate::jiofiber::Provisioning>,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            id: 0,
            label: String::new(),
            server: String::new(),
            proxy: String::new(),
            domain: String::new(),
            username: String::new(),
            password: String::new(),
            auth_id: String::new(),
            display_name: String::new(),
            dialing_prefix: String::new(),
            dial_plan: String::new(),
            hide_cid: false,
            voicemail_number: String::new(),
            srtp: SrtpMode::Disabled,
            transport: Transport::Udp,
            public_addr: String::new(),
            register_refresh: 300,
            keep_alive: 15,
            publish: false,
            ice: false,
            allow_rewrite: false,
            disable_session_timer: false,
            provisioning: None,
        }
    }
}

impl Account {
    /// Whether `other` has exactly the same settings: every field but the
    /// id, and for router-made accounts but the credentials' expiry (which
    /// moves with every request even when the credentials don't).
    pub fn same_as(&self, other: &Account) -> bool {
        let comparable = |a: &Account| {
            let mut a = a.clone();
            a.id = 0;
            if let Some(provisioning) = &mut a.provisioning {
                provisioning.expires_at = None;
            }
            a
        };
        comparable(self) == comparable(other)
    }

    /// The account's own number: the username without `sip:` or `@host`.
    pub fn number(&self) -> &str {
        let user = self.username.trim();
        let user = user.strip_prefix("sip:").unwrap_or(user);
        user.split('@').next().unwrap_or(user)
    }

    /// The account's SIP address: `sip:user@host`, the host being the
    /// username's own or else the domain.
    pub fn sip_uri(&self) -> String {
        let user = self.username.trim();
        let user = user.strip_prefix("sip:").unwrap_or(user);
        let domain = self.domain.trim();
        if user.contains('@') || domain.is_empty() { format!("sip:{user}") } else { format!("sip:{user}@{domain}") }
    }

    /// The name to show: label, else display name, else username, else server.
    pub fn title(&self) -> &str {
        [&self.label, &self.display_name, &self.username, &self.server]
            .into_iter()
            .find(|s| !s.trim().is_empty())
            .map_or("Unnamed account", |s| s.as_str())
    }

    /// The key under which the password is stored.
    pub fn secret_key(&self) -> String {
        format!("account-{}", self.id)
    }

    /// Whether enough is filled in to try registering or calling.
    pub fn is_usable(&self) -> bool {
        !self.username.trim().is_empty() && (!self.server.trim().is_empty() || !self.domain.trim().is_empty())
    }

    pub fn proxies(&self) -> Vec<String> {
        self.proxy.split_whitespace().map(str::to_owned).collect()
    }
}

/// The local "call by IP address" account.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalAccount {
    pub enabled: bool,
    pub display_name: String,
    pub username: String,
    pub domain: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_are_the_same_when_all_but_the_id_match() {
        use crate::jiofiber::Provisioning;
        let a = Account { id: 1, label: "Office".into(), server: "pbx.example".into(), username: "1001".into(), password: "pw".into(), ..Account::default() };
        assert!(a.same_as(&Account { id: 7, ..a.clone() }));
        assert!(!a.same_as(&Account { label: "Home".into(), ..a.clone() }));
        assert!(!a.same_as(&Account { password: "other".into(), ..a.clone() }));
        let router = Account { provisioning: Some(Provisioning { router: "192.168.0.1".into(), expires_at: Some(1), ..Provisioning::default() }), ..a.clone() };
        let later = Account { provisioning: Some(Provisioning { expires_at: Some(2), ..router.provisioning.clone().unwrap() }), ..router.clone() };
        assert!(router.same_as(&later), "a new expiry alone doesn't make it different");
        assert!(!router.same_as(&a));
    }

    #[test]
    fn router_provisioning_is_saved_with_the_account() {
        use crate::jiofiber::{Mode, Provisioning};
        let account = Account {
            id: 3,
            provisioning: Some(Provisioning { router: "192.168.0.1".into(), mode: Mode::Temporary, auto_refresh: false, expires_at: Some(1_900_000_000) }),
            ..Account::default()
        };
        let text = toml::to_string(&account).unwrap();
        let read: Account = toml::from_str(&text).unwrap();
        assert_eq!(read.provisioning, account.provisioning);
        // Ordinary accounts carry no provisioning table.
        assert!(!toml::to_string(&Account::default()).unwrap().contains("provisioning"));
        assert_eq!(toml::from_str::<Account>("id = 1").unwrap().provisioning, None);
    }

    #[test]
    fn number_is_the_user_part() {
        let a = Account { username: "+910000000000@sip.example.com".into(), ..Account::default() };
        assert_eq!(a.number(), "+910000000000");
        assert_eq!(Account { username: " sip:1001 ".into(), ..Account::default() }.number(), "1001");
        assert_eq!(a.sip_uri(), "sip:+910000000000@sip.example.com");
        assert_eq!(Account { username: "1001".into(), domain: "pbx.example".into(), ..Account::default() }.sip_uri(), "sip:1001@pbx.example");
    }

    #[test]
    fn title_falls_back() {
        let mut a = Account { server: "sip.example.com".into(), ..Account::default() };
        assert_eq!(a.title(), "sip.example.com");
        a.username = "1001".into();
        assert_eq!(a.title(), "1001");
        a.label = "Office".into();
        assert_eq!(a.title(), "Office");
        assert_eq!(Account::default().title(), "Unnamed account");
    }

    #[test]
    fn password_is_never_serialized() {
        let a = Account { password: "secret".into(), ..Account::default() };
        let text = toml::to_string(&a).unwrap();
        assert!(!text.contains("secret"), "{text}");
        let back: Account = toml::from_str(&text).unwrap();
        assert!(back.password.is_empty());
    }

    #[test]
    fn enums_use_kebab_case() {
        let a: Account = toml::from_str("transport = \"tls\"\nsrtp = \"dtls-sdes\"").unwrap();
        assert_eq!(a.transport, Transport::Tls);
        assert_eq!(a.srtp, SrtpMode::DtlsSdes);
    }

    #[test]
    fn usable_needs_user_and_host() {
        assert!(!Account::default().is_usable());
        assert!(Account { username: "1001".into(), domain: "pbx".into(), ..Account::default() }.is_usable());
        assert_eq!(Account { proxy: " a:5060  b ".into(), ..Account::default() }.proxies(), ["a:5060", "b"]);
    }
}
