//! JioFiber / AirFiber routers: SIP credentials for this computer.
//!
//! The router hands out SIP credentials over HTTPS (port 8443) for a device
//! identified by its hostname and a "MAC address" derived from it. There are
//! two kinds:
//!
//! * **Temporary** (`nwk_intf=eth`): issued at once, no OTP; they expire.
//! * **Permanent** (`nwk_intf=wifi`): the device has to be registered first
//!   (`op_type=add`), which sends an OTP to the line's mobile number.
//!
//! The answer is an XML document of `<parm name=… value=…>` elements with the
//! IMS server, user and password. The router itself is the SIP proxy, on port
//! 5068 over TLS. Before any of that, `http://<router>:8080/request_account`
//! tells whether the router has a voice line and which.
//!
//! This module is the pure part: the device identity, the requests'
//! parameters, and reading the router's answers. The HTTP lives in the app.

use serde::{Deserialize, Serialize};

use crate::account::{Account, SrtpMode, Transport};

/// Names the router answers to, tried in this order (the default gateway is
/// tried after the first).
pub const ROUTER_NAME: &str = "jiofiber.local.html";
pub const ROUTER_FALLBACK_IPS: [&str; 2] = ["192.168.29.1", "192.168.31.1"];
/// `http://<router>:ACCOUNT_PORT/request_account` describes the voice line.
pub const ACCOUNT_PORT: u16 = 8080;
/// The credentials service (HTTPS, self-signed certificate).
pub const PROVISIONING_PORT: u16 = 8443;
/// The router's SIP proxy (TLS).
pub const SIP_PORT: u16 = 5068;

/// Which credentials to ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Issued without an OTP; they expire and can be requested again.
    #[default]
    Temporary,
    /// Needs this device registered with an OTP once; they last.
    Permanent,
}

impl Mode {
    /// The `nwk_intf` value that selects the kind of credentials.
    fn network_interface(self) -> &'static str {
        match self {
            Self::Temporary => "eth",
            Self::Permanent => "wifi",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Temporary => "temporary",
            Self::Permanent => "permanent",
        }
    }
}

/// How an account was set up from a router, kept with the account so the
/// editor keeps the router's fields locked and the credentials can be
/// requested again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Provisioning {
    /// The router's name or address, as reached when connecting.
    pub router: String,
    pub mode: Mode,
    /// Request new temporary credentials when the old ones stop working.
    pub auto_refresh: bool,
    /// When the credentials expire (Unix seconds), if the router said.
    pub expires_at: Option<i64>,
}

impl Default for Provisioning {
    fn default() -> Self {
        Self { router: String::new(), mode: Mode::Temporary, auto_refresh: true, expires_at: None }
    }
}

/// The router's device identity for a hostname: pjproject's string hash
/// (`hash * 33 + byte`, 32 bits). The JFC pjproject build puts the same value
/// in every Contact's `+sip.instance`, so both name one device.
pub fn device_hash(hostname: &str) -> u32 {
    hostname.bytes().fold(0u32, |hash, byte| hash.wrapping_mul(33).wrapping_add(u32::from(byte)))
}

/// The "MAC address" the router knows this device by: two zero bytes, then
/// the hash's bytes in memory (little-endian) order, e.g. `00:00:4e:1a:c0:93`.
pub fn device_mac(hostname: &str) -> String {
    let [a, b, c, d] = device_hash(hostname).to_le_bytes();
    format!("00:00:{a:02x}:{b:02x}:{c:02x}:{d:02x}")
}

/// The query parameters for a credentials request, in the router's order.
/// `register` adds `op_type=add`: register this device (sends an OTP).
pub fn credentials_query(hostname: &str, mode: Mode, register: bool) -> Vec<(&'static str, String)> {
    let mut query: Vec<(&'static str, String)> = vec![
        ("terminal_sw_version", "RCSAndrd".into()),
        ("terminal_vendor", hostname.into()),
        ("terminal_model", hostname.into()),
        ("SMS_port", "0".into()),
        ("act_type", "volatile".into()),
        ("IMSI", String::new()),
        ("msisdn", String::new()),
        ("IMEI", String::new()),
        ("vers", "0".into()),
        ("token", String::new()),
        ("rcs_state", "0".into()),
        ("rcs_version", "5.1B".into()),
        ("rcs_profile", "joyn_blackbird".into()),
        ("client_vendor", "JUIC".into()),
        ("default_sms_app", "2".into()),
        ("default_vvm_app", "0".into()),
        ("device_type", "vvm".into()),
        ("client_version", "JSEAndrd-1.0".into()),
        ("mac_address", device_mac(hostname)),
        ("alias", hostname.into()),
        ("nwk_intf", mode.network_interface().into()),
    ];
    if register {
        query.push(("op_type", "add".into()));
    }
    query
}

