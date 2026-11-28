//! Release-name parser: pure functions over scene and P2P naming conventions (movies, episodes, music, books) and the normalised [`Quality`].
//!
//! Heuristic by nature. The table tests in `tests/parser.rs` pin the behaviour; add a case there for every name that parses wrong.

use regex::Regex;
use std::sync::LazyLock;

macro_rules! re {
    ($s:literal) => {{
        static R: LazyLock<Regex> = LazyLock::new(|| Regex::new($s).unwrap());
        &*R
    }};
}

macro_rules! qualities {
    ($($v:ident = $n:literal),* $(,)?) => {
        /// Normalised quality. Declaration order is the default ranking, worst to best, within each family (video like Radarr/Sonarr's quality definitions, then music like Lidarr's, then books like Readarr's); comparing across families means nothing. Profiles order their own allowed list.
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
        pub enum Quality {
            #[default]
            $($v),*
        }

        impl Quality {
            pub const ALL: &[Quality] = &[$(Quality::$v),*];

            /// Display name in the *arr style (`WEBDL-1080p`, `FLAC 24bit`, `EPUB`).
            pub fn name(self) -> &'static str {
                match self { $(Quality::$v => $n),* }
            }
        }
    };
}

qualities! {
    Unknown = "Unknown",
    WorkPrint = "WORKPRINT",
    Cam = "CAM",
    Telesync = "TELESYNC",
    Telecine = "TELECINE",
    Regional = "REGIONAL",
    DvdScr = "DVDSCR",
    Sdtv = "SDTV",
    Dvd = "DVD",
    DvdR = "DVD-R",
    WebDl480p = "WEBDL-480p",
    WebRip480p = "WEBRip-480p",
    Bluray480p = "Bluray-480p",
    Bluray576p = "Bluray-576p",
    Hdtv720p = "HDTV-720p",
    WebDl720p = "WEBDL-720p",
    WebRip720p = "WEBRip-720p",
    Bluray720p = "Bluray-720p",
    Hdtv1080p = "HDTV-1080p",
    WebDl1080p = "WEBDL-1080p",
    WebRip1080p = "WEBRip-1080p",
    Bluray1080p = "Bluray-1080p",
    Remux1080p = "Remux-1080p",
    Hdtv2160p = "HDTV-2160p",
    WebDl2160p = "WEBDL-2160p",
    WebRip2160p = "WEBRip-2160p",
    Bluray2160p = "Bluray-2160p",
    Remux2160p = "Remux-2160p",
    BrDisk = "BR-DISK",
    RawHd = "Raw-HD",
    Mp3Unknown = "MP3",
    Mp3_128 = "MP3-128",
    Mp3_192 = "MP3-192",
    Mp3VbrV2 = "MP3-VBR-V2",
    Aac = "AAC",
    Ogg = "OGG Vorbis",
    Mp3_256 = "MP3-256",
    Mp3VbrV0 = "MP3-VBR-V0",
    Mp3_320 = "MP3-320",
    Wav = "WAV",
    Ape = "APE",
    WavPack = "WavPack",
    Alac = "ALAC",
    Flac = "FLAC",
    Flac24 = "FLAC 24bit",
    Pdf = "PDF",
    Mobi = "MOBI",
    Azw3 = "AZW3",
    Epub = "EPUB",
    AudioMp3 = "MP3 Audiobook",
    AudioM4b = "M4B",
    AudioFlac = "FLAC Audiobook",
}

impl Quality {
    pub fn is_music(self) -> bool {
        (Self::Mp3Unknown..=Self::Flac24).contains(&self)
    }

    pub fn is_book(self) -> bool {
        self >= Self::Pdf
    }

    pub fn is_video(self) -> bool {
        (Self::WorkPrint..=Self::RawHd).contains(&self)
    }

    pub fn is_lossless(self) -> bool {
        (Self::Wav..=Self::Flac24).contains(&self) || self == Self::AudioFlac
    }

    /// Vertical resolution of a video quality.
    pub fn resolution(self) -> Option<u16> {
        let n = self.name();
        ["2160", "1080", "720", "576", "480"]
            .into_iter()
            .find(|r| n.ends_with(&format!("{r}p")))
            .and_then(|r| r.parse().ok())
    }
}

impl std::fmt::Display for Quality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Where a video came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    WorkPrint,
    Cam,
    Telesync,
    Telecine,
    Regional,
    Screener,
    Tv,
    Dvd,
    DvdR,
    WebDl,
    WebRip,
    Bluray,
}

/// Everything a name says about quality, besides titles and numbers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QualityInfo {
    pub quality: Quality,
    pub source: Option<Source>,
    pub resolution: Option<u16>,
    pub remux: bool,
    /// `H.264`, `H.265`, `AV1`, `XviD`, `DivX`, `VC-1`, `MPEG-2`, `VP9`.
    pub codec: Option<&'static str>,
    /// `DV`, `HDR10+`, `HDR10`, `HDR`, `HLG`.
    pub hdr: Vec<&'static str>,
    /// `TrueHD`, `DTS-HD MA`, `DTS:X`, `DTS`, `DD+`, `DD`, `AAC`, `FLAC`, `Opus`, `MP3`, `PCM`.
    pub audio: Option<&'static str>,
    pub atmos: bool,
    /// `5.1`, `7.1`, `2.0`.
    pub channels: Option<String>,
    pub proper: bool,
    pub repack: bool,
    /// 1 for a first release; PROPER/REPACK and `v2`-style tags raise it. Higher wins among equal qualities.
    pub version: u8,
    pub real: bool,
    pub group: Option<String>,
    /// Languages named in the release (`Multi` for MULTi/DUAL). Empty: none named (usually English or the original).
    pub languages: Vec<&'static str>,
    pub hardcoded_subs: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedMovie {
    pub title: String,
    pub year: Option<u16>,
    pub quality: QualityInfo,
}

