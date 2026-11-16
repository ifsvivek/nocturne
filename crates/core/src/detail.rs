//! Item detail data: the full record's extra sections mapped to plain UI types, plus the
//! formatting helpers (stars, end time, media info table). No window code here.
use crate::source::{Media, fmt_clock};
use nocturne_jellyfin as jf;

const TICKS_PER_S: f64 = 10_000_000.0;

/// One row of the "Media info" table.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamRow {
    pub kind: &'static str,
    pub title: String,
    pub codec: String,
    pub detail: String,
    pub bitrate: String,
}

/// One media source (file) with its streams.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InfoBlock {
    /// Jellyfin media source id (empty when unknown).
    pub id: String,
    /// The version's own name (usually the file name); empty when the server gives none.
    pub name: String,
    /// "4K" / "1080p" of the first video stream, and its codec in capitals.
    pub quality: String,
    pub codec: String,
    pub container: String,
    pub size: String,
    pub bitrate: String,
    pub streams: Vec<StreamRow>,
}

#[derive(Clone, Debug, Default)]
pub struct Full {
    pub tagline: Option<String>,
    pub official_rating: Option<String>,
    pub critic: Option<f32>,
    pub studios: Vec<(String, String)>,
    pub links: Vec<(String, String)>,
    /// Cast and crew as person records (kind Person, `sub` = role or job).
    pub people: Vec<Media>,
    /// Chapters as playable records (`resume_s` = start, `number` = index, tag = image tag).
    pub chapters: Vec<Media>,
    pub info: Vec<InfoBlock>,
    pub premiere: Option<String>,
    pub places: Vec<String>,
}

impl Full {
    pub fn from_item(i: &jf::Item) -> Self {
        let x = &i.x;
        let people = x
            .people
            .iter()
            .filter(|p| !p.id.is_empty())
            .map(|p| {
                let sub = p
                    .role
                    .clone()
                    .filter(|r| !r.trim().is_empty())
                    .unwrap_or_else(|| p.kind.clone());
                let mut m = Media::from_item(&jf::Item {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    kind: "Person".into(),
                    ..Default::default()
                });
                m.poster_tag = p.primary_image_tag.clone();
                m.sub = Some(sub);
                m
            })
            .collect();
        let chapters = x
            .chapters
            .iter()
            .enumerate()
            .filter(|(_, c)| c.image_tag.is_some())
            .map(|(n, c)| {
                let start = c.start_position_ticks as f64 / TICKS_PER_S;
                let mut m = Media::from_item(&jf::Item {
                    id: i.id.clone(),
                    name: c
                        .name
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| format!("Chapter {}", n + 1)),
                    kind: "Chapter".into(),
                    ..Default::default()
                });
                m.number = Some(n as u32);
                m.poster_tag = c.image_tag.clone();
                m.resume_s = Some(start);
                m.sub = Some(fmt_clock(start));
                m
            })
            .collect();
        Self {
            tagline: x.taglines.first().cloned().filter(|s| !s.trim().is_empty()),
            official_rating: x.official_rating.clone().filter(|s| !s.is_empty()),
            critic: x.critic_rating,
            studios: x
                .studios
                .iter()
                .map(|s| (s.id.clone(), s.name.clone()))
                .collect(),
            links: x
                .external_urls
                .iter()
                .filter(|u| u.url.starts_with("http"))
                .map(|u| (u.name.clone(), u.url.clone()))
                .collect(),
            people,
            chapters,
            info: x.media_sources.iter().map(info_block).collect(),
            premiere: x.premiere_date.clone(),
            places: x.production_locations.clone(),
        }
    }
}

fn info_block(s: &jf::MediaSource) -> InfoBlock {
    let video = s.media_streams.iter().find(|v| v.kind == "Video");
    InfoBlock {
        id: s.id.clone(),
        name: s.name.clone().unwrap_or_default(),
        quality: video
            .and_then(|v| Some(resolution_label(v.width?, v.height?).to_string()))
            .unwrap_or_default(),
        codec: video
            .and_then(|v| v.codec.clone())
            .unwrap_or_default()
            .to_uppercase(),
        container: s.container.clone().unwrap_or_default().to_uppercase(),
        size: s.size.map(fmt_size).unwrap_or_default(),
        bitrate: s.bitrate.map(fmt_bitrate).unwrap_or_default(),
        streams: s.media_streams.iter().filter_map(stream_row).collect(),
    }
}

