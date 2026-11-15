//! Turns loaded data into rows and buttons for the list screens (Users, Libraries, Tasks,
//! and so on). The dashboard's session cards use the same rows.
use super::Admin;
use super::act::{Act, Brand, Cfg, Lim, Net, Tp, TrigKind};
use super::fmt::{bytes, short, when};
use super::state::{Data, L, LibProviders, Section};
use nocturne_jellyfin_admin::edit::{self, Fetcher};

pub struct Btn {
    pub label: String,
    pub act: Act,
    pub danger: bool,
    pub primary: bool,
}

fn btn(label: impl Into<String>, act: Act, danger: bool, primary: bool) -> Btn {
    Btn {
        label: label.into(),
        act,
        danger,
        primary,
    }
}
fn b(label: impl Into<String>, act: Act) -> Btn {
    btn(label, act, false, false)
}
fn bd(label: impl Into<String>, act: Act) -> Btn {
    btn(label, act, true, false)
}
fn bp(label: impl Into<String>, act: Act) -> Btn {
    btn(label, act, false, true)
}

#[derive(Default)]
pub struct Item {
    pub group: Option<String>,
    pub title: String,
    pub sub: String,
    pub chips: Vec<(String, bool)>,
    pub avatar: bool,
    /// (user id, `PrimaryImageTag`) of a profile picture to show in the avatar.
    pub pic: Option<(String, String)>,
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
            Section::Access => access(d),
            Section::Libraries => libraries(d),
            Section::Providers => providers(d),
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
            Section::Trickplay => trickplay(d),
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
        let mut text = |title: &str, o: &Option<String>, k: Brand| {
            let cur = o.clone().unwrap_or_default();
            let mut it = row(
                title,
                set(o),
                vec![b("Edit", Act::EditBrand(k, cur.clone()))],
            );
            it.text = Some(if cur.trim().is_empty() {
                "Nothing set.".into()
            } else {
                cur
            });
            m.items.push(it);
        };
        text("Login message", &br.login_disclaimer, Brand::Disclaimer);
        text("Custom CSS", &br.custom_css, Brand::Css);
        let on = br.splashscreen_enabled;
        m.items.push(row(
            "Splash screen",
            yes(on),
            vec![b(
                if on { "Turn off" } else { "Turn on" },
                Act::ToggleSplash(!on),
            )],
        ));
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
            pic: u.primary_image_tag.clone().map(|tag| (u.id.clone(), tag)),
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

/// Per user: library access, feature switches, limits and the profile image.
fn access(d: &Data) -> Model {
    let mut m = Model {
        status: status(&d.users).or(status(&d.folders)),
        ..Default::default()
    };
    let (Some(users), Some(folders)) = (d.users.get(), d.folders.get()) else {
        return m;
    };
    let ratings = d.ratings.get().map(Vec::as_slice).unwrap_or_default();
    for u in users {
        let t = (u.id.clone(), u.name.clone());
        let raw = serde_json::to_value(&u.policy).unwrap_or_default();
        let mut add = |g: &str, mut it: Item| {
            it.group = Some(format!("{} · {g}", u.name));
            m.items.push(it);
        };
        for f in folders.iter().filter(|f| !f.item_id.is_empty()) {
            let on = edit::folder_allowed(&raw, &f.item_id);
            let mut it = row(
                &f.name,
                f.collection_type.clone().unwrap_or("mixed".into()),
                vec![b(
                    if on { "Block" } else { "Allow" },
                    Act::UserLibrary(t.clone(), (f.item_id.clone(), f.name.clone()), !on),
                )],
            );
            it.chips = vec![(if on { "allowed" } else { "blocked" }.into(), on)];
            add("Libraries", it);
        }
        for (key, label) in edit::POLICY_FLAGS {
            let on = u.policy.flag(key);
            let mut it = row(
                label,
                String::new(),
                vec![b(
                    if on { "Turn off" } else { "Turn on" },
                    Act::UserFlag(t.clone(), key, label, !on),
                )],
            );
            it.chips = vec![(yes(on), on)];
            add("Access", it);
        }
        let limit = |title: &str, k: Lim, v: i64, zero: &str| {
            let sub = if v == 0 {
                zero.to_string()
            } else {
                v.to_string()
            };
            row(
                title,
                sub,
                vec![b("Edit", Act::UserLimit(t.clone(), k, v.to_string()))],
            )
        };
        let level = u.policy.get("MaxParentalRating").as_i64();
        let rating = level.map_or(String::new(), |l| {
            ratings
                .iter()
                .find(|r| r.value.map(i64::from) == Some(l))
                .map_or(format!("level {l}"), |r| r.name.clone())
        });
        let names: Vec<_> = ratings.iter().map(|r| r.name.as_str()).collect();
        add(
            "Limits and content",
            row(
                "Maximum parental rating",
                if rating.is_empty() {
                    "no limit".into()
                } else {
                    rating.clone()
                },
                vec![b(
                    "Edit",
                    Act::UserRating(t.clone(), rating, names.join(", ")),
                )],
            ),
        );
        for (title, blocked, key) in [
            ("Blocked tags", true, "BlockedTags"),
            ("Allowed tags", false, "AllowedTags"),
        ] {
            let l: Vec<String> = u
                .policy
                .get(key)
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect();
            let sub = if l.is_empty() {
                "none".into()
            } else {
                l.join(", ")
            };
            add(
                "Limits and content",
                row(
                    title,
                    sub,
                    vec![b("Edit", Act::UserTags(t.clone(), blocked, l.join(", ")))],
                ),
            );
        }
        let n = |k: &str| u.policy.get(k).as_i64().unwrap_or(0);
        add(
            "Limits and content",
            limit(
                "Remote streaming limit (bit/s)",
                Lim::Bitrate,
                n("RemoteClientBitrateLimit"),
                "no limit",
            ),
        );
        add(
            "Limits and content",
            limit(
                "Simultaneous sessions",
                Lim::Sessions,
                n("MaxActiveSessions"),
                "unlimited",
            ),
        );
        let att = u
            .policy
            .get("LoginAttemptsBeforeLockout")
            .as_i64()
            .unwrap_or(0);
        add(
            "Password and sign-in",
            row(
                "Failed sign-ins before lockout",
                match att {
                    -1 => "never locks".to_string(),
                    0 => "server default".to_string(),
                    n => n.to_string(),
                },
                vec![b(
                    "Edit",
                    Act::UserLimit(t.clone(), Lim::Attempts, att.to_string()),
                )],
            ),
        );
        let mut info = row(
            "Password status",
            format!(
                "{} · {} failed sign-ins so far",
                if u.has_password {
                    "has a password"
                } else {
                    "no password"
                },
                n("InvalidLoginAttemptCount")
            ),
            vec![],
        );
        info.text = Some("Jellyfin has no password length, complexity or expiry rules. Set or reset a password on the Users page.".into());
        add("Password and sign-in", info);
        let has_img = u.primary_image_tag.is_some();
        let mut acts = vec![b("Upload image", Act::UserImage(t.clone()))];
        if has_img {
            acts.push(bd("Remove image", Act::UserImageRemove(t.clone())));
        }
        let mut img = row(
            "Profile image",
            if has_img { "set".into() } else { "none".into() },
            acts,
        );
        img.chips = vec![(if has_img { "set" } else { "none" }.into(), has_img)];
        add("Profile", img);
        let mut prefs = row(
            "Display preferences",
            "what this user saved in a client (read only)".into(),
            vec![b("Load", Act::ViewPrefs(t.clone()))],
        );
        prefs.text = Some(match &d.prefs {
            Some((id, text)) if *id == u.id => text.clone(),
            _ => "Press Enter on Load to read them.".into(),
        });
        add("Profile", prefs);
    }
    m
}

fn libraries(d: &Data) -> Model {
    let mut m = Model {
        toolbar: vec![
            bp("Add library", Act::AddLibrary),
            b("Refresh all", Act::RefreshAll),
        ],
        status: status(&d.folders),
        ..Default::default()
    };
    for f in d.folders.get().into_iter().flatten() {
        let busy = f.refresh_status.as_deref().filter(|s| *s != "Idle");
        let mut detail = vec![("TYPE", f.collection_type.clone().unwrap_or("mixed".into()))];
        detail.extend(f.locations.iter().map(|p| ("PATH", p.clone())));
        detail.push(("REFRESH", f.refresh_status.clone().unwrap_or("Idle".into())));
        let o = &f.library_options;
        detail.push((
            "METADATA REFRESH",
            match o["AutomaticRefreshIntervalDays"].as_i64().unwrap_or(0) {
                0 => "never automatically".into(),
                n => format!("every {n} days"),
            },
        ));
        detail.push(("LIBRARY SCAN", scan_schedule(d)));
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
            acts: lib_acts(f),
            ..Default::default()
        });
    }
    m
}

