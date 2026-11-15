//! Turns loaded data into rows and buttons for the list screens (Users, Libraries, Tasks,
//! and so on). The dashboard's session cards use the same rows.
use super::Admin;
use super::act::{Act, Cfg};
use super::fmt::{bytes, short, when};
use super::state::{Data, L, Section};

pub struct Btn {
    pub label: &'static str,
    pub act: Act,
    pub danger: bool,
    pub primary: bool,
}

fn btn(label: &'static str, act: Act, danger: bool, primary: bool) -> Btn {
    Btn {
        label,
        act,
        danger,
        primary,
    }
}
fn b(label: &'static str, act: Act) -> Btn {
    btn(label, act, false, false)
}
fn bd(label: &'static str, act: Act) -> Btn {
    btn(label, act, true, false)
}
fn bp(label: &'static str, act: Act) -> Btn {
    btn(label, act, false, true)
}

#[derive(Default)]
pub struct Item {
    pub group: Option<String>,
    pub title: String,
    pub sub: String,
    pub chips: Vec<(String, bool)>,
    pub avatar: bool,
    pub progress: Option<f32>,
    pub detail: Vec<(&'static str, String)>,
    /// Mono text block in the detail panel (log tail).
    pub text: Option<String>,
    pub acts: Vec<Btn>,
}

#[derive(Default)]
pub struct Model {
    pub toolbar: Vec<Btn>,
    pub items: Vec<Item>,
    pub status: Option<&'static str>,
}

impl Model {
    /// Index of item `i` among the list's children (group headers count as children).
    pub fn child_ix(&self, i: usize) -> usize {
        (0..=i.min(self.items.len().saturating_sub(1)))
            .filter(|&k| self.group_start(k))
            .count()
            + i
    }

    pub fn group_start(&self, k: usize) -> bool {
        self.items[k].group.is_some() && (k == 0 || self.items[k].group != self.items[k - 1].group)
    }
}

fn status<T>(l: &L<T>) -> Option<&'static str> {
    match l {
        L::Loading => Some("Loading"),
        L::Failed => Some("This couldn't be loaded. Press R to try again."),
        L::Ready(_) => None,
    }
}

fn yes(v: bool) -> String {
    if v { "on" } else { "off" }.into()
}

impl Admin {
    pub(super) fn model(&self) -> Model {
        let d = &self.st.data;
        match self.st.section {
            Section::Dashboard => dashboard(d),
            Section::General => general(d),
            Section::Branding => branding(d),
            Section::Users => users(d),
            Section::Libraries => libraries(d),
            Section::Metadata => config_page(d, |c| {
                vec![
                    text_row(
                        "Metadata language",
                        c.preferred_metadata_language.clone(),
                        Cfg::MetaLang,
                    ),
                    text_row(
                        "Metadata country",
                        c.metadata_country_code.clone(),
                        Cfg::MetaCountry,
                    ),
                ]
            }),
            Section::Resume => config_page(d, |c| {
                vec![
                    num_row(
                        "Minimum resume position",
                        c.min_resume_pct,
                        "%",
                        Cfg::ResumeMin,
                    ),
                    num_row(
                        "Maximum resume position",
                        c.max_resume_pct,
                        "%",
                        Cfg::ResumeMax,
                    ),
                    num_row(
                        "Minimum resume duration",
                        c.min_resume_duration_seconds,
                        " s",
                        Cfg::ResumeSecs,
                    ),
                ]
            }),
            Section::Streaming => config_page(d, |c| {
                vec![num_row(
                    "Remote streaming limit",
                    c.remote_client_bitrate_limit,
                    " bit/s",
                    Cfg::BitrateLimit,
                )]
            }),
            Section::Transcoding => transcoding(d),
            Section::Backups => backups(d),
            Section::Devices => devices(d),
            Section::Activity => activity(d),
            Section::Plugins => plugins(d),
            Section::Networking => networking(d),
            Section::ApiKeys => api_keys(d),
            Section::Logs => logs(d),
            Section::Tasks => tasks(d),
        }
    }
}

fn row(title: &str, sub: String, acts: Vec<Btn>) -> Item {
    Item {
        title: title.into(),
        sub,
        acts,
        ..Default::default()
    }
}

fn text_row(title: &str, v: Option<String>, c: Cfg) -> Item {
    let v = v.unwrap_or_default();
    row(title, v.clone(), vec![b("Edit", Act::EditCfg(c, v))])
}

