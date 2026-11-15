//! Read-only models for the player: media streams, trickplay info, media segments.
use serde::Deserialize;
use std::collections::HashMap;

/// One audio/video/subtitle stream of a media source (`Type` is "Video", "Audio", "Subtitle").
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct MediaStream {
    pub index: i64,
    #[serde(rename = "Type")]
    pub kind: String,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub display_title: Option<String>,
    pub is_default: bool,
    pub is_forced: bool,
    pub is_external: bool,
    pub is_text_subtitle_stream: bool,
    pub channels: Option<u32>,
    pub height: Option<u32>,
    pub bit_rate: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct MediaSource {
    pub id: String,
    pub container: Option<String>,
    pub bitrate: Option<u64>,
    pub media_streams: Vec<MediaStream>,
}

/// Tile-sheet geometry for one trickplay width (`Interval` is milliseconds per thumbnail).
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct TrickplayInfo {
    pub width: u32,
    pub height: u32,
    pub tile_width: u32,
    pub tile_height: u32,
    pub thumbnail_count: u32,
    pub interval: u32,
}

/// What the player needs from `GET /Items/{id}` beyond the browse fields.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct PlayItem {
    pub media_sources: Vec<MediaSource>,
    /// media source id -> width -> info.
    pub trickplay: HashMap<String, HashMap<String, TrickplayInfo>>,
}

impl PlayItem {
    /// Trickplay of the first source that has any: the widest sheet no wider than `max_w`
    /// (else the narrowest), with the source id the tiles are requested under.
    pub fn trickplay_for(&self, max_w: u32) -> Option<(String, TrickplayInfo)> {
        let src = self.media_sources.first().map(|s| s.id.as_str())?;
        let infos: Vec<TrickplayInfo> = self.trickplay.get(src)?.values().copied().collect();
        let pick = infos
            .iter()
            .filter(|i| i.width <= max_w)
            .max_by_key(|i| i.width)
            .or_else(|| infos.iter().min_by_key(|i| i.width))?;
        Some((src.to_string(), *pick))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    Intro,
    Outro,
    Recap,
    Preview,
    Commercial,
    Unknown,
}

/// A skippable range of an item, in 100 ns ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub kind: SegmentKind,
    pub start_ticks: i64,
    pub end_ticks: i64,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub(crate) struct RawSegments {
    pub items: Vec<RawSegment>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub(crate) struct RawSegment {
    #[serde(rename = "Type")]
    kind: String,
    start_ticks: i64,
    end_ticks: i64,
}

impl RawSegments {
    pub(crate) fn into_segments(self) -> Vec<Segment> {
        self.items
            .into_iter()
            .map(|s| Segment {
                kind: match s.kind.as_str() {
                    "Intro" => SegmentKind::Intro,
                    "Outro" => SegmentKind::Outro,
                    "Recap" => SegmentKind::Recap,
                    "Preview" => SegmentKind::Preview,
                    "Commercial" => SegmentKind::Commercial,
                    _ => SegmentKind::Unknown,
                },
                start_ticks: s.start_ticks,
                end_ticks: s.end_ticks,
            })
            .filter(|s| s.end_ticks > s.start_ticks)
            .collect()
    }
}

/// Parameters of a Jellyfin HLS transcode request. `None` caps are left to the server.
#[derive(Debug, Clone, Default)]
pub struct Transcode {
    pub source_id: String,
    pub video_bitrate: Option<u64>,
    pub max_height: Option<u32>,
    pub audio_bitrate: Option<u64>,
    pub max_audio_channels: Option<u32>,
    /// Let the server remux instead of re-encoding when the source already fits the caps.
    pub allow_copy: bool,
    pub play_session: String,
}

impl Transcode {
    pub(crate) fn pairs(&self) -> Vec<(&'static str, String)> {
        let mut p = vec![
            ("MediaSourceId", self.source_id.clone()),
            ("VideoCodec", "h264".into()),
            ("AudioCodec", "aac".into()),
            ("SegmentContainer", "ts".into()),
            ("MinSegments", "1".into()),
            ("BreakOnNonKeyFrames", "true".into()),
            ("PlaySessionId", self.play_session.clone()),
        ];
        if let Some(b) = self.video_bitrate {
            p.push(("VideoBitrate", b.to_string()));
        }
        if let Some(h) = self.max_height {
            p.push(("MaxHeight", h.to_string()));
        }
        if let Some(b) = self.audio_bitrate {
            p.push(("AudioBitrate", b.to_string()));
        }
        if let Some(c) = self.max_audio_channels {
            p.push(("TranscodingMaxAudioChannels", c.to_string()));
        }
        if self.allow_copy {
            p.push(("AllowVideoStreamCopy", "true".into()));
            p.push(("AllowAudioStreamCopy", "true".into()));
        }
        p
    }
}