/// When the server scans for new files: the triggers of its "Scan Media Library" task. The
/// schedule is server wide, not per library; edit it under Scheduled Tasks.
fn scan_schedule(d: &Data) -> String {
    let Some(t) = d
        .tasks
        .get()
        .and_then(|t| t.iter().find(|t| t.key == "RefreshLibrary"))
    else {
        return "see Scheduled Tasks".into();
    };
    if t.triggers.is_empty() {
        return "no schedule (server wide task)".into();
    }
    let l: Vec<_> = t.triggers.iter().map(|x| x.describe()).collect();
    format!("{} (server wide task)", l.join(", "))
}

/// Buttons of one library: scan, folders, the common options, remove.
fn lib_acts(f: &nocturne_jellyfin_admin::VirtualFolder) -> Vec<Btn> {
    let t = (f.item_id.clone(), f.name.clone());
    let mut v = vec![b("Refresh", Act::RefreshFolder(t.clone()))];
    v.push(b("Rename", Act::RenameLibrary(t.clone())));
    v.push(b("Add folder", Act::AddLibPath(t.clone())));
    if f.locations.len() > 1 {
        for (i, p) in f.locations.iter().enumerate() {
            v.push(bd(
                format!("Remove folder {}", i + 1),
                Act::RemoveLibPath(t.clone(), p.clone()),
            ));
        }
    }
    let o = &f.library_options;
    for (label, key) in [
        ("Real-time monitor", "EnableRealtimeMonitor"),
        ("Trickplay images", "EnableTrickplayImageExtraction"),
        ("Chapter images", "EnableChapterImageExtraction"),
        ("Save NFO files", "SaveLocalMetadata"),
        ("Embedded titles", "EnableEmbeddedTitles"),
        ("Internet providers", "EnableInternetProviders"),
    ] {
        if let Some(on) = o[key].as_bool() {
            v.push(b(
                format!("{label}: {}", yes(on)),
                Act::LibOption(t.clone(), key, label, !on),
            ));
        }
    }
    let days = o["AutomaticRefreshIntervalDays"].as_i64().unwrap_or(0);
    v.push(b(
        format!("Refresh metadata every: {days} d"),
        Act::LibRefreshDays(t.clone(), days.to_string()),
    ));
    v.push(bd("Remove library", Act::RemoveLibrary(t)));
    v
}