/// An episode, several, a season pack or a whole series.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedEpisode {
    pub series: String,
    pub year: Option<u16>,
    /// First (or only) season; `None` for absolute-numbered, daily and complete-series releases.
    pub season: Option<u32>,
    /// Every season a pack covers (`S01-S03` gives 1, 2, 3); empty when not a pack.
    pub seasons: Vec<u32>,
    pub episodes: Vec<u32>,
    /// Absolute episode numbers (anime).
    pub absolute: Vec<u32>,
    /// Daily shows: (year, month, day).
    pub air_date: Option<(u16, u8, u8)>,
    /// A season pack, several, or the complete series (then `seasons` is empty).
    pub full_season: bool,
    /// Season 0 or marked as a special.
    pub special: bool,
    pub quality: QualityInfo,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedMusic {
    pub artist: String,
    pub album: String,
    pub year: Option<u16>,
    pub quality: Quality,
    /// `FLAC`, `ALAC`, `APE`, `WavPack`, `WAV`, `MP3`, `AAC`, `OGG`.
    pub format: Option<&'static str>,
    /// kbit/s, for lossy formats when named.
    pub bitrate: Option<u32>,
    pub lossless: bool,
    pub group: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedBook {
    pub author: String,
    pub title: String,
    pub year: Option<u16>,
    pub quality: Quality,
    /// `EPUB`, `MOBI`, `AZW3`, `PDF`, `M4B`, `MP3`, `FLAC`.
    pub format: Option<&'static str>,
    pub audiobook: bool,
    pub group: Option<String>,
}

// ponytail: years past 2030 are read as part of a title ("Blade Runner 2049"); move the bound when it starts to bite.
const YEARS: std::ops::RangeInclusive<u16> = 1900..=2030;

fn year_of(s: &str) -> Option<u16> {
    let s = s.trim_matches(|c: char| !c.is_ascii_digit());
    (s.len() == 4)
        .then(|| s.parse().ok())
        .flatten()
        .filter(|y| YEARS.contains(y))
}

/// File extension removed (media, archives and download files; not `.ts`, which is also the TELESYNC tag).
fn strip_ext(s: &str) -> &str {
    let s = s.trim();
    match re!(r"(?i)\.(mkv|mp4|avi|m4v|wmv|mov|webm|nzb|torrent|epub|mobi|pdf|azw3?|m4b|m4a|mp3|flac|zip|rar|7z|cbz|cbr)$").find(s) {
        Some(m) => &s[..m.start()],
        None => s,
    }
}

/// Words joined by `.`/`_` turned into spaces; single-letter runs (`S.H.I.E.L.D`) kept with their dots. A name that already has spaces keeps its dots (`J.R.R. Tolkien`).
fn clean(s: &str) -> String {
    let s = s.replace('_', " ");
    if s.trim().contains(' ') || !re!(r"\.\w").is_match(&s) {
        let out = re!(r"\s+").replace_all(&s, " ");
        return out
            .trim_matches(|c: char| c.is_whitespace() || "-–:,[({".contains(c))
            .to_string();
    }
    let mut out = String::new();
    let mut prev_single = false;
    for (i, part) in s.split('.').enumerate() {
        let single = part.chars().count() == 1 && part.chars().all(char::is_alphanumeric);
        if i > 0 {
            out.push(if prev_single && single { '.' } else { ' ' });
        }
        out.push_str(part);
        prev_single = single;
    }
    let out = re!(r"\s+").replace_all(&out, " ");
    out.trim_matches(|c: char| c.is_whitespace() || "-–:,[({".contains(c))
        .to_string()
}

/// A title part cleaned, with a trailing `(2019)`/`2019` split off as the year.
fn title_and_year(s: &str) -> (String, Option<u16>) {
    let t = clean(s.trim_end_matches(['.', ' ', '-', '(', '[']));
    if let Some(c) = re!(r"^(.+?)[\s(\[]+((?:19|20)\d{2})[)\]]?$").captures(&t)
        && let Some(y) = year_of(&c[2])
    {
        return (clean(&c[1]), Some(y));
    }
    (t, None)
}

/// Comparable title: lowercase letters and digits, `&` as "and", without "the/a/an/and/of".
pub fn normalize_title(s: &str) -> String {
    let s = s.to_lowercase().replace('&', " and ");
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !matches!(*w, "the" | "a" | "an" | "and" | "of"))
        .collect()
}

