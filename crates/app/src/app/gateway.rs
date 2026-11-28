//! Settings > Gateway: starts and stops the optional `nocturne-gateway` helper and shows what it says (docs/dev/GATEWAY_PLAN.md sections 5 and 6).
//!
//! The helper is a separate process: the app spawns it as a child (next to the app binary, then `PATH`), talks to it over the control socket ([`nocturne_core::gateway`]) and stops it when it quits, unless "Keep running when NOCTURNE is closed" put it under a systemd user unit. Nothing runs and nothing polls while the gateway is off and the page is closed. The gateway never learns a server password and never gets a confirmation token from here.
use super::App;
use super::message::Message;
use crate::style;
use crate::task::run;
use crate::theme as t;
use crate::widget::{action_button, mono, rule};
use iced::widget::image::{FilterMethod, Handle};
use iced::widget::{column, container, image, row, space, text, text_input};
use iced::{Alignment, Element, Length, Task};
use nocturne_core::gateway::{self as gw, Request, Response, Status, TokenInfo};
use nocturne_core::settings as s;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

type El<'a> = Element<'a, Message>;

const BINARY: &str = "nocturne-gateway";
/// Pixels per QR module: whole pixels, so the code stays crisp.
const QR_SCALE: usize = 5;

#[derive(Clone)]
pub enum Msg {
    /// The status timer (only while the page is open or the gateway is on).
    Poll,
    Status(Result<Box<(Status, Vec<TokenInfo>)>, String>),
    /// Is a gateway already running? Answer to the probe sent when the switch went on.
    Probed(Result<Box<Status>, String>),
    /// The result of a request that has nothing to show but its failure.
    Done(Result<(), String>),
    Copy,
    CopyText(String),
    ShowPassword,
    HidePassword,
    Regenerate,
    Password(Result<String, String>),
    QcInput(String),
    Approve(String),
    Revoke(String),
    CertInput(String),
    KeyInput(String),
    ApplyTls,
    /// The systemd unit was written or removed: whether it is installed now.
    Unit(Result<bool, String>),
}

#[derive(Default)]
pub struct State {
    child: Option<Child>,
    status: Option<Status>,
    devices: Vec<TokenInfo>,
    /// Why the gateway is not running, in words for the page.
    note: Option<String>,
    password: Option<String>,
    pw_note: Option<String>,
    qc: String,
    cert: String,
    key: String,
    qr: Option<(String, Handle, u32)>,
    /// The settings the running gateway was last told about.
    applied: Option<gw::GatewaySettings>,
    polling: bool,
    /// The certificate inputs were filled from the saved paths for this visit to the page.
    seeded: bool,
}

/// A blocking request to the running gateway. Control sockets are unix sockets, so this is the one place that asks the platform (a Windows named pipe comes with that platform).
fn ask(req: &Request) -> Result<Response, String> {
    #[cfg(unix)]
    return gw::request(req).map_err(|_| "The gateway is not running.".to_string());
    #[cfg(not(unix))]
    {
        let _ = req;
        Err("The gateway is not available on this platform yet.".into())
    }
}

/// Run a blocking request off the UI thread.
async fn ask_async(req: Request) -> Result<Response, String> {
    tokio::task::spawn_blocking(move || ask(&req))
        .await
        .map_err(|e| e.to_string())?
}

fn ok_or(r: Result<Response, String>) -> Result<(), String> {
    match r? {
        Response::Err(e) => Err(e),
        _ => Ok(()),
    }
}

/// `nocturne-gateway` next to the app binary, then on `PATH`.
pub fn find_binary(exe_dir: Option<&Path>, path_var: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let beside = exe_dir.map(|d| d.join(BINARY));
    let on_path = path_var
        .into_iter()
        .flat_map(std::env::split_paths)
        .map(|d| d.join(BINARY));
    beside.into_iter().chain(on_path).find(|p| p.is_file())
}