fn num_row(title: &str, v: Option<i32>, unit: &str, c: Cfg) -> Item {
    let v = v.unwrap_or(0);
    row(
        title,
        format!("{v}{unit}"),
        vec![b("Edit", Act::EditCfg(c, v.to_string()))],
    )
}

fn config_page(
    d: &Data,
    rows: impl FnOnce(&nocturne_jellyfin_admin::ServerConfig) -> Vec<Item>,
) -> Model {
    Model {
        status: status(&d.config),
        items: d.config.get().map(rows).unwrap_or_default(),
        ..Default::default()
    }
}

fn dashboard(d: &Data) -> Model {
    let mut m = Model {
        toolbar: vec![
            bp("Scan all libraries", Act::RefreshAll),
            bd("Restart", Act::Restart),
            bd("Shutdown", Act::Shutdown),
        ],
        ..Default::default()
    };
    for s in d
        .sessions
        .get()
        .into_iter()
        .flatten()
        .filter(|s| s.is_active)
    {
        let who = s.user_name.clone().unwrap_or("unknown".into());
        let label = format!("{} on {}", s.client, s.device_name);
        let playing = s.now_playing_item.as_ref();
        let paused = s.play_state.as_ref().is_some_and(|p| p.is_paused);
        let mut acts = vec![];
        if s.supports_remote_control {
            acts.push(b("Message", Act::Message((s.id.clone(), label.clone()))));
        }
        if playing.is_some() {
            acts.push(b(
                "Stop playback",
                Act::StopPlayback((s.id.clone(), label.clone())),
            ));
        }
        m.items.push(Item {
            title: format!("{} {}", s.client, s.application_version),
            sub: format!("{} · {}", s.device_name, when(Some(&s.last_activity_date))),
            chips: playing
                .map(|_| {
                    (
                        if paused { "paused" } else { "playing" }.to_string(),
                        !paused,
                    )
                })
                .into_iter()
                .collect(),
            detail: vec![
                ("USER", who),
                (
                    "PLAYING",
                    playing.map_or("nothing".into(), |p| format!("{} ({})", p.name, p.kind)),
                ),
            ],
            acts,
            ..Default::default()
        });
    }
    m
}

fn general(d: &Data) -> Model {
    let mut m = config_page(d, |c| {
        let qc = c.quick_connect_available.unwrap_or(false);
        vec![
            text_row("Server name", c.server_name.clone(), Cfg::Name),
            text_row("Display language", c.ui_culture.clone(), Cfg::Culture),
            num_row(
                "Log retention",
                c.log_file_retention_days,
                " days",
                Cfg::LogDays,
            ),
            num_row(
                "Activity retention",
                c.activity_log_retention_days,
                " days",
                Cfg::ActivityDays,
            ),
            row(
                "Quick Connect",
                yes(qc),
                vec![b(
                    if qc { "Turn off" } else { "Turn on" },
                    Act::ToggleQuickConnect(!qc),
                )],
            ),
        ]
    });
    if let Some(s) = d.system.get() {
        for (t, p) in [
            ("Data folder", &s.program_data_path),
            ("Metadata folder", &s.internal_metadata_path),
            ("Cache folder", &s.cache_path),
            ("Log folder", &s.log_path),
            ("Transcoding temp", &s.transcoding_temp_path),
        ] {
            m.items.push(Item {
                group: Some("Paths (read only)".into()),
                title: t.into(),
                sub: p.clone(),
                ..Default::default()
            });
        }
    }
    for it in m.items.iter_mut().filter(|i| i.group.is_none()) {
        it.group = Some("Server".into());
    }
    m
}

fn branding(d: &Data) -> Model {
    let mut m = Model {
        status: status(&d.branding),
        ..Default::default()
    };
    if let Some(br) = d.branding.get() {
        let set = |o: &Option<String>| {
            o.as_deref()
                .filter(|s| !s.trim().is_empty())
                .map_or("not set".to_string(), |s| {
                    format!("set · {} chars", s.len())
                })
        };
        m.items
            .push(row("Login message", set(&br.login_disclaimer), vec![]));
        m.items.push(row("Custom CSS", set(&br.custom_css), vec![]));
        m.items
            .push(row("Splash screen", yes(br.splashscreen_enabled), vec![]));
    }
    m
}