fn release_group(name: &str) -> Option<String> {
    let s = strip_ext(name);
    if let Some(c) = re!(r"^\[([^\]]+)\]").captures(s) {
        return Some(c[1].trim().to_string());
    }
    let mut s = s.trim();
    // Site tags after the group: `-GROUP[rarbg]`, `-GROUP (TGx)`.
    while let Some(m) = re!(r"\s*(\[[^\]]*\]|\([^)]*\))$").find(s) {
        if m.start() == 0 {
            break;
        }
        s = &s[..m.start()];
    }
    let c = re!(r"(?i)-([a-z0-9][a-z0-9]*)$").captures(s)?;
    let g = &c[1];
    let lower = g.to_ascii_lowercase();
    let bad = matches!(
        lower.as_str(),
        "dl" | "hd" | "ma" | "x" | "es" | "rip" | "ray" | "web" | "sub" | "dub" | "cd" | "1"
    ) || g.bytes().all(|b| b.is_ascii_digit())
        || re!(r"(?i)^(?:\d{3,4}p|x26[45]|h26[45]|flac|mp3|epub|mobi|pdf|retail|ebook)$")
            .is_match(g);
    (!bad).then(|| g.to_string())
}

fn source(n: &str) -> Option<Source> {
    let checks: [(&Regex, Source); 12] = [
        (re!(r"(?i)\b(?:workprint|wp)\b"), Source::WorkPrint),
        (re!(r"(?i)\b(?:cam|cam-?rip|hd-?cam)\b"), Source::Cam),
        (
            re!(r"(?i)\b(?:telesync|hd-?ts|pdvd|predvdrip)\b|\bTS\b"),
            Source::Telesync,
        ),
        (re!(r"(?i)\b(?:telecine|hd-?tc)\b|\bTC\b"), Source::Telecine),
        (
            re!(r"(?i)\b(?:dvd-?scr|scr|screener|bd-?scr|web-?screener)\b"),
            Source::Screener,
        ),
        (re!(r"(?i)\b(?:r5|r6)\b"), Source::Regional),
        (
            re!(
                r"(?i)\b(?:blu-?ray|bd-?rip|br-?rip|bd-?remux|bd(?:25|50|66|100)?|uhd-?bluray|bdmv|bdiso)\b"
            ),
            Source::Bluray,
        ),
        (
            re!(r"(?i)\b(?:web-?rip|web-?dl-?rip|web-?cap|webmux|hd-?rip)\b"),
            Source::WebRip,
        ),
        (
            re!(r"(?i)\b(?:web-?dl|web-?hd|web|amzn|nf|dsnp|hmax|atvp|itunes(?:hd)?|pcok|hulu)\b"),
            Source::WebDl,
        ),
        (
            re!(
                r"(?i)\b(?:hdtv(?:rip)?|pdtv|sdtv|tv-?rip|dsr(?:ip)?|sat-?rip|dth-?rip|dvb-?rip|ahdtv|uhdtv)\b"
            ),
            Source::Tv,
        ),
        (re!(r"(?i)\b(?:dvd-?r|dvd[59]|dvd-?iso)\b"), Source::DvdR),
        (
            re!(r"(?i)\b(?:dvd(?:rip|mux)?|ntsc|pal|vhs(?:rip)?)\b"),
            Source::Dvd,
        ),
    ];
    checks
        .into_iter()
        .find(|(r, _)| r.is_match(n))
        .map(|(_, s)| s)
}

fn resolution(n: &str) -> Option<u16> {
    if re!(r"(?i)\b(?:2160[pi]|4k|uhd|3840x2160)\b").is_match(n) {
        Some(2160)
    } else if re!(r"(?i)\b(?:1080[pi]|1920x1080|fhd)\b").is_match(n) {
        Some(1080)
    } else if re!(r"(?i)\b(?:720[pi]|1280x720)\b").is_match(n) {
        Some(720)
    } else if re!(r"(?i)\b576[pi]\b").is_match(n) {
        Some(576)
    } else if re!(r"(?i)\b(?:480[pi]|640x480|848x480)\b").is_match(n) {
        Some(480)
    } else {
        None
    }
}

