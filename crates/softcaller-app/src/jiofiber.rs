//! Talking to a JioFiber/AirFiber router for SIP credentials (see
//! `softcaller_core::jiofiber` for what the router expects and answers).
//!
//! Everything here blocks, so the UI runs it on worker threads ([`Job`]).
//! The credentials service uses HTTPS with the router's self-signed
//! certificate, so its requests skip certificate checks: they only ever go
//! to the router on the local network. The requests are written by hand,
//! like the router's own tools send them.

use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use fastframe_shell::Waker;
use softcaller_core::jiofiber::{self, Credentials, Mode, RouterAccount};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const IO_TIMEOUT: Duration = Duration::from_secs(30);

/// A blocking task on a thread of its own; [`Job::poll`] takes its result.
pub struct Job<T>(Receiver<T>);

impl<T> std::fmt::Debug for Job<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Job")
    }
}

impl<T: Send + 'static> Job<T> {
    pub fn start(waker: &Waker, work: impl FnOnce() -> T + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let waker = waker.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work());
            waker.wake();
        });
        Self(rx)
    }

    /// The result once the task has finished.
    pub fn poll(&self) -> Option<T> {
        match self.0.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => panic!("a router task ended without a result"),
        }
    }
}

/// This computer's name, as the router and pjproject know it.
pub fn hostname() -> String {
    gethostname::gethostname().to_string_lossy().into_owned()
}

/// Where to look for the router: its name, this computer's default
/// gateway, then the usual addresses; without repeats.
pub fn candidates() -> Vec<String> {
    let mut hosts = vec![jiofiber::ROUTER_NAME.to_owned()];
    match netdev::get_default_gateway() {
        Ok(gateway) => hosts.extend(gateway.ipv4.iter().map(ToString::to_string)),
        Err(e) => log::info!("no default gateway: {e}"),
    }
    hosts.extend(jiofiber::ROUTER_FALLBACK_IPS.iter().map(|ip| (*ip).to_owned()));
    let mut seen = std::collections::HashSet::new();
    hosts.retain(|host| seen.insert(host.clone()));
    hosts
}

/// The first of [`candidates`] that describes a voice line.
pub fn discover() -> Option<(String, RouterAccount)> {
    candidates().into_iter().find_map(|host| {
        let account = request_account(&host).map_err(|e| log::info!("no router at {host}: {e}")).ok()?;
        Some((host, account))
    })
}

/// `http://<host>:8080/request_account`: the router's voice line.
pub fn request_account(host: &str) -> Result<RouterAccount, String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("enter the router's address".into());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .build()
        .into();
    let url = format!("http://{}:{}/request_account", url_host(host), jiofiber::ACCOUNT_PORT);
    let mut response = agent.get(&url).call().map_err(|e| format!("cannot reach {host}: {e}"))?;
    let status = response.status();
    let body = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("{host} answered {status}"));
    }
    jiofiber::parse_router_account(&body).ok_or_else(|| format!("{host} has no SIP voice line"))
}

/// What a credentials request got.
#[derive(Debug)]
pub enum CredentialsAnswer {
    Credentials(Credentials),
    /// The router refused (for permanent credentials: this device isn't
    /// registered yet).
    Refused(u16),
}

/// Asks the router for this device's credentials.
pub fn request_credentials(host: &str, hostname: &str, mode: Mode) -> Result<CredentialsAnswer, String> {
    let query = encoded_query(&jiofiber::credentials_query(hostname, mode, false));
    let response = https_get(host, &format!("/?{query}"), &[])?;
    if response.status != 200 {
        log::info!("the router refused {} credentials: {}", mode.label(), response.status);
        return Ok(CredentialsAnswer::Refused(response.status));
    }
    jiofiber::parse_credentials(&response.body).map(CredentialsAnswer::Credentials)
}

/// New credentials of the kind an account already has (no OTP: a device
/// the router no longer knows has to be connected again).
pub fn renew_credentials(host: &str, hostname: &str, mode: Mode) -> Result<Credentials, String> {
    match request_credentials(host, hostname, mode)? {
        CredentialsAnswer::Credentials(credentials) => Ok(credentials),
        CredentialsAnswer::Refused(status) if mode == Mode::Permanent => {
            Err(format!("the router no longer has this device registered ({status}); use Connect JioFiber/AirFiber again"))
        }
        CredentialsAnswer::Refused(status) => Err(format!("the router refused new credentials ({status})")),
    }
}