/// Per library and item type: the metadata and image fetchers in the order the server tries
/// them, plus local readers. Only what the server offers is listed; what it lacks is said so.
fn providers(d: &Data) -> Model {
    let mut m = Model {
        status: status(&d.providers),
        ..Default::default()
    };
    for lp in d.providers.get().into_iter().flatten() {
        provider_rows(&mut m, lp);
    }
    if m.status.is_none() && m.items.is_empty() {
        m.status = Some("There are no libraries yet.");
    }
    m
}

fn provider_rows(m: &mut Model, lp: &LibProviders) {
    let f = &lp.folder;
    let t = (f.item_id.clone(), f.name.clone());
    let kind = f.collection_type.as_deref().unwrap_or("mixed");
    let note = |m: &mut Model, g: String, title: &str, sub: String| {
        let mut it = row(title, sub, vec![]);
        it.group = Some(g);
        m.items.push(it);
    };
    for ty in &lp.avail.type_options {
        for images in [false, true] {
            let what = if images {
                "Image fetchers"
            } else {
                "Metadata fetchers"
            };
            let group = format!("{} · {} · {what}", f.name, ty.kind);
            let offered = if images {
                &ty.image_fetchers
            } else {
                &ty.metadata_fetchers
            };
            if offered.is_empty() {
                note(
                    m,
                    group,
                    "None offered",
                    format!(
                        "This server has no {} for {} items. A plugin may add some.",
                        what.to_lowercase(),
                        ty.kind
                    ),
                );
                continue;
            }
            let list = edit::current_fetchers(&f.library_options, &ty.kind, ty, images);
            for (i, p) in list.iter().enumerate() {
                let change = |l: Vec<Fetcher>, what_changes: String| {
                    Act::Fetchers(t.clone(), ty.kind.clone(), images, what_changes, l)
                };
                let mut acts = vec![];
                let mut l = list.clone();
                if edit::toggle_fetcher(&mut l, i).is_ok() {
                    acts.push(b(
                        if p.enabled { "Turn off" } else { "Turn on" },
                        change(
                            l,
                            format!(
                                "{} {}",
                                if p.enabled { "Turn off" } else { "Turn on" },
                                p.name
                            ),
                        ),
                    ));
                }
                for (up, label) in [(true, "Move up"), (false, "Move down")] {
                    let mut l = list.clone();
                    if edit::move_fetcher(&mut l, i, up).is_ok() {
                        acts.push(b(label, change(l, format!("{label}: {}", p.name))));
                    }
                }
                let mut it = row(&p.name, format!("priority {}", i + 1), acts);
                it.chips = vec![(if p.enabled { "on" } else { "off" }.into(), p.enabled)];
                it.group = Some(group.clone());
                m.items.push(it);
            }
        }
    }
    let readers = edit::disabled_readers(&f.library_options);
    for r in &lp.avail.metadata_readers {
        let on = !readers.contains(&r.name);
        let mut it = row(
            &r.name,
            "reads metadata files stored next to your media".into(),
            vec![b(
                if on { "Turn off" } else { "Turn on" },
                Act::Reader(t.clone(), r.name.clone(), !on),
            )],
        );
        it.chips = vec![(if on { "on" } else { "off" }.into(), on)];
        it.group = Some(format!("{} · Local metadata readers", f.name));
        m.items.push(it);
    }
    let names = |l: &[nocturne_jellyfin_admin::Provider]| {
        if l.is_empty() {
            "none installed".to_string()
        } else {
            l.iter()
                .map(|p| p.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    for (title, l) in [
        ("Subtitle providers", &lp.avail.subtitle_fetchers),
        ("Lyric providers", &lp.avail.lyric_fetchers),
        ("Media segment providers", &lp.avail.media_segment_providers),
        ("Metadata savers", &lp.avail.metadata_savers),
    ] {
        note(
            m,
            format!("{} · {kind} · Offered by this server (read only)", f.name),
            title,
            names(l),
        );
    }
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

fn trickplay(d: &Data) -> Model {
    config_page(d, |c| {
        let t = &c.extra["TrickplayOptions"];
        let num = |k: &str| t[k].as_i64().map_or("?".to_string(), |n| n.to_string());
        let text = |k: &str| t[k].as_str().unwrap_or("?").to_string();
        let flag = |title: &str, key: &'static str| {
            let on = t[key].as_bool().unwrap_or(false);
            row(
                title,
                yes(on),
                vec![b(
                    if on { "Turn off" } else { "Turn on" },
                    Act::ToggleTrickplay(key, title.to_string(), !on),
                )],
            )
        };
        let edit = |title: &str, key: &'static str, kind: Tp, v: String, unit: &str| {
            row(
                title,
                format!("{v}{unit}"),
                vec![b(
                    "Edit",
                    Act::EditTrickplay(key, title.to_string(), kind, v),
                )],
            )
        };
        if t.is_null() {
            return vec![row(
                "Trickplay",
                "This server reports no trickplay settings.".into(),
                vec![],
            )];
        }
        let widths = t["WidthResolutions"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_i64())
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        vec![
            edit("Interval", "Interval", Tp::Num, num("Interval"), " ms"),
            edit(
                "Thumbnail widths",
                "WidthResolutions",
                Tp::List,
                widths,
                " px",
            ),
            edit(
                "Tile width",
                "TileWidth",
                Tp::Num,
                num("TileWidth"),
                " images",
            ),
            edit(
                "Tile height",
                "TileHeight",
                Tp::Num,
                num("TileHeight"),
                " images",
            ),
            edit(
                "JPEG quality",
                "JpegQuality",
                Tp::Num,
                num("JpegQuality"),
                "",
            ),
            edit("Qscale", "Qscale", Tp::Num, num("Qscale"), ""),
            edit(
                "Process threads (0 = auto)",
                "ProcessThreads",
                Tp::Num,
                num("ProcessThreads"),
                "",
            ),
            edit(
                "Scan behaviour",
                "ScanBehavior",
                Tp::Scan,
                text("ScanBehavior"),
                "",
            ),
            edit(
                "Process priority",
                "ProcessPriority",
                Tp::Priority,
                text("ProcessPriority"),
                "",
            ),
            flag("Hardware decoding", "EnableHwAcceleration"),
            flag("Hardware encoding", "EnableHwEncoding"),
            flag("Key frames only", "EnableKeyFrameOnlyExtraction"),
        ]
    })
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
            acts: task_acts(t, running),
            ..Default::default()
        });
        for (i, tr) in t.triggers.iter().enumerate().take(TRIG_KEYS.len()) {
            m.items
                .last_mut()
                .expect("just pushed")
                .detail
                .push((TRIG_KEYS[i], tr.describe()));
        }
    }
    m
}

