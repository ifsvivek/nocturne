//! Pure validation for the structured admin edits. Everything here runs BEFORE a request is
//! built, so a typo can never reach the server (a wrong network value can lock the admin out).

use crate::{Error, Result, TaskTrigger};
use serde_json::{Value, json};
use std::net::IpAddr;

pub const TICKS_PER_SEC: i64 = 10_000_000;

fn bad<T>(m: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(m.into()))
}

// ---- scheduled-task triggers ----

pub const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Canonical English day name for `s` (case-insensitive), if it is one.
pub fn day_name(s: &str) -> Option<&'static str> {
    DAYS.into_iter().find(|d| d.eq_ignore_ascii_case(s.trim()))
}

/// `HH:MM` (24 h) to ticks since midnight.
pub fn parse_time_of_day(s: &str) -> Option<i64> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some((h * 3600 + m * 60) * TICKS_PER_SEC)
}

/// Ticks since midnight back to `HH:MM`.
pub fn format_time_of_day(ticks: i64) -> String {
    let m = ticks / TICKS_PER_SEC / 60;
    format!("{:02}:{:02}", m / 60 % 24, m % 60)
}

impl TaskTrigger {
    pub fn daily(time: &str) -> Result<Self> {
        let t = Self {
            kind: "DailyTrigger".into(),
            time_of_day_ticks: parse_time_of_day(time),
            ..Default::default()
        };
        validate_trigger(&t).map(|_| t)
    }

    pub fn weekly(day: &str, time: &str) -> Result<Self> {
        let t = Self {
            kind: "WeeklyTrigger".into(),
            time_of_day_ticks: parse_time_of_day(time),
            day_of_week: day_name(day).map(str::to_string),
            ..Default::default()
        };
        validate_trigger(&t).map(|_| t)
    }

    pub fn every_minutes(minutes: i64) -> Result<Self> {
        let t = Self {
            kind: "IntervalTrigger".into(),
            interval_ticks: Some(minutes.saturating_mul(60 * TICKS_PER_SEC)),
            ..Default::default()
        };
        validate_trigger(&t).map(|_| t)
    }

    pub fn on_startup() -> Self {
        Self {
            kind: "StartupTrigger".into(),
            ..Default::default()
        }
    }

    /// One-line description for lists, such as `Daily at 03:00`.
    pub fn describe(&self) -> String {
        let at = self.time_of_day_ticks.map(format_time_of_day);
        match self.kind.as_str() {
            "DailyTrigger" => format!("Daily at {}", at.unwrap_or_default()),
            "WeeklyTrigger" => format!(
                "{} at {}",
                self.day_of_week.as_deref().unwrap_or("?"),
                at.unwrap_or_default()
            ),
            "IntervalTrigger" => {
                let m = self.interval_ticks.unwrap_or(0) / TICKS_PER_SEC / 60;
                match (m / 60, m % 60) {
                    (0, m) => format!("Every {m} min"),
                    (h, 0) => format!("Every {h} h"),
                    (h, m) => format!("Every {h} h {m} min"),
                }
            }
            "StartupTrigger" => "On server startup".into(),
            k => k.into(),
        }
    }
}

pub fn validate_trigger(t: &TaskTrigger) -> Result<()> {
    let day_ticks = 86_400 * TICKS_PER_SEC;
    let time_ok = t
        .time_of_day_ticks
        .is_some_and(|x| (0..day_ticks).contains(&x));
    match t.kind.as_str() {
        "DailyTrigger" if time_ok => {}
        "DailyTrigger" => return bad("a daily trigger needs a time of day (HH:MM)"),
        "WeeklyTrigger" if time_ok && t.day_of_week.as_deref().and_then(day_name).is_some() => {}
        "WeeklyTrigger" => return bad("a weekly trigger needs a day name and a time of day"),
        "IntervalTrigger" if t.interval_ticks.is_some_and(|x| x >= 60 * TICKS_PER_SEC) => {}
        "IntervalTrigger" => return bad("an interval trigger needs at least one minute"),
        "StartupTrigger" => {}
        k => return bad(format!("unknown trigger type {k}")),
    }
    if t.max_runtime_ticks.is_some_and(|x| x <= 0) {
        return bad("maximum runtime must be positive");
    }
    Ok(())
}

