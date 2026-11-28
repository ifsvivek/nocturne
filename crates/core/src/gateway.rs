//! The gateway as the app sees it: its settings, the peer rule of each binding mode, the control-socket
//! protocol and the address to show. Pure and dependency-light on purpose: the app must never link
//! the gateway crate (axum, argon2). docs/dev/GATEWAY_PLAN.md sections 5 and 6.
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};

/// The one Jellyfin version string the gateway advertises (plan decision 1); the 12.4.0 OpenAPI document stays the shape reference.
pub const ADVERTISED_VERSION: &str = "10.11.6";
pub const DEFAULT_PORT: u16 = 8099;
/// UDP port the Jellyfin phone and TV apps broadcast to.
pub const DISCOVERY_PORT: u16 = 7359;

/// Who may connect (Settings > Gateway > Allow connections from).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Binding {
    /// 127.0.0.1 and ::1 only.
    #[default]
    Local,
    /// All interfaces, but only loopback and private-range peers are served (cleartext HTTP).
    Lan,
    /// All interfaces, any peer, HTTPS with the user's own certificate required.
    Remote,
}

impl Binding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "This computer only",
            Self::Lan => "Local network",
            Self::Remote => "Remote over HTTPS",
        }
    }
}

/// The `[gateway]` table of `settings.toml`. The app's `Settings` embeds this struct as its `gateway` field; the gateway process reads the same table with [`GatewaySettings::load_from`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewaySettings {
    pub enabled: bool,
    pub binding: Binding,
    pub port: u16,
    /// PEM files; only used (and required) for [`Binding::Remote`].
    pub cert_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>,
    /// Allow the session and command routes (`gateway_remote_control` in the plan, default off).
    pub remote_control: bool,
    /// Per-server library views next to the unified ones.
    pub per_server_views: bool,
    /// Answer the apps' UDP discovery (LAN mode only).
    pub announce: bool,
    /// Image cache cap in MiB, 0 = none.
    pub cache_mb: u32,
    /// Keep the gateway running when the app closes (systemd user unit, Linux).
    pub keep_running: bool,
    /// `X-Forwarded-For` is believed only from this peer (a reverse proxy in front of Remote mode).
    pub trusted_proxy: Option<IpAddr>,
}

impl Default for GatewaySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            binding: Binding::Local,
            port: DEFAULT_PORT,
            cert_path: None,
            key_path: None,
            remote_control: false,
            per_server_views: false,
            announce: true,
            cache_mb: 0,
            keep_running: false,
            trusted_proxy: None,
        }
    }
}

impl GatewaySettings {
    /// The `[gateway]` table of the file at `path`; a missing file, table or field gives the defaults.
    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| toml::from_str::<toml::Table>(&t).ok())
            .and_then(|mut t| t.remove("gateway"))
            .and_then(|v| v.try_into().ok())
            .unwrap_or_default()
    }

    /// Why the gateway must refuse to start with these settings, if it must.
    pub fn validate(&self) -> Result<(), String> {
        if self.binding == Binding::Remote && (self.cert_path.is_none() || self.key_path.is_none())
        {
            return Err("Remote over HTTPS needs a certificate and a key file".into());
        }
        Ok(())
    }

    pub fn tls(&self) -> bool {
        self.binding == Binding::Remote
    }
}

/// Unwrap `::ffff:a.b.c.d` so one rule covers both families.
pub fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

/// Loopback, RFC 1918, link-local, CGNAT `100.64.0.0/10` (Tailscale), IPv6 unique-local `fc00::/7` and `fe80::/10`.
pub fn is_private_peer(ip: IpAddr) -> bool {
    match normalize(ip) {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || (o[0] == 100 && (o[1] & 0xc0) == 64)
        }
        IpAddr::V6(v6) => {
            let s = v6.segments()[0];
            v6.is_loopback() || (s & 0xfe00) == 0xfc00 || (s & 0xffc0) == 0xfe80
        }
    }
}

/// Whether a peer may be served at all in this mode (checked at accept time, before any bytes are read).
pub fn peer_allowed(binding: Binding, ip: IpAddr) -> bool {
    match binding {
        Binding::Local => normalize(ip).is_loopback(),
        Binding::Lan => is_private_peer(ip),
        Binding::Remote => true,
    }
}

/// The addresses to bind for a mode: loopback only for `Local`, every interface otherwise. IPv6 is best effort (the caller ignores a failed bind of the second family).
pub fn bind_addrs(binding: Binding) -> [IpAddr; 2] {
    match binding {
        Binding::Local => [Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into()],
        _ => [Ipv4Addr::UNSPECIFIED.into(), Ipv6Addr::UNSPECIFIED.into()],
    }
}