const TRIG_KEYS: [&str; 8] = [
    "TRIGGER 1",
    "TRIGGER 2",
    "TRIGGER 3",
    "TRIGGER 4",
    "TRIGGER 5",
    "TRIGGER 6",
    "TRIGGER 7",
    "TRIGGER 8",
];

fn task_acts(t: &nocturne_jellyfin_admin::ScheduledTask, running: bool) -> Vec<Btn> {
    let tgt = (t.id.clone(), t.name.clone());
    let mut v = vec![if running {
        b("Stop", Act::StopTask(tgt.clone()))
    } else {
        b("Run now", Act::StartTask(tgt.clone()))
    }];
    for (label, k) in [
        ("Add daily", TrigKind::Daily),
        ("Add weekly", TrigKind::Weekly),
        ("Add interval", TrigKind::Interval),
        ("Add on startup", TrigKind::Startup),
    ] {
        v.push(b(label, Act::AddTrigger(tgt.clone(), k)));
    }
    for (i, tr) in t.triggers.iter().enumerate().take(TRIG_KEYS.len()) {
        if TrigKind::of(tr).is_some_and(|k| k != TrigKind::Startup) {
            v.push(b(
                format!("Edit trigger {}", i + 1),
                Act::EditTrigger(tgt.clone(), i, tr.clone()),
            ));
        }
        v.push(bd(
            format!("Delete trigger {}", i + 1),
            Act::DeleteTrigger(tgt.clone(), i, tr.describe()),
        ));
    }
    v
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
        toolbar: vec![
            b("Add repository", Act::RepoAdd),
            bd("Restart server to apply", Act::Restart),
        ],
        status: status(&d.plugins),
        ..Default::default()
    };
    let installed: Vec<_> = d.plugins.get().into_iter().flatten().collect();
    for p in &installed {
        let mut it = row(
            &p.name,
            format!("v{} · {}", p.version, p.description),
            vec![],
        );
        let pending = matches!(p.status.as_str(), "Restart" | "Deleted");
        it.chips = vec![(
            if pending {
                "restart needed".into()
            } else {
                p.status.to_lowercase()
            },
            p.status == "Active",
        )];
        it.detail = vec![
            ("VERSION", p.version.clone()),
            ("STATUS", p.status.clone()),
            ("ID", short(&p.id)),
        ];
        // Plugins bundled with the server cannot be switched off or removed from here.
        if p.can_uninstall {
            let off = p.status == "Disabled";
            let tgt = |on| Act::PluginEnable(p.id.clone(), p.version.clone(), p.name.clone(), on);
            it.acts = vec![
                b(if off { "Enable" } else { "Disable" }, tgt(off)),
                bd(
                    "Uninstall",
                    Act::PluginUninstall(p.id.clone(), p.version.clone(), p.name.clone()),
                ),
            ];
        }
        it.group = Some("Installed".into());
        m.items.push(it);
    }
    if m.status.is_none() {
        for r in d.repos.get().into_iter().flatten() {
            let mut it = row(
                &r.name,
                r.url.clone(),
                vec![
                    b(
                        if r.enabled { "Disable" } else { "Enable" },
                        Act::RepoEnable(r.url.clone(), r.name.clone(), !r.enabled),
                    ),
                    bd("Remove", Act::RepoRemove(r.url.clone(), r.name.clone())),
                ],
            );
            it.chips = vec![(
                if r.enabled { "enabled" } else { "disabled" }.into(),
                r.enabled,
            )];
            it.group = Some("Repositories".into());
            m.items.push(it);
        }
        match &d.packages {
            L::Ready(pk) => {
                let mut avail: Vec<_> = pk
                    .iter()
                    .filter(|x| {
                        !installed.iter().any(|i| i.name == x.name) && !x.versions.is_empty()
                    })
                    .collect();
                avail.sort_by(|a, c| a.name.cmp(&c.name));
                for x in avail {
                    let mut it = row(
                        &x.name,
                        format!("{} · {}", x.category, x.description.trim()),
                        vec![b("Install", Act::PluginInstall(Box::new(x.clone())))],
                    );
                    it.detail = vec![
                        ("OWNER", x.owner.clone()),
                        ("LATEST", x.versions[0].version.clone()),
                    ];
                    it.group = Some("Available from the plugin catalog".into());
                    m.items.push(it);
                }
            }
            L::Failed => {
                let mut it = row(
                    "Plugin catalog unavailable",
                    "The server could not reach its plugin repositories. Press R to try again."
                        .into(),
                    vec![],
                );
                it.group = Some("Available from the plugin catalog".into());
                m.items.push(it);
            }
            L::Loading => {}
        }
    }
    m
}