fn users(d: &Data) -> Model {
    let mut m = Model {
        toolbar: vec![b("Create user", Act::CreateUser)],
        status: status(&d.users),
        ..Default::default()
    };
    for u in d.users.get().into_iter().flatten() {
        let t = (u.id.clone(), u.name.clone());
        let (dis, adm) = (u.policy.is_disabled, u.policy.is_administrator);
        let seen = u
            .last_activity_date
            .as_deref()
            .or(u.last_login_date.as_deref());
        m.items.push(Item {
            title: u.name.clone(),
            sub: format!("last active {}", when(seen)),
            chips: [(adm, "admin", true), (dis, "disabled", false)]
                .into_iter()
                .filter(|c| c.0)
                .map(|c| (c.1.to_string(), c.2))
                .collect(),
            avatar: true,
            detail: vec![
                ("ID", short(&u.id)),
                (
                    "PASSWORD",
                    if u.has_password { "set" } else { "none" }.into(),
                ),
                ("LAST LOGIN", when(u.last_login_date.as_deref())),
                ("LAST ACTIVE", when(u.last_activity_date.as_deref())),
            ],
            acts: vec![
                b(
                    if dis { "Enable" } else { "Disable" },
                    Act::Disable(t.clone(), !dis),
                ),
                b(
                    if adm { "Remove admin" } else { "Make admin" },
                    Act::MakeAdmin(t.clone(), !adm),
                ),
                b("Set password", Act::SetPassword(t.clone())),
                bd("Reset password", Act::ResetPassword(t.clone())),
                bd("Delete", Act::DeleteUser(t)),
            ],
            ..Default::default()
        });
    }
    m
}

fn libraries(d: &Data) -> Model {
    let mut m = Model {
        toolbar: vec![b("Refresh all", Act::RefreshAll)],
        status: status(&d.folders),
        ..Default::default()
    };
    for f in d.folders.get().into_iter().flatten() {
        let busy = f.refresh_status.as_deref().filter(|s| *s != "Idle");
        let mut detail = vec![("TYPE", f.collection_type.clone().unwrap_or("mixed".into()))];
        detail.extend(f.locations.iter().map(|p| ("PATH", p.clone())));
        detail.push(("REFRESH", f.refresh_status.clone().unwrap_or("Idle".into())));
        let o = &f.library_options;
        for (k, key) in [
            ("TRICKPLAY IMAGES", "EnableTrickplayImageExtraction"),
            ("CHAPTER IMAGES", "EnableChapterImageExtraction"),
            ("REAL-TIME MONITOR", "EnableRealtimeMonitor"),
            ("SAVE NFO FILES", "SaveLocalMetadata"),
            ("EMBEDDED TITLES", "EnableEmbeddedTitles"),
        ] {
            if let Some(v) = o[key].as_bool() {
                detail.push((k, yes(v)));
            }
        }
        if let Some(sv) = o["MetadataSavers"].as_array() {
            let names: Vec<_> = sv.iter().filter_map(|x| x.as_str()).collect();
            detail.push((
                "METADATA SAVERS",
                if names.is_empty() {
                    "none".into()
                } else {
                    names.join(", ")
                },
            ));
        }
        m.items.push(Item {
            title: f.name.clone(),
            sub: format!(
                "{} · {}",
                f.collection_type.as_deref().unwrap_or("mixed"),
                f.locations.join(" · ")
            ),
            chips: busy.map(|s| (s.to_lowercase(), true)).into_iter().collect(),
            progress: busy.and(f.refresh_progress).map(|p| (p / 100.0) as f32),
            detail,
            acts: vec![b(
                "Refresh",
                Act::RefreshFolder((f.item_id.clone(), f.name.clone())),
            )],
            ..Default::default()
        });
    }
    m
}