fn locate() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok();
    find_binary(
        exe.as_deref().and_then(Path::parent),
        std::env::var_os("PATH").as_deref(),
    )
}

fn fmt_last_used(unix: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    match now.saturating_sub(unix) {
        0..=89 => "just now".into(),
        s @ 90..=5399 => format!("{} min ago", s / 60),
        s @ 5400..=129_599 => format!("{} h ago", s / 3600),
        s => format!("{} days ago", s / 86_400),
    }
}

/// A QR code as a grid of dark modules.
#[derive(Clone, Debug, PartialEq)]
pub struct Qr {
    width: usize,
    dark: Vec<bool>,
}

impl Qr {
    pub fn encode(payload: &str) -> Option<Self> {
        use qrcode::{Color as C, QrCode};
        let code = QrCode::new(payload.as_bytes()).ok()?;
        let width = code.width();
        Some(Self {
            width,
            dark: code.to_colors().into_iter().map(|c| c == C::Dark).collect(),
        })
    }
}

impl Qr {
    /// Pixels of the code, black on white with the four-module quiet zone scanners need (whatever the theme), `scale` pixels per module. Returns (side in pixels, RGBA bytes).
    pub fn rgba(&self, scale: usize) -> (u32, Vec<u8>) {
        let side = (self.width + 8) * scale;
        let mut px = vec![255u8; side * side * 4];
        for (i, d) in self.dark.iter().enumerate() {
            if !*d {
                continue;
            }
            let (mx, my) = ((i % self.width + 4) * scale, (i / self.width + 4) * scale);
            for y in my..my + scale {
                for x in mx..mx + scale {
                    px[(y * side + x) * 4..(y * side + x) * 4 + 3].fill(0);
                }
            }
        }
        (side as u32, px)
    }
}

impl App {
    /// Whether the status timer should run, and how fast.
    pub(super) fn gateway_poll_period(&self) -> Option<Duration> {
        if self.settings_open {
            Some(Duration::from_secs(1))
        } else if self.settings.gateway.enabled || self.gw.child.is_some() {
            Some(Duration::from_secs(10))
        } else {
            None
        }
    }

    /// The ServerId of our own gateway as last reported; a server whose id is this one is the gateway itself and is refused as a source.
    pub(super) fn gateway_owns(&self, server_id: &str) -> bool {
        !server_id.is_empty()
            && self
                .gw
                .status
                .as_ref()
                .is_some_and(|s| s.server_id == server_id)
    }

    /// Start with the app: bring the gateway up when the switch is on.
    pub(super) fn gateway_start(&mut self) -> Task<Message> {
        if self.settings.gateway.enabled {
            return self.gateway_probe();
        }
        // Not asked to run, but one may be running from a systemd unit: nothing to do until the page is opened.
        Task::none()
    }

    fn gateway_probe(&mut self) -> Task<Message> {
        self.gw.applied = Some(self.settings.gateway.clone());
        run(
            async {
                match ask_async(Request::Status).await? {
                    Response::Status(s) => Ok(s),
                    _ => Err("unexpected answer".to_string()),
                }
            },
            |r| Message::Gateway(Msg::Probed(r)),
        )
    }

    /// Settings changed: bring the helper in line (start, stop, reload, unit). Cheap when no gateway setting moved.
    pub(super) fn gateway_apply(&mut self) -> Task<Message> {
        let now = self.settings.gateway.clone();
        let before = self.gw.applied.clone();
        if before.as_ref() == Some(&now) {
            return Task::none();
        }
        let was_on = before.as_ref().is_some_and(|b| b.enabled);
        let keep_changed = before.as_ref().map(|b| b.keep_running) != Some(now.keep_running);
        let mut tasks = vec![];
        if keep_changed && (now.keep_running || before.is_some()) {
            tasks.push(self.gateway_unit(now.keep_running && now.enabled));
        }
        if now.enabled {
            tasks.push(self.gateway_probe());
        } else {
            self.gw.applied = Some(now.clone());
            if was_on || self.gw.child.is_some() || self.gw.status.is_some() {
                tasks.push(self.gateway_stop());
            }
            if now.keep_running && !keep_changed {
                // Switched off while kept running: the unit goes too.
                tasks.push(self.gateway_unit(false));
            }
        }
        Task::batch(tasks)
    }