fn networking(d: &Data) -> Model {
    let mut m = Model {
        toolbar: vec![bd("Restart server to apply", Act::Restart)],
        status: status(&d.network),
        ..Default::default()
    };
    let Some(n) = d.network.get() else { return m };
    let mut grp = |g: &str, rows: Vec<Item>| {
        for mut r in rows {
            r.group = Some(g.into());
            m.items.push(r);
        }
    };
    let mut warn = row(
        "Wrong values can lock you out",
        "Read before changing anything here.".into(),
        vec![],
    );
    warn.text = Some("A wrong port, bind address or network can make this server unreachable, including from NOCTURNE, until it is fixed on the server machine. Each change asks you to type NETWORK. Most changes apply after a restart.".into());
    grp("Read this first", vec![warn]);
    let port = |t: &str, p: u16, k: Net| {
        row(
            t,
            p.to_string(),
            vec![bd("Edit", Act::EditNet(k, p.to_string()))],
        )
    };
    let list = |t: &str, l: &[String], k: Net, empty: &str| {
        row(
            t,
            if l.is_empty() {
                empty.to_string()
            } else {
                l.join(", ")
            },
            vec![bd("Edit", Act::EditNet(k, l.join(", ")))],
        )
    };
    let flag = |k: Net, on: bool| {
        row(
            k.label(),
            yes(on),
            vec![bd(
                if on { "Turn off" } else { "Turn on" },
                Act::NetFlag(k, !on),
            )],
        )
    };
    grp(
        "Ports",
        vec![
            port("HTTP port", n.internal_http_port, Net::HttpPort),
            port("Public HTTP port", n.public_http_port, Net::PublicHttpPort),
            port("HTTPS port", n.internal_https_port, Net::HttpsPort),
            port(
                "Public HTTPS port",
                n.public_https_port,
                Net::PublicHttpsPort,
            ),
        ],
    );
    grp(
        "Addresses and networks",
        vec![
            list(
                "Bind addresses",
                &n.local_network_addresses,
                Net::Bind,
                "all interfaces",
            ),
            list(
                "LAN networks",
                &n.local_network_subnets,
                Net::Lan,
                "detected automatically",
            ),
        ],
    );
    grp(
        "Access",
        vec![
            flag(Net::Remote, n.enable_remote_access),
            flag(Net::Upnp, n.enable_upnp),
            flag(Net::Discovery, n.auto_discovery),
            flag(Net::Ipv4, n.enable_ipv4),
            flag(Net::Ipv6, n.enable_ipv6),
        ],
    );
    grp(
        "Not editable here",
        [
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
        ]
        .into_iter()
        .map(|(t, v)| row(t, v, vec![]))
        .collect(),
    );
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
    use nocturne_jellyfin_admin::TaskTrigger;

    fn data_with(f: impl FnOnce(&mut Data)) -> Data {
        let mut d = Data::default();
        f(&mut d);
        d
    }

    #[test]
    fn plugin_actions_follow_what_the_server_allows() {
        let plug = |n: &str, st: &str, un: bool| nocturne_jellyfin_admin::Plugin {
            name: n.into(),
            status: st.into(),
            can_uninstall: un,
            ..Default::default()
        };
        let pkg = |n: &str| nocturne_jellyfin_admin::Package {
            name: n.into(),
            versions: vec![Default::default()],
            ..Default::default()
        };
        let d = data_with(|d| {
            d.plugins = L::Ready(vec![
                plug("Bundled", "Active", false),
                plug("Extra", "Disabled", true),
            ]);
            d.packages = L::Ready(vec![pkg("Extra"), pkg("New")]);
        });
        let m = plugins(&d);
        let labels = |i: usize| {
            m.items[i]
                .acts
                .iter()
                .map(|a| a.label.clone())
                .collect::<Vec<_>>()
        };
        assert!(labels(0).is_empty(), "bundled plugins offer nothing");
        assert_eq!(labels(1), ["Enable", "Uninstall"]);
        assert_eq!(m.items.len(), 3, "installed plugins are not offered again");
        assert_eq!(labels(2), ["Install"]);
        let d = data_with(|d| {
            d.plugins = L::Ready(vec![]);
            d.repos = L::Ready(vec![nocturne_jellyfin_admin::Repository {
                name: "R".into(),
                url: "https://r/m".into(),
                enabled: false,
            }]);
        });
        let m = plugins(&d);
        let acts: Vec<_> = m.items[0]
            .acts
            .iter()
            .map(|a| (a.label.as_str(), a.danger))
            .collect();
        assert_eq!(acts, [("Enable", false), ("Remove", true)]);
        assert_eq!(m.items[0].group.as_deref(), Some("Repositories"));
    }

    #[test]
    fn access_rows_cover_libraries_features_limits_and_image() {
        use nocturne_jellyfin_admin::{ParentalRating, User, VirtualFolder};
        let mut u = User {
            id: "u".into(),
            name: "ann".into(),
            ..Default::default()
        };
        u.policy = serde_json::from_value(serde_json::json!({
            "EnableAllFolders": false, "EnabledFolders": ["AA"], "EnableRemoteAccess": true,
            "MaxParentalRating": 13, "BlockedTags": ["gore"], "LoginAttemptsBeforeLockout": -1
        }))
        .unwrap();
        let f = |id: &str, n: &str| VirtualFolder {
            item_id: id.into(),
            name: n.into(),
            ..Default::default()
        };
        let d = data_with(|d| {
            d.users = L::Ready(vec![u]);
            d.folders = L::Ready(vec![f("aa", "A"), f("bb", "B")]);
            d.ratings = L::Ready(vec![ParentalRating {
                name: "PG-13".into(),
                value: Some(13),
            }]);
        });
        let m = access(&d);
        let find = |t: &str| m.items.iter().find(|i| i.title == t).unwrap();
        assert_eq!(find("A").chips[0].0, "allowed");
        assert_eq!(find("B").chips[0].0, "blocked");
        assert_eq!(find("B").acts[0].label, "Allow");
        assert_eq!(find("Remote access").chips[0].0, "on");
        assert_eq!(find("Downloads").chips[0].0, "off");
        assert_eq!(find("Maximum parental rating").sub, "PG-13");
        assert_eq!(find("Blocked tags").sub, "gore");
        assert_eq!(find("Failed sign-ins before lockout").sub, "never locks");
        assert_eq!(find("Profile image").acts.len(), 1, "nothing to remove yet");
        assert!(
            m.items
                .iter()
                .all(|i| i.group.as_deref().is_some_and(|g| g.starts_with("ann · ")))
        );
        assert!(
            find("Password status")
                .text
                .as_deref()
                .unwrap()
                .contains("no password length")
        );
    }

    #[test]
    fn provider_rows_follow_what_the_server_offers() {
        use nocturne_jellyfin_admin::{AvailableOptions, AvailableType, Provider, VirtualFolder};
        let p = |n: &str, on| Provider {
            name: n.into(),
            default_enabled: on,
        };
        let lp = LibProviders {
            folder: VirtualFolder {
                name: "Films".into(),
                item_id: "L".into(),
                collection_type: Some("movies".into()),
                ..Default::default()
            },
            avail: AvailableOptions {
                metadata_readers: vec![p("Nfo", true)],
                type_options: vec![AvailableType {
                    kind: "Movie".into(),
                    metadata_fetchers: vec![p("A", true), p("B", false)],
                    ..Default::default()
                }],
                ..Default::default()
            },
        };
        let d = data_with(|d| d.providers = L::Ready(vec![lp]));
        let m = providers(&d);
        let labels = |i: usize| {
            m.items[i]
                .acts
                .iter()
                .map(|a| a.label.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            m.items[0].group.as_deref(),
            Some("Films · Movie · Metadata fetchers")
        );
        assert_eq!(
            labels(0),
            ["Turn off", "Move down"],
            "first row cannot move up"
        );
        assert_eq!(
            labels(1),
            ["Turn on", "Move up"],
            "last row cannot move down"
        );
        assert_eq!(
            m.items[2].title, "None offered",
            "image fetchers missing: said so"
        );
        assert_eq!(m.items[3].title, "Nfo");
        assert!(
            m.items.iter().skip(4).all(|i| i.acts.is_empty()),
            "the rest is read only"
        );
        // Move down on A makes the list B, A with both states kept
        let Act::Fetchers(_, ty, images, _, l) = &m.items[0].acts[1].act else {
            panic!()
        };
        assert_eq!((ty.as_str(), *images), ("Movie", false));
        assert_eq!(
            l.iter()
                .map(|f| (f.name.as_str(), f.enabled))
                .collect::<Vec<_>>(),
            [("B", false), ("A", true)]
        );
    }

    #[test]
    fn library_rows_show_the_scan_schedule() {
        let d = data_with(|d| {
            d.tasks = L::Ready(vec![nocturne_jellyfin_admin::ScheduledTask {
                key: "RefreshLibrary".into(),
                triggers: vec![TaskTrigger::every_minutes(720).unwrap()],
                ..Default::default()
            }]);
        });
        assert_eq!(scan_schedule(&d), "Every 12 h (server wide task)");
        assert_eq!(scan_schedule(&Data::default()), "see Scheduled Tasks");
    }

    #[test]
    fn network_buttons_are_all_danger_and_tasks_list_triggers() {
        let d = data_with(|d| d.network = L::Ready(Default::default()));
        let m = networking(&d);
        let acts: Vec<_> = m.items.iter().flat_map(|i| &i.acts).collect();
        assert!(acts.len() >= 11 && acts.iter().all(|a| a.danger));
        let t = nocturne_jellyfin_admin::ScheduledTask {
            id: "t".into(),
            name: "Scan".into(),
            triggers: vec![
                TaskTrigger::daily("03:00").unwrap(),
                TaskTrigger::on_startup(),
            ],
            ..Default::default()
        };
        let labels: Vec<_> = task_acts(&t, false).into_iter().map(|b| b.label).collect();
        assert_eq!(
            labels,
            [
                "Run now",
                "Add daily",
                "Add weekly",
                "Add interval",
                "Add on startup",
                "Edit trigger 1",
                "Delete trigger 1",
                "Delete trigger 2"
            ]
        );
    }

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