/// The URL clients are told to use. `host` is this machine's address on the network (ignored for `Local`).
pub fn address_url(binding: Binding, host: &str, port: u16) -> String {
    let (scheme, default) = if binding == Binding::Remote {
        ("https", 443)
    } else {
        ("http", 80)
    };
    let host = if binding == Binding::Local {
        "127.0.0.1".to_string()
    } else if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    if port == default {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}:{port}")
    }
}

/// What a QR code on the Settings page encodes: the address only, never a credential.
pub fn qr_payload(url: &str) -> String {
    url.trim().to_string()
}

/// One signed-in client device (a gateway token; the token itself is never shown).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenInfo {
    pub id: String,
    pub client: String,
    pub device_name: String,
    pub device_id: String,
    /// Unix seconds.
    pub created: u64,
    pub last_used: u64,
    /// A named API key (created through the control socket) rather than a device sign-in; `device_name` holds its name.
    #[serde(default)]
    pub api_key: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceStatus {
    pub label: String,
    /// `ready`, `signed_out`, `keyring_locked`, `local`.
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingCode {
    pub code: String,
    pub device: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub version: String,
    /// The gateway's own Jellyfin `ServerId`: the app refuses a source with this id (it would loop back into the gateway).
    #[serde(default)]
    pub server_id: String,
    pub binding: Binding,
    pub tls: bool,
    /// Listening socket addresses, `ip:port`.
    pub listening: Vec<String>,
    /// The URL to give to clients.
    pub address: String,
    pub user_name: String,
    pub devices: usize,
    pub pending_codes: Vec<PendingCode>,
    pub sources: Vec<SourceStatus>,
    /// The first-start passphrase has not been fetched yet ([`Request::RevealPassword`]).
    pub password_unseen: bool,
    /// Why the listeners are down (port taken, bad certificate), if they are.
    pub error: Option<String>,
    pub uptime_secs: u64,
}

/// One request per line, JSON, on the control socket.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "arg", rename_all = "snake_case")]
pub enum Request {
    Status,
    /// Re-read settings and sources.
    Reload,
    Stop,
    Tokens,
    Revoke(String),
    /// Approve the Quick Connect code a client shows.
    ApproveQuickConnect(String),
    /// New passphrase; every token is revoked.
    RegeneratePassword,
    /// The passphrase generated at first start, once.
    RevealPassword,
    /// A named long-lived API key for a client that signs in with a key instead of the password; answered with [`Response::Key`] (shown once). Revoked with `Revoke` like a device; survives a password regeneration, as API keys do on a server.
    CreateApiKey(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Response {
    Status(Box<Status>),
    Tokens(Vec<TokenInfo>),
    Password(String),
    /// A new API key, shown once.
    Key(String),
    Ok,
    Err(String),
}

/// Longest line either side accepts.
pub const MAX_LINE: usize = 64 * 1024;

/// `$XDG_RUNTIME_DIR/nocturne/gateway.sock` (None where there is no runtime dir; Windows will use a named pipe).
pub fn socket_path() -> Option<PathBuf> {
    nocturne_session::paths::runtime_dir().map(|d| d.join("nocturne").join("gateway.sock"))
}

/// Ask the running gateway. Blocking, short timeouts: call it from a worker, not the UI thread. An error means no gateway is running (or it is not ours).
#[cfg(unix)]
pub fn request(req: &Request) -> std::io::Result<Response> {
    let path = socket_path().ok_or_else(|| std::io::Error::other("no runtime directory"))?;
    request_at(&path, req, std::time::Duration::from_secs(3))
}

/// [`request`] against an explicit socket path (tests).
#[cfg(unix)]
pub fn request_at(
    path: &Path,
    req: &Request,
    timeout: std::time::Duration,
) -> std::io::Result<Response> {
    use std::io::{BufRead, Read, Write};
    let mut s = std::os::unix::net::UnixStream::connect(path)?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    let mut line = serde_json::to_vec(req).map_err(std::io::Error::other)?;
    line.push(b'\n');
    s.write_all(&line)?;
    let mut out = Vec::new();
    std::io::BufReader::new(s.take(MAX_LINE as u64)).read_until(b'\n', &mut out)?;
    serde_json::from_slice(&out).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn defaults_are_off_local_8099() {
        let s = GatewaySettings::default();
        assert!(!s.enabled && !s.remote_control && s.announce);
        assert_eq!((s.binding, s.port), (Binding::Local, 8099));
        assert!(s.validate().is_ok());
    }

    #[test]
    fn remote_needs_certificate_and_key() {
        let mut s = GatewaySettings {
            binding: Binding::Remote,
            ..Default::default()
        };
        assert!(s.validate().is_err());
        s.cert_path = Some("c.pem".into());
        assert!(s.validate().is_err());
        s.key_path = Some("k.pem".into());
        assert!(s.validate().is_ok() && s.tls());
    }

    #[test]
    fn load_reads_only_the_gateway_table() {
        let d = std::env::temp_dir().join(format!("nocturne-gw-core-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("settings.toml");
        std::fs::write(
            &p,
            "version = 2\nallow_server_writes = true\n[gateway]\nenabled = true\nbinding = \"lan\"\nport = 9000\n",
        )
        .unwrap();
        let s = GatewaySettings::load_from(&p);
        assert!(s.enabled && s.binding == Binding::Lan && s.port == 9000 && s.announce);
        assert_eq!(
            GatewaySettings::load_from(&d.join("none.toml")),
            GatewaySettings::default()
        );
        std::fs::write(&p, "[gateway]\nbinding = \"nonsense\"\n").unwrap();
        assert_eq!(GatewaySettings::load_from(&p), GatewaySettings::default());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn peer_rules_per_mode() {
        let lan = [
            "192.168.1.5",
            "10.0.0.2",
            "172.16.4.4",
            "169.254.1.1",
            "100.64.0.9",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.0.1",
        ];
        let wan = [
            "8.8.8.8",
            "100.128.0.1",
            "172.32.0.1",
            "2001:db8::1",
            "::ffff:8.8.8.8",
        ];
        for a in ["127.0.0.1", "::1", "::ffff:127.0.0.1"] {
            assert!(peer_allowed(Binding::Local, ip(a)), "{a}");
            assert!(peer_allowed(Binding::Lan, ip(a)), "{a}");
        }
        for a in lan {
            assert!(!peer_allowed(Binding::Local, ip(a)), "{a} local");
            assert!(peer_allowed(Binding::Lan, ip(a)), "{a} lan");
        }
        for a in wan {
            assert!(!peer_allowed(Binding::Local, ip(a)), "{a} local");
            assert!(!peer_allowed(Binding::Lan, ip(a)), "{a} lan");
            assert!(peer_allowed(Binding::Remote, ip(a)), "{a} remote");
        }
    }

    #[test]
    fn address_and_qr() {
        assert_eq!(
            address_url(Binding::Local, "ignored", 8099),
            "http://127.0.0.1:8099"
        );
        assert_eq!(
            address_url(Binding::Lan, "192.168.1.5", 8099),
            "http://192.168.1.5:8099"
        );
        assert_eq!(
            address_url(Binding::Lan, "fd00::1", 8099),
            "http://[fd00::1]:8099"
        );
        assert_eq!(
            address_url(Binding::Remote, "nas.example", 443),
            "https://nas.example"
        );
        assert_eq!(
            address_url(Binding::Remote, "nas.example", 8443),
            "https://nas.example:8443"
        );
        assert_eq!(qr_payload(" http://x:1 \n"), "http://x:1");
    }

    #[test]
    fn protocol_round_trips() {
        for r in [
            Request::Status,
            Request::Reload,
            Request::Stop,
            Request::Tokens,
            Request::Revoke("ab".into()),
            Request::ApproveQuickConnect("123456".into()),
            Request::RegeneratePassword,
            Request::RevealPassword,
            Request::CreateApiKey("tv".into()),
        ] {
            let j = serde_json::to_string(&r).unwrap();
            assert!(!j.contains('\n'));
            assert_eq!(serde_json::from_str::<Request>(&j).unwrap(), r);
        }
        assert_eq!(
            serde_json::to_string(&Request::Revoke("ab".into())).unwrap(),
            r#"{"cmd":"revoke","arg":"ab"}"#
        );
        for r in [
            Response::Ok,
            Response::Err("no".into()),
            Response::Password("p".into()),
            Response::Key("k".into()),
            Response::Tokens(vec![TokenInfo {
                id: "i".into(),
                client: "c".into(),
                device_name: "d".into(),
                device_id: "x".into(),
                created: 1,
                last_used: 2,
                api_key: true,
            }]),
        ] {
            let j = serde_json::to_string(&r).unwrap();
            assert_eq!(serde_json::from_str::<Response>(&j).unwrap(), r);
        }
        assert!(serde_json::from_str::<Request>(r#"{"cmd":"rm_rf"}"#).is_err());
    }
}