fn first(n: &str, table: &[(&Regex, &'static str)]) -> Option<&'static str> {
    table.iter().find(|(r, _)| r.is_match(n)).map(|(_, v)| *v)
}

const LANGUAGES: &[(&str, &str)] = &[
    (r"multi|dual(?:[ .-]?audio)?", "Multi"),
    (r"english", "English"),
    (r"(?:true)?french|vff|vfq|vf2|vfi", "French"),
    (r"german", "German"),
    (r"italian|ita", "Italian"),
    (r"spanish|esp|castellano|latino|latam", "Spanish"),
    (r"portuguese|pt-?br|dublado", "Portuguese"),
    (r"russian|rus", "Russian"),
    (r"japanese|jap|jpn", "Japanese"),
    (r"korean|kor", "Korean"),
    (r"chinese|mandarin|cantonese|chs|cht", "Chinese"),
    (r"hindi", "Hindi"),
    (r"tamil", "Tamil"),
    (r"telugu", "Telugu"),
    (r"dutch|flemish", "Dutch"),
    (r"polish|pldub", "Polish"),
    (r"swedish|swe", "Swedish"),
    (r"danish", "Danish"),
    (r"norwegian", "Norwegian"),
    (r"finnish", "Finnish"),
    (r"nordic", "Nordic"),
    (r"turkish", "Turkish"),
    (r"arabic", "Arabic"),
    (r"hebrew", "Hebrew"),
    (r"czech", "Czech"),
    (r"hungarian|hun", "Hungarian"),
    (r"greek", "Greek"),
    (r"thai", "Thai"),
    (r"vietnamese", "Vietnamese"),
    (r"ukrainian|ukr", "Ukrainian"),
    (r"romanian", "Romanian"),
];

fn languages(n: &str) -> Vec<&'static str> {
    static RES: LazyLock<Vec<(Regex, &str)>> = LazyLock::new(|| {
        LANGUAGES
            .iter()
            .map(|(p, l)| (Regex::new(&format!(r"(?i)\b(?:{p})\b")).unwrap(), *l))
            .collect()
    });
    RES.iter()
        .filter(|(r, _)| r.is_match(n))
        .map(|(_, l)| *l)
        .collect()
}

fn hardcoded_subs(n: &str) -> bool {
    re!(r"(?i)\b(?:hc|hardsubs?|subbed|([a-z]+)subs?)\b")
        .captures_iter(n)
        .any(|c| {
            c.get(1).is_none_or(|p| {
                !matches!(
                    p.as_str().to_ascii_lowercase().as_str(),
                    "soft" | "multi" | "horrible" | "no"
                )
            })
        })
}

fn video_quality(i: &QualityInfo, n: &str) -> Quality {
    use Quality::*;
    if re!(r"(?i)\braw-?hd\b").is_match(n)
        || (i.codec == Some("MPEG-2") && re!(r"(?i)\b1080i\b").is_match(n))
    {
        return RawHd;
    }
    if re!(r"(?i)\b(?:bdmv|complete[ .](?:uhd[ .])?blu-?ray|bd(?:25|50|66|100)|blu-?ray[ .](?:iso|disk|disc)|bdiso)\b").is_match(n) {
        return BrDisk;
    }
    let pick = |r2160, r1080, r720, low| match i.resolution {
        Some(2160) => r2160,
        Some(1080) => r1080,
        Some(720) => r720,
        _ => low,
    };
    let old_codec = matches!(i.codec, Some("XviD" | "DivX"));
    if i.remux && matches!(i.source, None | Some(Source::Bluray)) {
        return if i.resolution == Some(2160) {
            Remux2160p
        } else {
            Remux1080p
        };
    }
    match i.source {
        Some(Source::Bluray) => match i.resolution {
            Some(576) => Bluray576p,
            Some(480) => Bluray480p,
            None if old_codec => Bluray480p,
            _ => pick(Bluray2160p, Bluray1080p, Bluray720p, Bluray720p),
        },
        Some(Source::WebDl) => pick(WebDl2160p, WebDl1080p, WebDl720p, WebDl480p),
        Some(Source::WebRip) => pick(WebRip2160p, WebRip1080p, WebRip720p, WebRip480p),
        Some(Source::Tv) => pick(Hdtv2160p, Hdtv1080p, Hdtv720p, Sdtv),
        Some(Source::Dvd) => Dvd,
        Some(Source::DvdR) => DvdR,
        Some(Source::Screener) => DvdScr,
        Some(Source::Regional) => Regional,
        Some(Source::Telecine) => Telecine,
        Some(Source::Telesync) => Telesync,
        Some(Source::Cam) => Cam,
        Some(Source::WorkPrint) => WorkPrint,
        None => match i.resolution {
            Some(480 | 576) => Sdtv,
            Some(_) => pick(Hdtv2160p, Hdtv1080p, Hdtv720p, Sdtv),
            None if old_codec => Sdtv,
            None => Unknown,
        },
    }
}

/// Quality details of a video release name (the part after the title, ideally: a language or source word inside a title is otherwise read as a tag).
pub fn parse_quality(name: &str) -> QualityInfo {
    let n = name.replace('_', " ");
    let n = n.as_str();
    let mut i = QualityInfo {
        source: source(n),
        resolution: resolution(n),
        remux: re!(r"(?i)\b(?:remux|bdremux)\b").is_match(n),
        codec: first(
            n,
            &[
                (re!(r"(?i)\b(?:x264|h\.?264|avc)\b"), "H.264"),
                (re!(r"(?i)\b(?:x265|h\.?265|hevc)\b"), "H.265"),
                (re!(r"(?i)\bav1\b"), "AV1"),
                (re!(r"(?i)\bxvid\b"), "XviD"),
                (re!(r"(?i)\bdivx\b"), "DivX"),
                (re!(r"(?i)\bvc-?1\b"), "VC-1"),
                (re!(r"(?i)\bmpeg-?2\b"), "MPEG-2"),
                (re!(r"(?i)\bvp9\b"), "VP9"),
            ],
        ),
        audio: first(
            n,
            &[
                (re!(r"(?i)\btrue-?hd"), "TrueHD"),
                (re!(r"(?i)\bdts-?x\b|\bdts:x\b"), "DTS:X"),
                (re!(r"(?i)\bdts-?hd(?:[ .-]?ma)?\b|\bdts-?ma\b"), "DTS-HD MA"),
                (re!(r"(?i)\bdts"), "DTS"),
                (re!(r"(?i)\b(?:ddp|dd\+|e-?ac-?3)"), "DD+"),
                (re!(r"(?i)\b(?:dd(?:\d|\b)|ac-?3|dolby[ .]digital)"), "DD"),
                (re!(r"(?i)\baac"), "AAC"),
                (re!(r"(?i)\bflac"), "FLAC"),
                (re!(r"(?i)\bopus\b"), "Opus"),
                (re!(r"(?i)\bmp3\b"), "MP3"),
                (re!(r"(?i)\bl?pcm\b"), "PCM"),
            ],
        ),
        atmos: re!(r"(?i)\batmos\b").is_match(n),
        channels: re!(r"(?i)(?:aac|ddp?|dd\+|e?ac-?3|dts(?:-?hd)?(?:[ .-]?ma)?|truehd|flac|opus|l?pcm|atmos)[ .-]?([1-9])[ .]([01])\b")
            .captures(n)
            .map(|c| format!("{}.{}", &c[1], &c[2])),
        proper: re!(r"(?i)\bproper\b").is_match(n),
        repack: re!(r"(?i)\b(?:repack\d?|rerip)\b").is_match(n),
        real: re!(r"\bREAL\b").is_match(n),
        group: release_group(name),
        languages: languages(n),
        hardcoded_subs: hardcoded_subs(n),
        ..QualityInfo::default()
    };
    let mut hdr = Vec::new();
    if re!(r"(?i)\b(?:dv|dovi|dolby[ .]?vision)\b").is_match(n) {
        hdr.push("DV");
    }
    if re!(r"(?i)\bhdr10(?:\+|plus)").is_match(n) {
        hdr.push("HDR10+");
    } else if re!(r"(?i)\bhdr10\b").is_match(n) {
        hdr.push("HDR10");
    } else if re!(r"(?i)\bhdr\b").is_match(n) {
        hdr.push("HDR");
    }
    if re!(r"(?i)\bhlg\b").is_match(n) {
        hdr.push("HLG");
    }
    i.hdr = hdr;
    let repack_n = re!(r"(?i)\brepack(\d)\b")
        .captures(n)
        .and_then(|c| c[1].parse::<u8>().ok());
    let anime_v = re!(r"(?i)(?:\d|\b)v([2-9])\b")
        .captures(n)
        .and_then(|c| c[1].parse::<u8>().ok());
    i.version = repack_n
        .map(|r| r + 1)
        .or(anime_v)
        .unwrap_or(if i.proper || i.repack { 2 } else { 1 });
    i.quality = video_quality(&i, n);
    i
}

/// Tokens that end a movie title (quality, edition, language and release tags).
fn stop_token(t: &str, hard_only: bool) -> bool {
    let hard = re!(
        r"(?i)^(?:\d{3,4}[pi]|4k|uhd|blu|blu-?ray|bd(?:rip|remux)?|br-?rip|web(?:-?dl|-?rip|hd)?|hdtv|pdtv|dvd(?:rip|scr|r|5|9)?|hd-?rip|cam(?:rip)?|hd-?cam|ts|tc|hd-?ts|telesync|telecine|scr|screener|r5|remux|x26[45]|h26[45]|hevc|avc|xvid|divx|av1|10-?bit|hdr(?:10)?|dv|amzn|nf|dsnp|hmax|atvp|complete)(?:-.*)?$"
    );
    let soft = re!(
        r"(?i)^(?:proper|repack|real|rerip|internal|limited|extended|unrated|uncut|directors|dc|theatrical|remastered|imax|criterion|festival|multi|dual|subbed|dubbed|hc|french|truefrench|german|italian|spanish|hindi|russian|japanese|korean|vostfr|nordic|swedish|read\.?nfo)$"
    );
    hard.is_match(t) || (!hard_only && soft.is_match(t))
}

/// Movie name: title, year, quality.
pub fn parse_movie(name: &str) -> ParsedMovie {
    let s = strip_ext(name);
    let body = re!(r"^\[[^\]]+\]\s*").replace(s, "");
    let body = body.as_ref();
    // Token start offsets in `body`.
    let toks: Vec<(usize, &str)> = re!(r"[^\s._\[\]()-]+")
        .find_iter(body)
        .map(|m| (m.start(), m.as_str()))
        .collect();
    let hard = toks
        .iter()
        .position(|(_, t)| stop_token(t, true))
        .unwrap_or(toks.len());
    let year_at = toks[..hard]
        .iter()
        .enumerate()
        .rev()
        .find(|(i, (_, t))| *i > 0 && t.len() == 4 && year_of(t).is_some())
        .map(|(i, _)| i);
    let end = year_at.unwrap_or_else(|| {
        toks.iter()
            .enumerate()
            .position(|(i, (_, t))| i > 0 && stop_token(t, false))
            .unwrap_or(toks.len())
    });
    let cut = toks.get(end).map_or(body.len(), |(o, _)| *o);
    let year = year_at.and_then(|i| year_of(toks[i].1));
    let mut q = parse_quality(&body[cut..]);
    q.group = release_group(name);
    ParsedMovie {
        title: clean(body[..cut].trim_end_matches(['.', ' ', '-', '(', '['])),
        year,
        quality: q,
    }
}

/// Numbers from an episode list like `E01E02`, `E01-E03`, `01-03`; a hyphen means a range.
fn episode_list(s: &str) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut range = false;
    for m in re!(r"\d+|-").find_iter(s) {
        if m.as_str() == "-" {
            range = true;
            continue;
        }
        let Ok(n) = m.as_str().parse::<u32>() else {
            continue;
        };
        match out.last().copied() {
            Some(prev) if range && n > prev && n - prev < 100 => out.extend(prev + 1..=n),
            Some(prev) if n <= prev => {}
            _ => out.push(n),
        }
        range = false;
    }
    out
}

fn valid_date(y: u16, m: u32, d: u32) -> Option<(u16, u8, u8)> {
    (YEARS.contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d))
        .then_some((y, m as u8, d as u8))
}