fn transcoding(d: &Data) -> Model {
    let mut m = Model {
        status: status(&d.encoding),
        ..Default::default()
    };
    let Some(c) = d.encoding.get() else { return m };
    let s = |k: &str| c[k].as_str().unwrap_or_default().to_string();
    let n = |k: &str| c[k].as_i64().unwrap_or_default();
    let on = |k: &str| yes(c[k].as_bool().unwrap_or(false));
    let edit = |title: &str, key: &'static str, numeric: bool, v: String| {
        row(
            title,
            v.clone(),
            vec![b("Edit", Act::EditEnc(key, title.to_string(), numeric, v))],
        )
    };
    m.items = vec![
        edit(
            "Hardware acceleration",
            "HardwareAccelerationType",
            false,
            s("HardwareAccelerationType"),
        ),
        edit(
            "Transcode threads (-1 = auto)",
            "EncodingThreadCount",
            true,
            n("EncodingThreadCount").to_string(),
        ),
        edit(
            "Throttle delay (seconds)",
            "ThrottleDelaySeconds",
            true,
            n("ThrottleDelaySeconds").to_string(),
        ),
        row("FFmpeg path", s("EncoderAppPathDisplay"), vec![]),
        row("Transcode temp path", s("TranscodingTempPath"), vec![]),
        row("Throttle transcodes", on("EnableThrottling"), vec![]),
        row("Tone mapping", on("EnableTonemapping"), vec![]),
        row("Hardware encoding", on("EnableHardwareEncoding"), vec![]),
    ];
    m
}

fn backups(d: &Data) -> Model {
    let mut m = Model {
        status: status(&d.backups),
        ..Default::default()
    };
    for b in d.backups.get().into_iter().flatten() {
        let name = b["Path"].as_str().unwrap_or("backup").to_string();
        m.items.push(row(
            &name,
            b["DateCreated"].as_str().unwrap_or_default().to_string(),
            vec![],
        ));
    }
    if m.status.is_none() && m.items.is_empty() {
        m.status = Some("No backups on this server.");
    }
    m
}

fn tasks(d: &Data) -> Model {
    let mut m = Model {
        status: status(&d.tasks),
        ..Default::default()
    };
    let mut v: Vec<_> = d
        .tasks
        .get()
        .into_iter()
        .flatten()
        .filter(|t| !t.is_hidden)
        .collect();
    v.sort_by(|a, c| (&a.category, &a.name).cmp(&(&c.category, &c.name)));
    for t in v {
        let running = t.state == "Running";
        let tgt = (t.id.clone(), t.name.clone());
        let last = t.last_execution_result.as_ref();
        m.items.push(Item {
            group: Some(t.category.clone()),
            title: t.name.clone(),
            sub: last.map_or("never run".into(), |r| {
                format!(
                    "last: {} · {}",
                    r.status.to_lowercase(),
                    when(Some(&r.end_time_utc))
                )
            }),
            chips: vec![(t.state.to_lowercase(), running)],
            progress: running.then(|| t.current_progress_percentage.unwrap_or(0.0) as f32 / 100.0),
            detail: vec![
                ("ABOUT", t.description.clone()),
                ("STATE", t.state.clone()),
                (
                    "LAST RESULT",
                    last.map_or("none".into(), |r| r.status.clone()),
                ),
                ("LAST RUN", when(last.map(|r| r.end_time_utc.as_str()))),
                ("TRIGGERS", t.triggers.len().to_string()),
            ],
            acts: vec![if running {
                b("Stop", Act::StopTask(tgt))
            } else {
                b("Run now", Act::StartTask(tgt))
            }],
            ..Default::default()
        });
    }
    m
}

fn devices(d: &Data) -> Model {
    let mut m = Model {
        status: status(&d.devices),
        ..Default::default()
    };
    for dv in d.devices.get().into_iter().flatten() {
        let app = dv.app_name.clone().unwrap_or_default();
        m.items.push(Item {
            title: dv.name.clone(),
            sub: format!(
                "{app} · {} · {}",
                dv.last_user_name.clone().unwrap_or_default(),
                when(Some(&dv.date_last_activity))
            ),
            avatar: true,
            detail: vec![
                (
                    "APP",
                    format!("{app} {}", dv.app_version.clone().unwrap_or_default()),
                ),
                ("LAST USER", dv.last_user_name.clone().unwrap_or_default()),
                ("LAST ACTIVE", when(Some(&dv.date_last_activity))),
                ("ID", short(&dv.id)),
            ],
            acts: vec![bd(
                "Remove device",
                Act::DeleteDevice((dv.id.clone(), dv.name.clone())),
            )],
            ..Default::default()
        });
    }
    m
}