// ---- networking ----

/// The network settings NOCTURNE may change. Deliberately NOT included: base URL, HTTPS and
/// certificates, proxies and IP filters, which are easier to get wrong and harder to undo.
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkEdit {
    HttpPort(u16),
    PublicHttpPort(u16),
    HttpsPort(u16),
    PublicHttpsPort(u16),
    /// Addresses to bind to; empty = all interfaces.
    BindAddresses(Vec<String>),
    /// LAN networks: addresses or CIDR ranges.
    LanNetworks(Vec<String>),
    RemoteAccess(bool),
    Upnp(bool),
    AutoDiscovery(bool),
    Ipv4(bool),
    Ipv6(bool),
}

/// Split `a, b  c` into entries (commas, semicolons and whitespace all separate).
pub fn split_list(s: &str) -> Vec<String> {
    s.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn valid_network(s: &str) -> bool {
    match s.split_once('/') {
        None => s.parse::<IpAddr>().is_ok(),
        Some((a, p)) => match (a.parse::<IpAddr>(), p.parse::<u8>()) {
            (Ok(IpAddr::V4(_)), Ok(p)) => p <= 32,
            (Ok(IpAddr::V6(_)), Ok(p)) => p <= 128,
            _ => false,
        },
    }
}

impl NetworkEdit {
    fn key(&self) -> &'static str {
        match self {
            Self::HttpPort(_) => "InternalHttpPort",
            Self::PublicHttpPort(_) => "PublicHttpPort",
            Self::HttpsPort(_) => "InternalHttpsPort",
            Self::PublicHttpsPort(_) => "PublicHttpsPort",
            Self::BindAddresses(_) => "LocalNetworkAddresses",
            Self::LanNetworks(_) => "LocalNetworkSubnets",
            Self::RemoteAccess(_) => "EnableRemoteAccess",
            Self::Upnp(_) => "EnableUPnP",
            Self::AutoDiscovery(_) => "AutoDiscovery",
            Self::Ipv4(_) => "EnableIPv4",
            Self::Ipv6(_) => "EnableIPv6",
        }
    }

    /// Validate and apply to the raw network configuration. Every other field is untouched.
    pub fn apply(&self, cfg: &mut Value) -> Result<()> {
        let val = match self {
            Self::HttpPort(p)
            | Self::PublicHttpPort(p)
            | Self::HttpsPort(p)
            | Self::PublicHttpsPort(p) => {
                if *p == 0 {
                    return bad("port must be 1 to 65535");
                }
                json!(p)
            }
            Self::BindAddresses(l) | Self::LanNetworks(l) => {
                let bind = matches!(self, Self::BindAddresses(_));
                if let Some(x) = l.iter().find(|x| {
                    if bind {
                        x.parse::<IpAddr>().is_err()
                    } else {
                        !valid_network(x)
                    }
                }) {
                    return bad(format!(
                        "{x} is not a valid {}",
                        if bind { "address" } else { "network" }
                    ));
                }
                json!(l)
            }
            Self::RemoteAccess(b)
            | Self::Upnp(b)
            | Self::AutoDiscovery(b)
            | Self::Ipv4(b)
            | Self::Ipv6(b) => json!(b),
        };
        cfg[self.key()] = val;
        if matches!(self, Self::Ipv4(_) | Self::Ipv6(_))
            && !(cfg["EnableIPv4"].as_bool().unwrap_or(true)
                || cfg["EnableIPv6"].as_bool().unwrap_or(false))
        {
            return bad("at least one of IPv4 and IPv6 must stay on");
        }
        if let (Some(a), Some(b)) = (
            cfg["InternalHttpPort"].as_u64(),
            cfg["InternalHttpsPort"].as_u64(),
        ) && a == b
        {
            return bad("the HTTP and HTTPS ports must differ");
        }
        Ok(())
    }
}