/// What `request_account` says about the router's voice line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RouterAccount {
    /// The line's phone number.
    pub msisdn: String,
    pub imsi: String,
    /// The router type, e.g. `JFV`.
    pub mode: String,
    pub serial_number: String,
    pub mac_address: String,
    /// The line's services that are switched on (e.g. "Voice mail").
    pub services: Vec<String>,
}

/// Reads `request_account`'s JSON. `None` unless it describes a voice line
/// (a JSON object with a phone number).
pub fn parse_router_account(json: &str) -> Option<RouterAccount> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let object = value.as_object()?;
    let text = |key: &str| object.get(key).and_then(|v| v.as_str()).unwrap_or_default().trim().to_owned();
    let account = RouterAccount {
        msisdn: text("msisdn"),
        imsi: text("imsi"),
        mode: text("mode"),
        serial_number: text("SerialNumber"),
        mac_address: text("mac_address"),
        services: [
            ("VoiceMailSupport", "Voice mail"),
            ("CentralizedCallWaiting", "Call waiting"),
            ("CentralizedCallBlocking", "Call blocking"),
            ("JTCAutoWhitelist", "Auto whitelist"),
        ]
        .iter()
        .filter(|(key, _)| text(key).eq_ignore_ascii_case("true"))
        .map(|(_, name)| (*name).to_owned())
        .collect(),
    };
    (!account.msisdn.is_empty()).then_some(account)
}

/// The SIP credentials from the router's configuration XML.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Credentials {
    /// The IMS user, e.g. `910000000000@ims.example`.
    pub username: String,
    pub password: String,
    /// The IMS home domain: the registrar.
    pub home_domain: String,
    pub realm: String,
    /// How long the credentials last, in seconds, if the router said.
    pub validity: Option<u64>,
}

/// Reads the router's configuration XML (`<parm name=… value=…/>`
/// elements, at any depth; names compared without case).
pub fn parse_credentials(xml: &str) -> Result<Credentials, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("the router's answer isn't valid XML: {e}"))?;
    let mut credentials = Credentials::default();
    for parm in doc.descendants().filter(|n| n.has_tag_name("parm")) {
        let (Some(name), Some(value)) = (parm.attribute("name"), parm.attribute("value")) else { continue };
        let value = value.trim().to_owned();
        match name.to_ascii_lowercase().as_str() {
            "username" => credentials.username = value,
            "userpwd" => credentials.password = value,
            "home_network_domain_name" => credentials.home_domain = value,
            "realm" => credentials.realm = value,
            "validity" => credentials.validity = value.parse().ok().filter(|&s| s > 0),
            _ => {}
        }
    }
    if credentials.username.is_empty() || credentials.password.is_empty() || credentials.home_domain.is_empty() {
        return Err("the router's answer has no SIP user, password or server".into());
    }
    Ok(credentials)
}