fn activity(d: &Data) -> Model {
    let mut m = Model {
        status: status(&d.activity),
        ..Default::default()
    };
    for e in d.activity.get().into_iter().flat_map(|a| &a.items) {
        let warn = matches!(e.severity.as_str(), "Warning" | "Error" | "Critical");
        m.items.push(Item {
            title: e.name.clone(),
            sub: format!("{} · {}", when(Some(&e.date)), e.kind),
            chips: if warn {
                vec![(e.severity.to_lowercase(), false)]
            } else {
                vec![]
            },
            detail: vec![
                ("TYPE", e.kind.clone()),
                ("SEVERITY", e.severity.clone()),
                ("WHEN", when(Some(&e.date))),
            ],
            text: e.overview.clone().filter(|s| !s.is_empty()),
            ..Default::default()
        });
    }
    m
}

fn plugins(d: &Data) -> Model {
    let mut m = Model {
        status: status(&d.plugins),
        ..Default::default()
    };
    for p in d.plugins.get().into_iter().flatten() {
        let mut it = row(
            &p.name,
            format!("v{} · {}", p.version, p.description),
            vec![],
        );
        it.chips = vec![(p.status.to_lowercase(), p.status == "Active")];
        it.detail = vec![
            ("VERSION", p.version.clone()),
            ("STATUS", p.status.clone()),
            ("ID", short(&p.id)),
        ];
        m.items.push(it);
    }
    m
}

fn networking(d: &Data) -> Model {
    let mut m = Model {
        status: status(&d.network),
        ..Default::default()
    };
    if let Some(n) = d.network.get() {
        let port = |p: u16| p.to_string();
        for (t, v) in [
            ("HTTP port", port(n.internal_http_port)),
            ("HTTPS port", port(n.internal_https_port)),
            ("Public HTTP port", port(n.public_http_port)),
            ("Public HTTPS port", port(n.public_https_port)),
            (
                "Base URL",
                if n.base_url.is_empty() {
                    "/".into()
                } else {
                    n.base_url.clone()
                },
            ),
            ("HTTPS", yes(n.enable_https)),
            ("Require HTTPS", yes(n.require_https)),
            ("Remote access", yes(n.enable_remote_access)),
            ("Auto discovery", yes(n.auto_discovery)),
            ("UPnP", yes(n.enable_upnp)),
            ("IPv4", yes(n.enable_ipv4)),
            ("IPv6", yes(n.enable_ipv6)),
        ] {
            m.items.push(row(t, v, vec![]));
        }
    }
    m
}

fn api_keys(d: &Data) -> Model {
    let mut m = Model {
        toolbar: vec![bp("Create API key", Act::CreateKey)],
        status: status(&d.keys),
        ..Default::default()
    };
    for k in d.keys.get().into_iter().flatten() {
        let mut it = row(
            &k.app_name,
            format!(
                "created {} · used {}",
                when(Some(&k.date_created)),
                when(Some(&k.date_last_activity))
            ),
            vec![bd(
                "Delete key",
                Act::DeleteKey((k.access_token.clone(), k.app_name.clone())),
            )],
        );
        it.detail = vec![
            ("CREATED", when(Some(&k.date_created))),
            ("LAST USED", when(Some(&k.date_last_activity))),
        ];
        m.items.push(it);
    }
    m
}

fn logs(d: &Data) -> Model {
    let mut m = Model {
        status: status(&d.logs),
        ..Default::default()
    };
    for f in d.logs.get().into_iter().flatten() {
        let mut it = row(
            &f.name,
            format!("{} · {}", bytes(f.size), when(Some(&f.date_modified))),
            vec![b("View tail", Act::ViewLog(f.name.clone()))],
        );
        it.text = Some(match &d.log {
            Some((n, t)) if *n == f.name => t.clone(),
            _ => "Press Enter on View tail to load the last 200 lines.".into(),
        });
        it.detail = vec![
            ("SIZE", bytes(f.size)),
            ("MODIFIED", when(Some(&f.date_modified))),
        ];
        m.items.push(it);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_index_counts_group_headers() {
        let it = |g: Option<String>| Item {
            group: g,
            ..Default::default()
        };
        let m = Model {
            items: vec![
                it(Some("a".into())),
                it(Some("a".into())),
                it(Some("b".into())),
                it(Some("b".into())),
            ],
            ..Default::default()
        };
        assert_eq!([0, 1, 2, 3].map(|i| m.child_ix(i)), [1, 2, 4, 5]);
    }
}