// ---- plugin repositories ----

/// A repository needs a name and an http(s) manifest URL. Adding one makes the server download
/// code from it, so a plain `http://` URL is allowed but the UI warns about it.
pub fn validate_repository(name: &str, url: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 100 {
        return bad("a repository needs a name of up to 100 characters");
    }
    let ok = reqwest::Url::parse(url.trim())
        .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some());
    if !ok {
        return bad("the repository address must be an http or https URL");
    }
    Ok(())
}

// ---- trickplay and library options ----

/// Checks one `TrickplayOptions` field before it is written.
pub fn validate_trickplay(key: &str, v: &Value) -> Result<()> {
    let int = |lo: i64, hi: i64| match v.as_i64() {
        Some(n) if (lo..=hi).contains(&n) => Ok(()),
        _ => bad(format!("{key} must be a number from {lo} to {hi}")),
    };
    let one_of = |l: &[&str]| match v.as_str() {
        Some(s) if l.contains(&s) => Ok(()),
        _ => bad(format!("{key} must be one of {}", l.join(", "))),
    };
    match key {
        "EnableHwAcceleration" | "EnableHwEncoding" | "EnableKeyFrameOnlyExtraction" => {
            if v.is_boolean() {
                Ok(())
            } else {
                bad(format!("{key} must be on or off"))
            }
        }
        "Interval" => int(1000, 600_000),
        "TileWidth" | "TileHeight" => int(1, 50),
        "JpegQuality" => int(1, 100),
        "Qscale" => int(2, 31),
        "ProcessThreads" => int(0, 64),
        "ScanBehavior" => one_of(&["Blocking", "NonBlocking"]),
        "ProcessPriority" => one_of(&["High", "AboveNormal", "Normal", "BelowNormal", "Idle"]),
        "WidthResolutions" => match v.as_array() {
            Some(a)
                if !a.is_empty()
                    && a.iter()
                        .all(|x| x.as_i64().is_some_and(|n| (16..=8192).contains(&n))) =>
            {
                Ok(())
            }
            _ => bad("WidthResolutions must be a list of widths from 16 to 8192"),
        },
        _ => bad(format!("{key} is not an editable trickplay setting")),
    }
}

/// Library option keys NOCTURNE may change (the rest round-trips untouched).
pub const LIBRARY_OPTION_KEYS: [&str; 10] = [
    "Enabled",
    "EnableRealtimeMonitor",
    "EnableTrickplayImageExtraction",
    "ExtractTrickplayImagesDuringLibraryScan",
    "EnableChapterImageExtraction",
    "SaveLocalMetadata",
    "EnableEmbeddedTitles",
    "EnableInternetProviders",
    "SaveTrickplayWithMedia",
    "AutomaticRefreshIntervalDays",
];

pub fn validate_library_option(key: &str, v: &Value) -> Result<()> {
    match key {
        "AutomaticRefreshIntervalDays" if v.as_i64().is_some_and(|n| (0..=3650).contains(&n)) => {
            Ok(())
        }
        "AutomaticRefreshIntervalDays" => bad("refresh interval must be 0 to 3650 days"),
        k if LIBRARY_OPTION_KEYS.contains(&k) && v.is_boolean() => Ok(()),
        k => bad(format!("{k} is not an editable library setting")),
    }
}

// ---- metadata and image providers ----

/// One fetcher of a library type, in the order the server tries them.
#[derive(Debug, Clone, PartialEq)]
pub struct Fetcher {
    pub name: String,
    pub enabled: bool,
}

fn names(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect()
}

fn type_entry<'a>(options: &'a Value, item_type: &str) -> Option<&'a Value> {
    options["TypeOptions"]
        .as_array()?
        .iter()
        .find(|t| t["Type"] == item_type)
}