/// Episode, multi-episode, season pack, daily or absolute-numbered (anime) name. `None` if nothing in it looks like an episode or a season.
pub fn parse_episode(name: &str) -> Option<ParsedEpisode> {
    // `_` is a word character to the regex engine: make `\b` work for `Show_Name_S01E01`.
    let s = strip_ext(name).replace('_', " ");
    let anime = re!(r"^\[[^\]]+\]\s*")
        .find(&s)
        .map(|m| m.end())
        .unwrap_or(0);
    let body = &s[anime..];
    let mut e = ParsedEpisode::default();
    let mut found: Option<(usize, usize)> = None;

    if let Some(c) = re!(r"(?i)\bS(\d{1,2})(?:-S?|[ .]?(?:-|to|&)[ .]?S)(\d{1,2})\b").captures(body)
        && !re!(r"(?i)^E\d").is_match(&body[c.get(0).unwrap().end()..])
    {
        let (a, b): (u32, u32) = (c[1].parse().ok()?, c[2].parse().ok()?);
        if b > a {
            e.seasons = (a..=b).collect();
            e.season = Some(a);
            e.full_season = true;
            found = Some((c.get(0)?.start(), c.get(0)?.end()));
        }
    }
    if found.is_none()
        && let Some(c) = re!(r"(?i)\b(?:seasons?)[ ._]?(\d{1,2})[ ._]?(?:-|to|&)[ ._]?(\d{1,2})\b")
            .captures(body)
    {
        let (a, b): (u32, u32) = (c[1].parse().ok()?, c[2].parse().ok()?);
        if b > a {
            e.seasons = (a..=b).collect();
            e.season = Some(a);
            e.full_season = true;
            found = Some((c.get(0)?.start(), c.get(0)?.end()));
        }
    }
    if found.is_none()
        && let Some(c) =
            re!(r"(?i)\bS(\d{1,4})[ .-]?(E\d{1,4}(?:(?:[-. ]?E|-E?)\d{1,3})*)").captures(body)
    {
        let all = c.get(0)?;
        let mut eps = &c[2];
        let mut end = all.end();
        // `S01E01-720p`: the last number was a resolution, not an episode.
        if body[end..].starts_with(['p', 'P', 'i', 'I'])
            && let Some(m) = re!(r"(?i)[-. ]?E?\d+$").find(eps)
        {
            end -= eps.len() - m.start();
            eps = &eps[..m.start()];
        }
        e.season = c[1].parse().ok();
        e.episodes = episode_list(eps);
        found = Some((all.start(), end));
    }
    if found.is_none()
        && let Some(c) =
            re!(r"(?i)\b(\d{1,2})x(\d{2,3})((?:[-_](?:\d{1,2}x)?\d{2,3})*)\b").captures(body)
    {
        e.season = c[1].parse().ok();
        let mut eps = vec![c[2].parse().ok()?];
        let rest = re!(r"(?i)\d{1,2}x").replace_all(&c[3], "");
        eps.extend(
            episode_list(&format!("{}{}", &c[2], rest))
                .into_iter()
                .skip(1),
        );
        e.episodes = eps;
        found = Some((c.get(0)?.start(), c.get(0)?.end()));
    }
    if found.is_none()
        && let Some(c) = re!(r"\b((?:19|20)\d{2})[-. ](\d{2})[-. ](\d{2})\b").captures(body)
        && let Some(d) = valid_date(c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?)
    {
        e.air_date = Some(d);
        found = Some((c.get(0)?.start(), c.get(0)?.end()));
    }
    if found.is_none()
        && let Some(c) = re!(r"\b(\d{2})[-. ](\d{2})[-. ]((?:19|20)\d{2})\b").captures(body)
    {
        let (a, b): (u32, u32) = (c[1].parse().ok()?, c[2].parse().ok()?);
        let (m, d) = if a > 12 { (b, a) } else { (a, b) };
        if let Some(d) = valid_date(c[3].parse().ok()?, m, d) {
            e.air_date = Some(d);
            found = Some((c.get(0)?.start(), c.get(0)?.end()));
        }
    }
    if found.is_none()
        && let Some(c) = re!(r"(?i)(?:[ .]S(\d{1,2}))?[ .]-[ .](\d{1,4})(?:v\d)?(?:[ .]?-[ .]?(\d{1,4})(?:v\d)?)?(?:[ .]|$)").captures(body)
    {
        // `Title - 05`, `Title - 1085v2`, `Title - 01-12` (anime absolute numbering), `Title S2 - 05` (seasonal).
        let a: u32 = c[2].parse().ok()?;
        if anime > 0 || year_of(&c[2]).is_none() {
            let b = c.get(3).and_then(|b| b.as_str().parse::<u32>().ok()).filter(|&b| b > a);
            let nums: Vec<u32> = b.map_or(vec![a], |b| (a..=b).collect());
            match c.get(1) {
                Some(sn) => {
                    e.season = sn.as_str().parse().ok();
                    e.episodes = nums;
                }
                None => e.absolute = nums,
            }
            found = Some((c.get(0)?.start(), c.get(0)?.end()));
        }
    }
    if found.is_none()
        && let Some(c) = re!(r"(?i)\b(?:S(\d{1,2})|Season[ ._]?(\d{1,2}))\b").captures(body)
    {
        e.season = c.get(1).or(c.get(2))?.as_str().parse().ok();
        e.seasons = e.season.into_iter().collect();
        e.full_season = true;
        found = Some((c.get(0)?.start(), c.get(0)?.end()));
    }
    if found.is_none()
        && let Some(c) = re!(r"(?i)[ .](\d)(\d{2})[ .](?:hdtv|pdtv|dsr|web|xvid|x264|720p|1080p|internal|proper|repack)\b").captures(body)
    {
        e.season = c[1].parse().ok();
        e.episodes = vec![c[2].parse().ok()?];
        found = Some((c.get(0)?.start(), c.get(0)?.end()));
    }
    if found.is_none()
        && let Some(c) =
            re!(r"(?i)\b(?:the[ ._])?complete[ ._](?:series|collection)\b").captures(body)
    {
        e.full_season = true;
        found = Some((c.get(0)?.start(), c.get(0)?.end()));
    }
    let (start, end) = found?;
    let (series, year) = title_and_year(&body[..start]);
    e.series = series;
    e.year = year;
    e.special = e.season == Some(0)
        || e.episodes.contains(&0)
        || re!(r"(?i)\b(?:special|specials|sp\d{0,2}|ova|oad)\b").is_match(&body[end..]);
    let mut q = parse_quality(&body[end..]);
    q.group = release_group(name);
    e.quality = q;
    Some(e)
}