fn stream_row(s: &jf::Stream) -> Option<StreamRow> {
    let kind = match s.kind.as_str() {
        "Video" => "Video",
        "Audio" => "Audio",
        "Subtitle" => "Subtitle",
        _ => return None,
    };
    let mut d: Vec<String> = vec![];
    match kind {
        "Video" => {
            if let (Some(w), Some(h)) = (s.width, s.height) {
                d.push(format!("{w}x{h} {}", resolution_label(w, h)));
            }
            if let Some(f) = s.average_frame_rate.filter(|f| *f > 0.0) {
                d.push(format!("{f:.3} fps").replace(".000", ""));
            }
            if let Some(b) = s.bit_depth {
                d.push(format!("{b}-bit"));
            }
            let range = s.video_range_type.as_deref().or(s.video_range.as_deref());
            if let Some(r) = range.filter(|r| !r.eq_ignore_ascii_case("sdr") && !r.is_empty()) {
                d.push(r.to_string());
            }
        }
        "Audio" => {
            if let Some(l) = s
                .channel_layout
                .clone()
                .or(s.channels.map(|c| format!("{c} ch")))
            {
                d.push(l);
            }
            if let Some(r) = s.sample_rate {
                d.push(format!("{} kHz", r as f32 / 1000.0));
            }
            if let Some(l) = s.language.as_deref().filter(|l| !l.is_empty()) {
                d.push(l.to_string());
            }
        }
        _ => {
            if let Some(l) = s.language.as_deref().filter(|l| !l.is_empty()) {
                d.push(l.to_string());
            }
        }
    }
    for (on, label) in [
        (s.is_default, "default"),
        (s.is_forced, "forced"),
        (s.is_external, "external"),
    ] {
        if on {
            d.push(label.into());
        }
    }
    Some(StreamRow {
        kind,
        title: s
            .display_title
            .clone()
            .or(s.title.clone())
            .unwrap_or_default(),
        codec: s.codec.clone().unwrap_or_default().to_uppercase(),
        detail: d.join(" · "),
        bitrate: s.bit_rate.map(fmt_bitrate).unwrap_or_default(),
    })
}

pub fn resolution_label(w: u32, h: u32) -> &'static str {
    match (w, h) {
        (w, h) if w >= 3500 || h >= 2000 => "4K",
        (w, h) if w >= 2500 || h >= 1300 => "1440p",
        (w, h) if w >= 1800 || h >= 1000 => "1080p",
        (w, h) if w >= 1200 || h >= 700 => "720p",
        (_, h) if h >= 500 => "576p",
        _ => "SD",
    }
}

pub fn fmt_bitrate(bps: u64) -> String {
    if bps >= 1_000_000 {
        format!("{:.1} Mbps", bps as f64 / 1e6)
    } else {
        format!("{} kbps", bps / 1000)
    }
}

pub fn fmt_size(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else {
        format!("{:.0} MB", b / MB)
    }
}

/// Community rating (0..10) as five stars in halves: `(full, half, empty)`.
pub fn stars(rating: f32) -> (u8, bool, u8) {
    let halves = (rating.clamp(0.0, 10.0)).round() as u8; // 0..=10 half-stars
    let (full, half) = (halves / 2, halves % 2 == 1);
    (full, half, 5 - full - half as u8)
}

/// "Ends at 9:35 PM" given the local time of day in minutes and the runtime.
pub fn ends_at(now_min: u32, runtime_min: u32) -> String {
    let t = (now_min + runtime_min) % (24 * 60);
    let (h, m) = (t / 60, t % 60);
    format!(
        "{}:{m:02} {}",
        if h % 12 == 0 { 12 } else { h % 12 },
        if h < 12 { "AM" } else { "PM" }
    )
}