/// The fetchers of `item_type` as the library has them now (`images` picks image over metadata
/// fetchers): the server's order first, then providers it offers that the library does not know
/// yet. A library with no stored entry uses the server's defaults.
pub fn current_fetchers(
    options: &Value,
    item_type: &str,
    avail: &crate::AvailableType,
    images: bool,
) -> Vec<Fetcher> {
    let offered = if images {
        &avail.image_fetchers
    } else {
        &avail.metadata_fetchers
    };
    let (on, order) = if images {
        ("ImageFetchers", "ImageFetcherOrder")
    } else {
        ("MetadataFetchers", "MetadataFetcherOrder")
    };
    let Some(e) = type_entry(options, item_type) else {
        return offered
            .iter()
            .map(|p| Fetcher {
                name: p.name.clone(),
                enabled: p.default_enabled,
            })
            .collect();
    };
    let (enabled, stored) = (names(&e[on]), names(&e[order]));
    let known = |n: &String| offered.iter().any(|p| &p.name == n);
    let mut list: Vec<String> = stored.into_iter().filter(known).collect();
    for p in offered {
        if !list.contains(&p.name) {
            list.push(p.name.clone());
        }
    }
    list.into_iter()
        .map(|n| Fetcher {
            enabled: enabled.contains(&n),
            name: n,
        })
        .collect()
}

/// Validate `list` against what the server offers and write it into `options`. The other kind
/// of fetcher of the same type is kept (or materialised from the defaults), never emptied.
pub fn apply_fetchers(
    options: &mut Value,
    item_type: &str,
    avail: &crate::AvailableType,
    images: bool,
    list: &[Fetcher],
) -> Result<()> {
    let offered = if images {
        &avail.image_fetchers
    } else {
        &avail.metadata_fetchers
    };
    let mut seen: Vec<&str> = list.iter().map(|f| f.name.as_str()).collect();
    seen.sort_unstable();
    let mut want: Vec<&str> = offered.iter().map(|p| p.name.as_str()).collect();
    want.sort_unstable();
    if seen != want {
        return bad("the list must contain every provider the server offers, once each");
    }
    let (other_images, kinds) = (!images, ["Metadata", "Image"]);
    let pick = |i: bool| kinds[usize::from(i)];
    let other = current_fetchers(options, item_type, avail, other_images);
    let mut entry = match type_entry(options, item_type) {
        Some(e) => e.clone(),
        None => json!({"Type": item_type}),
    };
    let put = |e: &mut Value, imgs: bool, l: &[Fetcher]| {
        e[format!("{}Fetchers", pick(imgs))] = json!(
            l.iter()
                .filter(|f| f.enabled)
                .map(|f| &f.name)
                .collect::<Vec<_>>()
        );
        e[format!("{}FetcherOrder", pick(imgs))] =
            json!(l.iter().map(|f| &f.name).collect::<Vec<_>>());
    };
    put(&mut entry, images, list);
    put(&mut entry, other_images, &other);
    let types = options["TypeOptions"].take();
    let mut types = types.as_array().cloned().unwrap_or_default();
    match types.iter_mut().find(|t| t["Type"] == item_type) {
        Some(t) => *t = entry,
        None => types.push(entry),
    }
    options["TypeOptions"] = json!(types);
    Ok(())
}

/// Flip fetcher `i` on or off; the last enabled fetcher may be turned off (a library can have none).
pub fn toggle_fetcher(list: &mut [Fetcher], i: usize) -> Result<()> {
    list.get_mut(i)
        .map(|f| f.enabled = !f.enabled)
        .ok_or_else(|| Error::Invalid("no such provider".into()))
}

/// Move fetcher `i` one place up (`up`) or down.
pub fn move_fetcher(list: &mut [Fetcher], i: usize, up: bool) -> Result<()> {
    let j = if up { i.checked_sub(1) } else { Some(i + 1) };
    match j.filter(|j| *j < list.len() && i < list.len()) {
        Some(j) => {
            list.swap(i, j);
            Ok(())
        }
        None => bad("cannot move it further"),
    }
}

/// Local metadata readers (NFO and so on) the library has switched off.
pub fn disabled_readers(options: &Value) -> Vec<String> {
    names(&options["DisabledLocalMetadataReaders"])
}