    fn gateway_stop(&mut self) -> Task<Message> {
        self.gw.status = None;
        self.gw.devices.clear();
        self.gw.note = None;
        self.gw.password = None;
        // The child is reaped on the next poll; the request makes it leave cleanly.
        run(ask_async(Request::Stop), |r| {
            Message::Gateway(Msg::Done(ok_or(r).or(Ok(()))))
        })
    }

    /// Write or remove the systemd user unit (stopping our own child first so the unit's process can take the instance lock).
    fn gateway_unit(&mut self, on: bool) -> Task<Message> {
        if !s::keep_running_supported() {
            return Task::none();
        }
        let exe = locate();
        if on {
            let _ = self.gw.child.take();
            self.gw.status = None;
        }
        run(
            async move {
                tokio::task::spawn_blocking(move || {
                    if on {
                        let exe = exe.ok_or_else(|| {
                            "nocturne-gateway was not found next to NOCTURNE or on PATH".to_string()
                        })?;
                        // Let our own child leave before the service starts.
                        let _ = ask(&Request::Stop);
                        std::thread::sleep(Duration::from_millis(700));
                        nocturne_core::user_unit::enable(
                            &nocturne_core::user_unit::GATEWAY,
                            &exe,
                            &[],
                        )
                        .map_err(|e| e.to_string())?;
                    } else {
                        nocturne_core::user_unit::disable(&nocturne_core::user_unit::GATEWAY)
                            .map_err(|e| e.to_string())?;
                    }
                    Ok(nocturne_core::user_unit::installed(
                        &nocturne_core::user_unit::GATEWAY,
                    ))
                })
                .await
                .map_err(|e| e.to_string())?
            },
            |r| Message::Gateway(Msg::Unit(r)),
        )
    }