fn music_quality(n: &str) -> (Quality, Option<&'static str>, Option<u32>) {
    use Quality::*;
    let format = first(
        n,
        &[
            (re!(r"(?i)\bflac\b"), "FLAC"),
            (re!(r"(?i)\balac\b"), "ALAC"),
            (re!(r"(?i)\bape\b"), "APE"),
            (re!(r"(?i)\b(?:wv|wavpack)\b"), "WavPack"),
            (re!(r"(?i)\bwav\b"), "WAV"),
            (re!(r"(?i)\bmp3\b"), "MP3"),
            (re!(r"(?i)\b(?:aac|m4a)\b"), "AAC"),
            (re!(r"(?i)\b(?:ogg|vorbis|opus)\b"), "OGG"),
            (re!(r"(?i)\blossless\b"), "FLAC"),
        ],
    );
    let bitrate = re!(r"(?i)\b(320|256|224|192|160|128|96)[ ]?(?:kbps|kbit/?s?|k)?\b")
        .captures(n)
        .and_then(|c| c[1].parse().ok());
    let v0 = re!(r"(?i)\bv0\b").is_match(n);
    let v2 = re!(r"(?i)\bv2\b").is_match(n);
    let hires =
        re!(r"(?i)\b24[ -]?bits?\b|\b24[-/ ](?:44|48|88|96|176|192)|\bhi-?res\b").is_match(n);
    let format = format.or((bitrate.is_some() || v0 || v2).then_some("MP3"));
    let q = match format {
        Some("FLAC") if hires => Flac24,
        Some("FLAC") => Flac,
        Some("ALAC") => Alac,
        Some("APE") => Ape,
        Some("WavPack") => WavPack,
        Some("WAV") => Wav,
        Some("AAC") => Aac,
        Some("OGG") => Ogg,
        Some("MP3") if v0 => Mp3VbrV0,
        Some("MP3") if v2 => Mp3VbrV2,
        Some("MP3") => match bitrate {
            Some(b) if b >= 320 => Mp3_320,
            Some(b) if b >= 256 => Mp3_256,
            Some(b) if b >= 192 => Mp3_192,
            Some(_) => Mp3_128,
            None => Mp3Unknown,
        },
        _ => Unknown,
    };
    (q, format, bitrate.filter(|_| !q.is_lossless()))
}