/// An OTP on its way: where to, and the router's session for checking it.
#[derive(Debug, Clone)]
pub struct OtpSession {
    /// The (masked) number the OTP went to, if the router said.
    pub sent_to: Option<String>,
    cookie: String,
}

/// Registers this device for permanent credentials; the router sends an
/// OTP to the line's mobile number.
pub fn register_device(host: &str, hostname: &str) -> Result<OtpSession, String> {
    // As the router's own tools send it: the values as they are and a
    // trailing `&`.
    let query: String = jiofiber::credentials_query(hostname, Mode::Permanent, true).iter().map(|(k, v)| format!("{k}={v}&")).collect();
    let response = https_get(host, &format!("/?{query}"), &[])?;
    if response.status != 200 {
        return Err(format!("the router didn't register this device ({}){}", response.status, detail(&response.body)));
    }
    Ok(OtpSession { sent_to: response.header("x-amn").map(str::to_owned), cookie: response.header("set-cookie").unwrap_or_default().to_owned() })
}

/// Checks the OTP; on success the device is registered.
pub fn verify_otp(host: &str, session: &OtpSession, otp: &str) -> Result<(), String> {
    let otp = otp.trim();
    if otp.is_empty() || !otp.bytes().all(|b| b.is_ascii_digit()) {
        return Err("enter the OTP's digits".into());
    }
    let mut headers = Vec::new();
    if !session.cookie.is_empty() {
        headers.push(("Cookie", session.cookie.as_str()));
    }
    let response = https_get(host, &format!("/?OTP={otp}"), &headers)?;
    if response.status != 200 {
        return Err(format!("the router didn't accept the OTP ({}){}", response.status, detail(&response.body)));
    }
    Ok(())
}

fn detail(body: &str) -> String {
    let body = body.trim();
    if body.is_empty() || body.len() > 200 { String::new() } else { format!(": {body}") }
}

/// `key=value&…`, percent-encoded like a form.
fn encoded_query(query: &[(&str, String)]) -> String {
    use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
    const VALUE: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');
    query.iter().map(|(k, v)| format!("{k}={}", utf8_percent_encode(v, VALUE))).collect::<Vec<_>>().join("&")
}

/// A host for a URL: IPv6 addresses in brackets.
fn url_host(host: &str) -> String {
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V6(ip)) => format!("[{ip}]"),
        _ => host.to_owned(),
    }
}

/// An HTTP response, read to the end.
#[derive(Debug)]
struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Response {
    /// The last header named `name` (case-insensitive).
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().rev().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    fn parse(raw: &[u8]) -> Result<Self, String> {
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("the router's answer has no HTTP header")?;
        let head = String::from_utf8_lossy(&raw[..split]);
        let mut lines = head.split("\r\n");
        let status_line = lines.next().unwrap_or_default();
        let status = status_line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| format!("unexpected answer: {status_line}"))?;
        let headers: Vec<(String, String)> =
            lines.filter_map(|line| line.split_once(':')).map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned())).collect();
        let mut body = raw[split + 4..].to_vec();
        let chunked = headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked"));
        if chunked {
            body = dechunk(&body);
        }
        Ok(Self { status, headers, body: String::from_utf8_lossy(&body).into_owned() })
    }
}

/// The data of a `Transfer-Encoding: chunked` body.
fn dechunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(line_end) = body.windows(2).position(|w| w == b"\r\n") {
        let size_text = String::from_utf8_lossy(&body[..line_end]);
        let Ok(size) = usize::from_str_radix(size_text.split(';').next().unwrap_or_default().trim(), 16) else { break };
        let start = line_end + 2;
        if size == 0 || start + size > body.len() {
            break;
        }
        out.extend_from_slice(&body[start..start + size]);
        body = body.get(start + size + 2..).unwrap_or_default();
    }
    out
}