    /// The app is quitting: stop the child we started (unless the service keeps the gateway running).
    pub(super) fn gateway_shutdown(&mut self) {
        let Some(mut child) = self.gw.child.take() else {
            return;
        };
        if self.settings.gateway.keep_running {
            return;
        }
        let _ = ask(&Request::Stop);
        for _ in 0..20 {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
    }

    fn gateway_spawn(&mut self) {
        if let Err(e) = self.settings.gateway.validate() {
            self.gw.note = Some(e);
            return;
        }
        let Some(bin) = locate() else {
            self.gw.note = Some(format!(
                "nocturne-gateway was not found next to NOCTURNE or on PATH. {}",
                "Install the gateway package or build it with cargo build -p nocturne-gateway."
            ));
            return;
        };
        match Command::new(bin)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
        {
            Ok(c) => {
                self.gw.child = Some(c);
                self.gw.note = None;
            }
            Err(e) => self.gw.note = Some(format!("Could not start the gateway: {e}")),
        }
    }

    fn gateway_poll(&mut self) -> Task<Message> {
        if let Some(c) = self.gw.child.as_mut()
            && let Ok(Some(code)) = c.try_wait()
        {
            self.gw.child = None;
            self.gw.status = None;
            if self.settings.gateway.enabled {
                self.gw.note = Some(match code.code() {
                    Some(c) => format!(
                        "The gateway stopped (exit code {c}). Another gateway may already hold the port."
                    ),
                    None => "The gateway stopped.".into(),
                });
            }
        }
        if self.settings_open {
            if !self.gw.seeded {
                self.gw.seeded = true;
                let show = |p: &Option<PathBuf>| {
                    p.as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                };
                self.gw.cert = show(&self.settings.gateway.cert_path);
                self.gw.key = show(&self.settings.gateway.key_path);
            }
        } else {
            self.gw.seeded = false;
            self.gw.password = None;
        }
        if self.gw.polling {
            return Task::none();
        }
        self.gw.polling = true;
        let with_devices = self.settings_open;
        run(
            async move {
                let Response::Status(s) = ask_async(Request::Status).await? else {
                    return Err("unexpected answer".to_string());
                };
                let devices = if with_devices {
                    match ask_async(Request::Tokens).await {
                        Ok(Response::Tokens(t)) => t,
                        _ => vec![],
                    }
                } else {
                    vec![]
                };
                Ok(Box::new((*s, devices)))
            },
            |r| Message::Gateway(Msg::Status(r)),
        )
    }

    pub(super) fn gateway_msg(&mut self, m: Msg) -> Task<Message> {
        match m {
            Msg::Poll => return self.gateway_poll(),
            Msg::Status(r) => {
                self.gw.polling = false;
                match r {
                    Ok(b) => {
                        let (st, devices) = *b;
                        if self.settings_open {
                            self.gw.devices = devices;
                        }
                        self.gw_set_status(st);
                    }
                    Err(_) => {
                        self.gw.status = None;
                        self.gw.devices.clear();
                    }
                }
            }
            Msg::Probed(r) => match r {
                // Already running (a service, or a previous app run): tell it what changed.
                Ok(st) => {
                    self.gw_set_status(*st);
                    self.gw.note = None;
                    return run(ask_async(Request::Reload), |r| {
                        Message::Gateway(Msg::Done(ok_or(r)))
                    });
                }
                Err(_) => {
                    if self.settings.gateway.enabled && self.gw.child.is_none() {
                        self.gateway_spawn();
                    }
                }
            },
            Msg::Done(r) => {
                if let Err(e) = r {
                    self.toast(e);
                }
            }
            Msg::Copy => {
                if let Some(addr) = self.gw.status.as_ref().map(|s| s.address.clone()) {
                    self.toast("Address copied.");
                    return iced::clipboard::write(addr);
                }
            }
            Msg::CopyText(v) => {
                self.toast("Copied.");
                return iced::clipboard::write(v);
            }
            Msg::ShowPassword => {
                self.gw.pw_note = None;
                return run(ask_async(Request::RevealPassword), |r| {
                    Message::Gateway(Msg::Password(match r {
                        Ok(Response::Password(p)) => Ok(p),
                        Ok(Response::Err(e)) | Err(e) => Err(e),
                        Ok(_) => Err("unexpected answer".into()),
                    }))
                });
            }
            Msg::HidePassword => {
                self.gw.password = None;
                self.gw.pw_note = None;
            }
            Msg::Regenerate => {
                return run(ask_async(Request::RegeneratePassword), |r| {
                    Message::Gateway(Msg::Password(match r {
                        Ok(Response::Password(p)) => Ok(p),
                        Ok(Response::Err(e)) | Err(e) => Err(e),
                        Ok(_) => Err("unexpected answer".into()),
                    }))
                });
            }
            Msg::Password(r) => match r {
                Ok(p) => {
                    self.gw.password = Some(p);
                    self.gw.pw_note = None;
                }
                Err(e) => {
                    self.gw.password = None;
                    self.gw.pw_note = Some(e);
                }
            },
            Msg::QcInput(v) => {
                self.gw.qc = v.chars().filter(char::is_ascii_digit).take(12).collect();
            }
            Msg::Approve(code) => {
                let code = if code.is_empty() {
                    self.gw.qc.clone()
                } else {
                    code
                };
                if code.is_empty() {
                    return Task::none();
                }
                self.gw.qc.clear();
                return run(ask_async(Request::ApproveQuickConnect(code)), |r| {
                    Message::Gateway(Msg::Done(ok_or(r)))
                });
            }
            Msg::Revoke(id) => {
                self.gw.devices.retain(|d| d.id != id);
                return run(ask_async(Request::Revoke(id)), |r| {
                    Message::Gateway(Msg::Done(ok_or(r)))
                });
            }
            Msg::CertInput(v) => self.gw.cert = v,
            Msg::KeyInput(v) => self.gw.key = v,
            Msg::ApplyTls => {
                let p = |v: &str| (!v.trim().is_empty()).then(|| PathBuf::from(v.trim()));
                self.settings.gateway.cert_path = p(&self.gw.cert);
                self.settings.gateway.key_path = p(&self.gw.key);
                return self.settings_saved_gateway();
            }
            Msg::Unit(r) => match r {
                Ok(on) => self.settings.gateway.keep_running = on,
                Err(e) => {
                    self.settings.gateway.keep_running =
                        nocturne_core::user_unit::installed(&nocturne_core::user_unit::GATEWAY);
                    self.toast(format!("Keep running: {e}"));
                }
            },
        }
        Task::none()
    }

    fn gw_set_status(&mut self, st: Status) {
        let addr = gw::qr_payload(&st.address);
        if self.gw.qr.as_ref().is_none_or(|(a, ..)| *a != addr)
            && let Some(q) = Qr::encode(&addr)
        {
            let (side, px) = q.rgba(QR_SCALE);
            self.gw.qr = Some((addr, Handle::from_rgba(side, side, px), side));
        }
        self.gw.status = Some(st);
    }

    /// Persist a gateway setting changed outside the rows and pass it on.
    fn settings_saved_gateway(&mut self) -> Task<Message> {
        self.settings.save();
        self.gw.applied = None;
        self.gateway_apply()
    }

    // ---------------------------------------------------------------- view

    fn gw_card<'a>(&self, title: &str, body: impl Into<El<'a>>) -> El<'a> {
        container(
            column![
                text(title.to_string()).size(t::BODY).font(t::semibold()),
                body.into(),
            ]
            .spacing(t::S3),
        )
        .padding(t::S4)
        .width(Length::Fill)
        .style(style::panel)
        .into()
    }

    /// Everything of the Gateway page that is not a plain row: status, address with QR, sign-in, devices, certificate paths, sources.
    pub(super) fn gateway_panel(&self) -> El<'_> {
        let g = &self.settings.gateway;
        let mut col = column![].spacing(t::S4).max_width(900);
        let small = |s: String| text(s).size(t::CONTROL_TEXT).color(t::text_secondary());

        // Status
        let (dot, line) = match (&self.gw.status, &self.gw.note, g.enabled) {
            (Some(st), _, _) => match &st.error {
                Some(e) => (t::danger(), format!("Not listening: {e}")),
                None => (t::accent(), format!("Online at {}", st.address)),
            },
            (None, Some(n), _) => (t::danger(), n.clone()),
            (None, None, true) => (t::text_secondary(), "Starting".to_string()),
            (None, None, false) => (
                t::text_secondary(),
                "Stopped (the switch above is off)".to_string(),
            ),
        };
        col = col.push(
            self.gw_card(
                "Status",
                row![
                    text("\u{25CF}").size(t::CONTROL_TEXT).color(dot),
                    text(line).size(t::BODY).width(Length::Fill),
                ]
                .spacing(t::S2),
            ),
        );

        let Some(st) = &self.gw.status else {
            return self.gw_certs(col, g);
        };

        // Address and QR
        let mut addr = column![
            row![
                mono(&st.address),
                space().width(Length::Fill),
                action_button("Copy", false, false, false, false, Message::Gateway(Msg::Copy)),
            ]
            .align_y(Alignment::Center),
            small(match st.binding {
                gw::Binding::Local => "Only this computer can connect.".to_string(),
                gw::Binding::Lan => "Plain HTTP, and only devices on your own network are served.".to_string(),
                gw::Binding::Remote => "HTTPS with your certificate. Prefer a private overlay such as Tailscale over opening a router port.".to_string(),
            }),
        ]
        .spacing(t::S2);
        if let Some((_, h, side)) = &self.gw.qr {
            addr = addr.push(
                image(h.clone())
                    .filter_method(FilterMethod::Nearest)
                    .width(*side as f32)
                    .height(*side as f32),
            );
        }
        col = col.push(self.gw_card("Address", addr));

        // Sign-in
        let mut sign = column![
            small(format!(
                "User name: {}. Apps sign in with this and the generated password, or with Quick Connect. A server password is never asked for or stored here.",
                st.user_name
            )),
        ]
        .spacing(t::S2);
        if let Some(p) = &self.gw.password {
            sign = sign.push(
                row![
                    mono(p),
                    space().width(Length::Fill),
                    action_button(
                        "Copy",
                        false,
                        false,
                        false,
                        false,
                        Message::Gateway(Msg::CopyText(p.clone()))
                    ),
                    action_button(
                        "Hide",
                        false,
                        false,
                        false,
                        false,
                        Message::Gateway(Msg::HidePassword)
                    ),
                ]
                .spacing(t::S2)
                .align_y(Alignment::Center),
            );
        }
        if let Some(n) = &self.gw.pw_note {
            sign = sign.push(small(n.clone()));
        }
        let mut pw_row = row![].spacing(t::S2);
        if st.password_unseen {
            pw_row = pw_row.push(action_button(
                "Show password",
                false,
                false,
                false,
                false,
                Message::Gateway(Msg::ShowPassword),
            ));
        }
        sign = sign.push(pw_row.push(action_button(
            "Regenerate password",
            false,
            false,
            false,
            false,
            Message::Gateway(Msg::Regenerate),
        )));
        sign = sign.push(small(
            if st.password_unseen {
                "The password can be shown once. Regenerating makes a new one and signs every device out.".into()
            } else {
                "The password was shown once already. Regenerating makes a new one and signs every device out.".into()
            },
        ));
        sign = sign.push(rule());
        sign = sign.push(
            row![
                text_input("Quick Connect code", &self.gw.qc)
                    .on_input(|v| Message::Gateway(Msg::QcInput(v)))
                    .on_submit(Message::Gateway(Msg::Approve(String::new())))
                    .size(t::BODY)
                    .padding([t::S2, t::S3])
                    .style(style::input)
                    .width(220),
                action_button(
                    "Approve code",
                    true,
                    false,
                    false,
                    self.gw.qc.is_empty(),
                    Message::Gateway(Msg::Approve(String::new()))
                ),
            ]
            .spacing(t::S2)
            .align_y(Alignment::Center),
        );
        for c in &st.pending_codes {
            sign = sign.push(
                row![
                    mono(&c.code),
                    text(format!("from {}", c.device))
                        .size(t::CONTROL_TEXT)
                        .color(t::text_secondary()),
                    space().width(Length::Fill),
                    action_button(
                        "Approve",
                        true,
                        false,
                        false,
                        false,
                        Message::Gateway(Msg::Approve(c.code.clone()))
                    ),
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center),
            );
        }
        col = col.push(self.gw_card("Sign-in", sign));

        // Signed-in devices
        let mut devs = column![].spacing(t::S2);
        if self.gw.devices.is_empty() {
            devs = devs.push(small(if st.devices == 0 {
                "No device is signed in.".into()
            } else {
                "Loading".into()
            }));
        }
        for d in &self.gw.devices {
            devs = devs.push(
                row![
                    column![
                        text(format!("{} on {}", d.client, d.device_name)).size(t::BODY),
                        small(format!("last used {}", fmt_last_used(d.last_used))),
                    ]
                    .width(Length::Fill),
                    action_button(
                        "Revoke",
                        false,
                        true,
                        false,
                        false,
                        Message::Gateway(Msg::Revoke(d.id.clone()))
                    ),
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center),
            );
        }
        col = col.push(self.gw_card("Signed-in devices", devs));

        // Sources
        if !st.sources.is_empty() {
            let mut src = column![].spacing(t::S1);
            for x in &st.sources {
                src = src.push(small(format!(
                    "{}: {}",
                    x.label,
                    match x.state.as_str() {
                        "ready" => "ready",
                        "local" => "local folders",
                        "signed_out" => "signed out (sign in again in Sources)",
                        "keyring_locked" => "keyring locked",
                        o => o,
                    }
                )));
            }
            col = col.push(self.gw_card("Sources behind the gateway", src));
        }
        self.gw_certs(col, g)
    }

    /// Certificate and key paths: only for the HTTPS mode.
    fn gw_certs<'a>(
        &self,
        col: iced::widget::Column<'a, Message>,
        g: &gw::GatewaySettings,
    ) -> El<'a> {
        let mut col = col;
        if g.binding == gw::Binding::Remote {
            let path = |ph: &str, v: &str, f: fn(String) -> Msg| {
                text_input(ph, v)
                    .on_input(move |x| Message::Gateway(f(x)))
                    .on_submit(Message::Gateway(Msg::ApplyTls))
                    .size(t::BODY)
                    .padding([t::S2, t::S3])
                    .style(style::input)
            };
            col = col.push(self.gw_card(
                "HTTPS certificate and key",
                column![
                    text("PEM files of your own certificate and private key. NOCTURNE does not create or renew certificates.")
                        .size(t::CONTROL_TEXT)
                        .color(t::text_secondary()),
                    path("Certificate file (.pem)", &self.gw.cert, Msg::CertInput),
                    path("Private key file (.pem)", &self.gw.key, Msg::KeyInput),
                    action_button("Apply", true, false, false, false, Message::Gateway(Msg::ApplyTls)),
                ]
                .spacing(t::S2),
            ));
        }
        container(col)
            .padding([t::S4, t::page_pad() + t::S6])
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_is_found_beside_the_app_first_then_on_path() {
        let d = tempfile::tempdir().unwrap();
        let (beside, other) = (d.path().join("app"), d.path().join("bin"));
        std::fs::create_dir_all(&beside).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let path = std::env::join_paths([d.path().join("nowhere"), other.clone()]).unwrap();
        assert_eq!(find_binary(Some(&beside), Some(&path)), None);
        std::fs::write(other.join(BINARY), "x").unwrap();
        assert_eq!(
            find_binary(Some(&beside), Some(&path)),
            Some(other.join(BINARY))
        );
        std::fs::write(beside.join(BINARY), "x").unwrap();
        assert_eq!(
            find_binary(Some(&beside), Some(&path)),
            Some(beside.join(BINARY))
        );
        assert_eq!(find_binary(None, None), None);
    }

    #[test]
    fn qr_encodes_the_address_only() {
        let q = Qr::encode("http://192.168.1.5:8099").unwrap();
        assert_eq!(q.dark.len(), q.width * q.width);
        assert!(q.dark.iter().any(|d| *d) && q.dark.iter().any(|d| !*d));
        // The three finder patterns: the top-left corner module is always dark.
        assert!(q.dark[0]);
        assert_eq!(Qr::encode("http://192.168.1.5:8099"), Some(q.clone()));
        // Pixels: a white quiet zone around, the first dark module at (4, 4) in modules.
        let (side, px) = q.rgba(2);
        assert_eq!(side as usize, (q.width + 8) * 2);
        assert_eq!(px.len(), (side * side * 4) as usize);
        let at = |x: usize, y: usize| px[(y * side as usize + x) * 4];
        assert_eq!((at(0, 0), at(7, 7), at(8, 8), at(9, 9)), (255, 255, 0, 0));
    }

    #[test]
    fn last_used_reads_naturally() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(fmt_last_used(now), "just now");
        assert_eq!(fmt_last_used(now - 600), "10 min ago");
        assert_eq!(fmt_last_used(now - 7200), "2 h ago");
        assert_eq!(fmt_last_used(now - 3 * 86_400), "3 days ago");
    }
}