/// Fills in what the router decides: the IMS server, the router as proxy
/// and domain (TLS, port 5068), the user (`+<number>@<domain>`, logging in
/// without the `+`) and the password. The rest of `account` stays as it is.
pub fn apply_credentials(account: &mut Account, credentials: &Credentials, router: &str) {
    let proxy = format!("{router}:{SIP_PORT}");
    account.server = credentials.home_domain.clone();
    account.proxy = proxy.clone();
    account.domain = proxy;
    let user = credentials.username.trim_start_matches('+');
    account.username = format!("+{user}");
    account.auth_id = user.to_owned();
    account.password = credentials.password.clone();
    account.transport = Transport::Tls;
    // JioFiber's IMS offers plain RTP in incoming INVITEs (the signaling is
    // still TLS). Mandatory DTLS-SRTP makes pjsua answer 488 before the call
    // reaches the app, so allow both: SRTP when the other side asks for it,
    // plain RTP otherwise.
    account.srtp = SrtpMode::Optional;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostnames_hash_like_pjproject() {
        assert_eq!(device_hash(""), 0);
        assert_eq!(device_hash("a"), 97);
        assert_eq!(device_hash("ab"), 97 * 33 + 98);
        // Wraps at 32 bits.
        let long = "DESKTOP-TEST1234";
        let expected = long.bytes().fold(0u64, |h, b| (h * 33 + u64::from(b)) & 0xFFFF_FFFF);
        assert_eq!(u64::from(device_hash(long)), expected);
    }

    #[test]
    fn the_mac_is_the_hash_in_memory_order() {
        let hash = device_hash("DESKTOP-TEST1234");
        let [a, b, c, d] = hash.to_le_bytes();
        assert_eq!(device_mac("DESKTOP-TEST1234"), format!("00:00:{a:02x}:{b:02x}:{c:02x}:{d:02x}"));
        assert_eq!(device_mac("ab"), "00:00:e3:0c:00:00");
    }

    #[test]
    fn queries_pick_the_kind_of_credentials() {
        let temporary = credentials_query("pc", Mode::Temporary, false);
        assert_eq!(temporary.first().map(|(k, _)| *k), Some("terminal_sw_version"));
        assert!(temporary.contains(&("nwk_intf", "eth".into())));
        assert!(temporary.contains(&("mac_address", device_mac("pc"))));
        assert!(!temporary.iter().any(|(k, _)| *k == "op_type"));
        let register = credentials_query("pc", Mode::Permanent, true);
        assert!(register.contains(&("nwk_intf", "wifi".into())));
        assert_eq!(register.last(), Some(&("op_type", "add".into())));
    }

    #[test]
    fn router_accounts_need_a_phone_number() {
        let json = r#"{"imsi":"001010000000001","msisdn":"5550100001","mcc":"001","mnc":"01","mode":"JFV",
            "mac_address":"02:00:00:00:00:01","SerialNumber":"TEST0001","VoiceMailSupport":"true",
            "CentralizedCallWaiting":"false"}"#;
        let account = parse_router_account(json).expect("a voice line");
        assert_eq!((account.msisdn.as_str(), account.mode.as_str(), account.serial_number.as_str()), ("5550100001", "JFV", "TEST0001"));
        assert_eq!(account.services, ["Voice mail"]);
        assert_eq!(parse_router_account(r#"{"msisdn":""}"#), None);
        assert_eq!(parse_router_account("<html>not json</html>"), None);
    }

    const CONFIG: &str = r#"<?xml version="1.0"?>
        <wap-provisioningdoc version="1.1">
          <characteristic type="VERS"><parm name="version" value="1"/><parm name="validity" value="86400"/></characteristic>
          <characteristic type="IMS">
            <parm name="home_network_domain_name" value="ims.example"/>
            <parm name="address" value="10.0.0.1"/>
            <parm name="realm" value="ims.example"/>
            <parm name="username" value="910000000000@ims.example"/>
            <parm name="userpwd" value="s3cret"/>
          </characteristic>
        </wap-provisioningdoc>"#;

    #[test]
    fn credentials_come_from_the_parms() {
        let c = parse_credentials(CONFIG).unwrap();
        assert_eq!(
            (c.username.as_str(), c.password.as_str(), c.home_domain.as_str(), c.validity),
            ("910000000000@ims.example", "s3cret", "ims.example", Some(86400))
        );
        assert!(parse_credentials("<x><parm name=\"username\" value=\"u\"/></x>").is_err());
        assert!(parse_credentials("not xml").is_err());
    }

    #[test]
    fn credentials_fill_the_routers_fields() {
        let mut account = Account { label: "Home".into(), display_name: "Ada".into(), ..Account::default() };
        apply_credentials(&mut account, &parse_credentials(CONFIG).unwrap(), "192.168.0.1");
        assert_eq!(account.server, "ims.example");
        assert_eq!((account.proxy.as_str(), account.domain.as_str()), ("192.168.0.1:5068", "192.168.0.1:5068"));
        assert_eq!((account.username.as_str(), account.auth_id.as_str()), ("+910000000000@ims.example", "910000000000@ims.example"));
        assert_eq!((account.password.as_str(), account.transport), ("s3cret", Transport::Tls));
        assert_eq!(account.srtp, SrtpMode::Optional, "JioFiber's calls arrive as plain RTP");
        assert_eq!((account.label.as_str(), account.display_name.as_str()), ("Home", "Ada"), "the user's own fields stay");
    }
}