/// `GET https://<host>:8443<path>`, accepting the router's self-signed
/// certificate.
fn https_get(host: &str, path: &str, headers: &[(&str, &str)]) -> Result<Response, String> {
    let host = host.trim();
    let address = (host, jiofiber::PROVISIONING_PORT)
        .to_socket_addrs()
        .map_err(|e| format!("cannot find {host}: {e}"))?
        .next()
        .ok_or_else(|| format!("cannot find {host}"))?;
    let tcp = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).map_err(|e| format!("cannot reach {host}: {e}"))?;
    tcp.set_read_timeout(Some(IO_TIMEOUT)).map_err(|e| e.to_string())?;
    tcp.set_write_timeout(Some(IO_TIMEOUT)).map_err(|e| e.to_string())?;

    let server_name = rustls::pki_types::ServerName::try_from(host.to_owned()).map_err(|e| format!("{host}: {e}"))?;
    let tls = rustls::ClientConnection::new(any_certificate_config(), server_name).map_err(|e| e.to_string())?;
    let mut stream = rustls::StreamOwned::new(tls, tcp);

    let mut request = format!("GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n", url_host(host));
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).map_err(|e| format!("cannot talk to {host}: {e}"))?;

    let mut raw = Vec::new();
    if let Err(e) = stream.read_to_end(&mut raw) {
        // Routers often close without TLS's close_notify; what came is the answer.
        if e.kind() != std::io::ErrorKind::UnexpectedEof || raw.is_empty() {
            return Err(format!("no answer from {host}: {e}"));
        }
    }
    Response::parse(&raw)
}

/// TLS that accepts the router's certificate as it is: self-signed, and an
/// old X.509 v1 certificate that rustls can't even parse, so neither the
/// certificate nor the handshake signature made with it is checked (with any
/// certificate accepted, checking the signature would prove nothing).
fn any_certificate_config() -> Arc<rustls::ClientConfig> {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::crypto::CryptoProvider;
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, SignatureScheme};

    #[derive(Debug)]
    struct AnyCertificate(Arc<CryptoProvider>);

    impl ServerCertVerifier for AnyCertificate {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(&self, _message: &[u8], _cert: &CertificateDer<'_>, _dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(&self, _message: &[u8], _cert: &CertificateDer<'_>, _dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCertificate(provider)))
        .with_no_client_auth();
    Arc::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_parse_with_headers_and_chunks() {
        let raw = b"HTTP/1.1 200 OK\r\nX-AMN: ******0001\r\nSet-Cookie: session=abc; Path=/\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n<xml>\r\n6\r\n</xml>\r\n0\r\n\r\n";
        let response = Response::parse(raw).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.header("x-amn"), Some("******0001"));
        assert_eq!(response.header("set-cookie"), Some("session=abc; Path=/"));
        assert_eq!(response.body, "<xml></xml>");
        let plain = Response::parse(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 2\r\n\r\nno").unwrap();
        assert_eq!((plain.status, plain.body.as_str()), (403, "no"));
        assert!(Response::parse(b"garbage").is_err());
    }

    #[test]
    fn queries_are_form_encoded() {
        let query = encoded_query(&[("mac_address", "00:00:12:ab:cd:ef".into()), ("alias", "My PC".into()), ("IMSI", String::new())]);
        assert_eq!(query, "mac_address=00%3A00%3A12%3Aab%3Acd%3Aef&alias=My%20PC&IMSI=");
    }

    /// Finds a real router and reads its voice line (prints no number).
    #[test]
    #[ignore = "needs a JioFiber/AirFiber router on the network"]
    fn finds_a_router() {
        let (host, account) = discover().expect("a router");
        println!("router at {host}: mode {}, services {:?}", account.mode, account.services);
    }

    /// Only the TLS handshake with the credentials service (no request).
    #[test]
    #[ignore = "needs a JioFiber/AirFiber router on the network"]
    fn the_routers_tls_handshake_works() {
        let (host, _) = discover().expect("a router");
        let address = (host.as_str(), jiofiber::PROVISIONING_PORT).to_socket_addrs().unwrap().next().unwrap();
        let mut tcp = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).unwrap();
        let name = rustls::pki_types::ServerName::try_from(host.clone()).unwrap();
        let mut tls = rustls::ClientConnection::new(any_certificate_config(), name).unwrap();
        while tls.is_handshaking() {
            tls.complete_io(&mut tcp).expect("TLS handshake");
        }
        println!("TLS with {host}: {:?}, {:?}", tls.protocol_version(), tls.negotiated_cipher_suite().map(|s| s.suite()));
    }

    #[test]
    fn the_router_name_comes_first_and_nothing_repeats() {
        let hosts = candidates();
        assert_eq!(hosts.first().map(String::as_str), Some(jiofiber::ROUTER_NAME));
        assert!(hosts.iter().any(|h| h == "192.168.29.1") && hosts.iter().any(|h| h == "192.168.31.1"));
        let unique: std::collections::HashSet<_> = hosts.iter().collect();
        assert_eq!(unique.len(), hosts.len());
    }
}