/// Library kinds the server accepts; `mixed` is sent as no type.
pub const LIBRARY_TYPES: [&str; 8] = [
    "movies",
    "tvshows",
    "music",
    "musicvideos",
    "homevideos",
    "boxsets",
    "books",
    "mixed",
];

pub fn validate_library_name(n: &str) -> Result<()> {
    if n.trim().is_empty() || n.contains(['/', '\\']) || n.len() > 100 {
        return bad("library names cannot be empty or contain slashes");
    }
    Ok(())
}

pub fn validate_library_path(p: &str) -> Result<()> {
    if !p.starts_with('/') || p.contains('\0') {
        return bad("the folder must be an absolute path on the server, such as /media/movies");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_triggers() {
        assert_eq!(
            parse_time_of_day("03:30"),
            Some(3 * 3600 * TICKS_PER_SEC + 1800 * TICKS_PER_SEC)
        );
        assert_eq!(parse_time_of_day("24:00"), None);
        assert_eq!(parse_time_of_day("3pm"), None);
        assert_eq!(
            format_time_of_day(parse_time_of_day("07:05").unwrap()),
            "07:05"
        );
        assert_eq!(
            TaskTrigger::daily("03:00").unwrap().describe(),
            "Daily at 03:00"
        );
        assert_eq!(
            TaskTrigger::weekly("sunday", "02:15").unwrap().describe(),
            "Sunday at 02:15"
        );
        assert_eq!(
            TaskTrigger::every_minutes(90).unwrap().describe(),
            "Every 1 h 30 min"
        );
        assert_eq!(
            TaskTrigger::every_minutes(360).unwrap().describe(),
            "Every 6 h"
        );
        assert!(TaskTrigger::every_minutes(0).is_err());
        assert!(TaskTrigger::weekly("funday", "02:00").is_err());
        assert!(TaskTrigger::daily("25:00").is_err());
        assert!(validate_trigger(&TaskTrigger::on_startup()).is_ok());
        let v = serde_json::to_value(TaskTrigger::daily("01:00").unwrap()).unwrap();
        assert_eq!(v["Type"], "DailyTrigger");
        assert!(
            v.get("IntervalTicks").is_none(),
            "absent fields are omitted"
        );
    }

    #[test]
    fn network_edits_validate() {
        let base = json!({"InternalHttpPort": 8096, "InternalHttpsPort": 8920, "EnableIPv4": true,
            "EnableIPv6": false, "Keep": [1]});
        let mut c = base.clone();
        NetworkEdit::HttpPort(9000).apply(&mut c).unwrap();
        assert_eq!(
            (c["InternalHttpPort"].as_i64(), c["Keep"].clone()),
            (Some(9000), json!([1]))
        );
        assert!(NetworkEdit::HttpPort(0).apply(&mut base.clone()).is_err());
        assert!(
            NetworkEdit::HttpPort(8920)
                .apply(&mut base.clone())
                .is_err(),
            "ports must differ"
        );
        assert!(
            NetworkEdit::Ipv4(false).apply(&mut base.clone()).is_err(),
            "never both off"
        );
        assert!(
            NetworkEdit::Ipv4(false)
                .apply(&mut json!({"EnableIPv4": true, "EnableIPv6": true}))
                .is_ok()
        );
        let mut c = base.clone();
        NetworkEdit::LanNetworks(split_list("192.168.1.0/24, fd00::/8 10.0.0.5"))
            .apply(&mut c)
            .unwrap();
        assert_eq!(c["LocalNetworkSubnets"].as_array().unwrap().len(), 3);
        for bad in ["192.168.1.0/33", "host.lan", "1.2.3.4/x", "::1/129"] {
            assert!(
                NetworkEdit::LanNetworks(vec![bad.into()])
                    .apply(&mut base.clone())
                    .is_err(),
                "{bad}"
            );
        }
        assert!(
            NetworkEdit::BindAddresses(vec!["192.168.1.0/24".into()])
                .apply(&mut base.clone())
                .is_err()
        );
        assert!(
            NetworkEdit::BindAddresses(vec![])
                .apply(&mut base.clone())
                .is_ok(),
            "empty = all interfaces"
        );
    }

    fn avail() -> crate::AvailableType {
        serde_json::from_value(json!({"Type":"Movie",
            "MetadataFetchers":[{"Name":"A","DefaultEnabled":true},{"Name":"B","DefaultEnabled":false}],
            "ImageFetchers":[{"Name":"X","DefaultEnabled":true}]}))
        .unwrap()
    }

    #[test]
    fn fetchers_default_toggle_move_and_apply() {
        let mut o = json!({"Keep": 1, "TypeOptions": []});
        let mut l = current_fetchers(&o, "Movie", &avail(), false);
        assert_eq!(
            l,
            [
                Fetcher {
                    name: "A".into(),
                    enabled: true
                },
                Fetcher {
                    name: "B".into(),
                    enabled: false
                }
            ]
        );
        toggle_fetcher(&mut l, 1).unwrap();
        move_fetcher(&mut l, 1, true).unwrap();
        assert!(move_fetcher(&mut l, 0, true).is_err() && move_fetcher(&mut l, 1, false).is_err());
        assert!(toggle_fetcher(&mut l, 5).is_err());
        apply_fetchers(&mut o, "Movie", &avail(), false, &l).unwrap();
        let e = &o["TypeOptions"][0];
        assert_eq!(e["MetadataFetcherOrder"], json!(["B", "A"]));
        assert_eq!(e["MetadataFetchers"], json!(["B", "A"]));
        assert_eq!(
            e["ImageFetchers"],
            json!(["X"]),
            "image fetchers materialised, not emptied"
        );
        assert_eq!(o["Keep"], 1);
        // reading back gives the same list; a provider the server added later is appended
        assert_eq!(current_fetchers(&o, "Movie", &avail(), false), l);
        let mut a2 = avail();
        a2.metadata_fetchers.push(crate::Provider {
            name: "C".into(),
            default_enabled: true,
        });
        assert_eq!(current_fetchers(&o, "Movie", &a2, false).len(), 3);
        // unknown or missing names are refused
        let mut bad_l = l.clone();
        bad_l[0].name = "Z".into();
        assert!(apply_fetchers(&mut o, "Movie", &avail(), false, &bad_l).is_err());
        assert!(apply_fetchers(&mut o, "Movie", &avail(), false, &l[..1]).is_err());
    }

    #[test]
    fn repository_values() {
        assert!(validate_repository("Mine", "https://r.example/manifest.json").is_ok());
        assert!(validate_repository("Mine", "http://10.0.0.2/m.json").is_ok());
        assert!(validate_repository("", "https://r.example/m.json").is_err());
        assert!(validate_repository("x", "ftp://r.example/m.json").is_err());
        assert!(validate_repository("x", "not a url").is_err());
    }

    #[test]
    fn trickplay_and_library_values() {
        assert!(validate_trickplay("JpegQuality", &json!(80)).is_ok());
        assert!(validate_trickplay("JpegQuality", &json!(101)).is_err());
        assert!(validate_trickplay("WidthResolutions", &json!([320, 640])).is_ok());
        assert!(validate_trickplay("WidthResolutions", &json!([])).is_err());
        assert!(validate_trickplay("ScanBehavior", &json!("Sometimes")).is_err());
        assert!(validate_trickplay("Nope", &json!(1)).is_err());
        assert!(validate_library_option("EnableRealtimeMonitor", &json!(true)).is_ok());
        assert!(validate_library_option("EnableRealtimeMonitor", &json!(1)).is_err());
        assert!(validate_library_option("PathInfos", &json!([])).is_err());
        assert!(
            validate_library_path("/media/x").is_ok() && validate_library_path("media").is_err()
        );
        assert!(validate_library_name("Films").is_ok() && validate_library_name("a/b").is_err());
        assert_eq!(split_list(" a,,b ;c"), ["a", "b", "c"]);
    }
}