/// Strip bracketed tags and trailing year/format words from a music or book part.
fn strip_tags(s: &str) -> String {
    let s = re!(r"\[[^\]]*\]|\{[^}]*\}").replace_all(s, " ");
    let s = re!(r"(?i)\((?:\d{4}|[^)]*(?:remaster|deluxe|edition|unabridged|abridged|retail|ebook|audiobook|flac|mp3|epub|mobi|pdf|azw3|m4b|web|cd|vinyl)[^)]*)\)").replace_all(&s, " ");
    clean(&s)
}

fn year_in(s: &str) -> Option<u16> {
    re!(r"\b(?:1[5-9]|20)\d{2}\b").find_iter(s).find_map(|m| {
        m.as_str()
            .parse()
            .ok()
            .filter(|y| (1500..=*YEARS.end()).contains(y))
    })
}

/// Music name: `Artist - Album (2020) [FLAC 24bit]` or scene `Artist-Album-WEB-FLAC-2020-GROUP`.
pub fn parse_music(name: &str) -> ParsedMusic {
    let s = strip_ext(name);
    let (q, format, bitrate) = music_quality(&s.replace('_', " "));
    let scene = !s.contains(' ') && s.contains('-');
    let (artist, rest) = if scene {
        let parts: Vec<&str> = s.split('-').collect();
        (
            parts[0].to_string(),
            parts.get(1).copied().unwrap_or_default().to_string(),
        )
    } else {
        let mut parts = s.splitn(3, " - ");
        let a = parts.next().unwrap_or_default().to_string();
        let b = parts.next().unwrap_or_default();
        // `Artist - 1999 - Album`
        let b = match (year_of(b.trim()), parts.next()) {
            (Some(_), Some(c)) => c.to_string(),
            (_, c) => c.map_or(b.to_string(), |c| format!("{b} - {c}")),
        };
        let b = b.split(" - ").next().unwrap_or_default().to_string();
        (a, b)
    };
    let album = strip_tags(&rest);
    let album = re!(r"(?i)[ ]*(?:\b(?:19|20)\d{2}\b.*|\b(?:web|cd|flac|mp3|vinyl|lp)\b.*)$")
        .replace(&album, "")
        .trim()
        .to_string();
    ParsedMusic {
        artist: strip_tags(&artist),
        album: if album.is_empty() {
            strip_tags(&rest)
        } else {
            album
        },
        year: year_in(s),
        quality: q,
        format,
        bitrate,
        lossless: q.is_lossless(),
        group: if scene { release_group(s) } else { None },
    }
}