/// Local minutes since midnight.
pub fn local_minutes() -> u32 {
    // SAFETY: localtime_r only writes into the zeroed tm we pass.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        (tm.tm_hour * 60 + tm.tm_min).max(0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_in_halves() {
        assert_eq!(stars(0.0), (0, false, 5));
        assert_eq!(stars(7.4), (3, true, 1), "7.4 -> 7 half stars");
        assert_eq!(stars(10.0), (5, false, 0));
        assert_eq!(stars(99.0), (5, false, 0), "clamped");
    }

    #[test]
    fn end_time_wraps_midnight() {
        assert_eq!(ends_at(20 * 60, 95), "9:35 PM");
        assert_eq!(ends_at(23 * 60 + 30, 90), "1:00 AM");
        assert_eq!(ends_at(0, 0), "12:00 AM");
        assert_eq!(ends_at(12 * 60, 5), "12:05 PM");
    }

    #[test]
    fn formats() {
        assert_eq!(fmt_bitrate(8_000_000), "8.0 Mbps");
        assert_eq!(fmt_bitrate(640_000), "640 kbps");
        assert_eq!(fmt_size(4_500_000_000), "4.2 GB");
        assert_eq!(fmt_size(300 * 1024 * 1024), "300 MB");
        assert_eq!(resolution_label(3840, 2160), "4K");
        assert_eq!(resolution_label(1920, 800), "1080p");
        assert_eq!(resolution_label(1280, 720), "720p");
        assert_eq!(resolution_label(640, 360), "SD");
    }

    #[test]
    fn maps_people_chapters_and_streams() {
        let j = r#"{"Id":"it","Name":"Invented","Type":"Movie","OfficialRating":"PG-13","CriticRating":71,
            "Taglines":["Tag"],"People":[
              {"Id":"p1","Name":"Ann","Role":"Lead","Type":"Actor","PrimaryImageTag":"pt"},
              {"Id":"p2","Name":"Bo","Type":"Director"},{"Name":"NoId","Type":"Actor"}],
            "Chapters":[{"StartPositionTicks":0,"Name":"Open","ImageTag":"c0"},
              {"StartPositionTicks":3600000000,"ImageTag":"c1"},{"StartPositionTicks":9,"Name":"x"}],
            "ExternalUrls":[{"Name":"IMDb","Url":"https://example.test/a"},{"Name":"bad","Url":"file:///x"}],
            "Studios":[{"Name":"S1","Id":"s1"}],
            "MediaSources":[{"Container":"mkv","Size":4500000000,"Bitrate":9000000,"MediaStreams":[
              {"Type":"Video","Codec":"hevc","Width":3840,"Height":2160,"AverageFrameRate":23.976,"BitDepth":10,"VideoRangeType":"HDR10","BitRate":8000000},
              {"Type":"Audio","Codec":"eac3","ChannelLayout":"5.1","Language":"eng","IsDefault":true,"SampleRate":48000},
              {"Type":"Subtitle","Codec":"srt","Language":"eng","IsExternal":true},
              {"Type":"EmbeddedImage"}]}]}"#;
        let i: jf::Item = serde_json::from_str(j).unwrap();
        let f = Full::from_item(&i);
        assert_eq!(f.official_rating.as_deref(), Some("PG-13"));
        assert_eq!(f.people.len(), 2, "person without id dropped");
        assert_eq!(f.people[0].sub.as_deref(), Some("Lead"));
        assert_eq!(
            f.people[1].sub.as_deref(),
            Some("Director"),
            "role falls back to job"
        );
        assert_eq!(f.people[0].poster_tag.as_deref(), Some("pt"));
        assert_eq!(
            f.chapters.len(),
            2,
            "chapters without an image are not shown"
        );
        assert_eq!(f.chapters[1].title, "Chapter 2");
        assert_eq!(f.chapters[1].resume_s, Some(360.0));
        assert_eq!(f.chapters[1].number, Some(1));
        assert_eq!(f.links.len(), 1);
        assert_eq!(f.studios, [("s1".to_string(), "S1".to_string())]);
        let b = &f.info[0];
        assert_eq!((b.container.as_str(), b.size.as_str()), ("MKV", "4.2 GB"));
        assert_eq!(b.streams.len(), 3, "unknown stream types skipped");
        assert_eq!(b.streams[0].codec, "HEVC");
        assert_eq!(
            b.streams[0].detail,
            "3840x2160 4K · 23.976 fps · 10-bit · HDR10"
        );
        assert_eq!(b.streams[0].bitrate, "8.0 Mbps");
        assert_eq!(b.streams[1].detail, "5.1 · 48 kHz · eng · default");
        assert_eq!(b.streams[2].detail, "eng · external");
    }
}