/// Book or audiobook name: `Author - Title (2019) [epub]`, `Title by Author [m4b]` or scene `Author.Name-Title-RETAIL-EPUB-eBOOK-GROUP`.
pub fn parse_book(name: &str) -> ParsedBook {
    use Quality::*;
    let raw = name.trim();
    let s = strip_ext(raw);
    let ext = raw[s.len()..].trim_start_matches('.').to_ascii_lowercase();
    let n = format!("{} {ext}", s.replace('_', " "));
    let format = first(
        &n,
        &[
            (re!(r"(?i)\bm4b\b"), "M4B"),
            (re!(r"(?i)\bepub\b"), "EPUB"),
            (re!(r"(?i)\bazw3?\b|\bkfx\b|\bkf8\b"), "AZW3"),
            (re!(r"(?i)\bmobi\b"), "MOBI"),
            (re!(r"(?i)\bpdf\b"), "PDF"),
            (re!(r"(?i)\bflac\b"), "FLAC"),
            (re!(r"(?i)\bmp3\b"), "MP3"),
        ],
    );
    let audio_word = re!(r"(?i)\b(?:audio-?books?|unabridged|abridged|narrated)\b").is_match(&n);
    let audiobook = audio_word || matches!(format, Some("M4B" | "MP3" | "FLAC"));
    let quality = match format {
        Some("EPUB") => Epub,
        Some("AZW3") => Azw3,
        Some("MOBI") => Mobi,
        Some("PDF") => Pdf,
        Some("M4B") => AudioM4b,
        Some("FLAC") => AudioFlac,
        Some("MP3") => AudioMp3,
        _ if audiobook => AudioMp3,
        _ => Unknown,
    };
    let scene = !s.contains(' ') && s.contains('-');
    let (author, title) = if scene {
        let p: Vec<&str> = s.split('-').collect();
        (
            p[0].to_string(),
            p.get(1).copied().unwrap_or_default().to_string(),
        )
    } else if let Some((a, t)) = s.split_once(" - ") {
        // `Author - Series 01 - Title`: the last part is the title.
        (
            a.to_string(),
            t.rsplit(" - ").next().unwrap_or(t).to_string(),
        )
    } else if let Some(c) = re!(r"(?i)^(.+?)\s+by\s+(.+)$").captures(s) {
        (c[2].to_string(), c[1].to_string())
    } else {
        (String::new(), s.to_string())
    };
    let tail = re!(
        r"(?i)\b(?:retail|ebook|epub|mobi|azw3?|pdf|m4b|mp3|flac|audiobook|unabridged|abridged)\b.*$"
    );
    let title = strip_tags(&title);
    let title = tail.replace(&title, "").trim().to_string();
    ParsedBook {
        author: strip_tags(&tail.replace(&author, "")),
        title: re!(r"\s+(?:19|20)\d{2}$").replace(&title, "").to_string(),
        year: year_in(s),
        quality,
        format,
        audiobook,
        group: if scene { release_group(s) } else { None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_and_families() {
        assert!(Quality::WebDl1080p > Quality::Hdtv1080p);
        assert!(Quality::Remux2160p > Quality::Bluray2160p);
        assert!(Quality::Flac24 > Quality::Mp3_320);
        assert!(Quality::Epub.is_book() && !Quality::Epub.is_music());
        assert!(Quality::Mp3VbrV0.is_music() && !Quality::Mp3VbrV0.is_video());
        assert!(Quality::Bluray720p.is_video());
        assert_eq!(Quality::WebRip2160p.resolution(), Some(2160));
        assert_eq!(Quality::Dvd.resolution(), None);
        assert_eq!(Quality::ALL.len(), Quality::AudioFlac as usize + 1);
    }

    #[test]
    fn helpers() {
        assert_eq!(
            clean("Marvels.Agents.of.S.H.I.E.L.D"),
            "Marvels Agents of S.H.I.E.L.D"
        );
        assert_eq!(episode_list("E01-E03"), vec![1, 2, 3]);
        assert_eq!(episode_list("E01E02"), vec![1, 2]);
        assert_eq!(
            normalize_title("The Lord of the Rings & Co."),
            "lordringsco"
        );
    }
}
