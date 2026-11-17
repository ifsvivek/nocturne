//! Remote control from other Jellyfin apps: what the server pushes over the session WebSocket
//! (`Play`, `Playstate`, `GeneralCommand`) mapped to a small command set. Pure; the app carries
//! the commands out on its player. Shapes follow `PlayRequest`, `PlaystateRequest` and
//! `GeneralCommand` in the Jellyfin source. Commands never write anything or ask for
//! confirmation; anything not listed here is ignored.
use crate::video::{Track, TrackKind};
use nocturne_jellyfin as jf;
use serde_json::{Value, json};

/// The `GeneralCommandType` names this app implements (posted as `SupportedCommands`).
pub const SUPPORTED_COMMANDS: [&str; 7] = [
    "SetVolume",
    "Mute",
    "Unmute",
    "ToggleMute",
    "SetAudioStreamIndex",
    "SetSubtitleStreamIndex",
    "DisplayMessage",
];

/// Body of `POST /Sessions/Capabilities/Full` (`ClientCapabilitiesDto`).
pub fn capabilities(enabled: bool) -> Value {
    json!({
        "PlayableMediaTypes": if enabled { json!(["Video", "Audio"]) } else { json!([]) },
        "SupportedCommands": if enabled { json!(SUPPORTED_COMMANDS) } else { json!([]) },
        "SupportsMediaControl": enabled,
        "SupportsPersistentIdentifier": true,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemoteCmd {
    /// Start these items (in order) from `start_index`; `PlayNext`/`PlayLast` only queue.
    Play {
        ids: Vec<String>,
        start_index: usize,
        start_secs: f64,
        queue_only: bool,
    },
    PlayPause,
    Pause,
    Unpause,
    Stop,
    SeekTo(f64),
    SeekBy(f64),
    Next,
    Previous,
    /// 0 to 100.
    Volume(f64),
    Mute(Option<bool>),
    /// Jellyfin stream index; -1 = none (subtitles).
    Audio(i64),
    Subtitle(i64),
    Message(String),
}

fn ticks(v: &Value) -> Option<f64> {
    v.as_i64().map(|t| t as f64 / 1e7)
}

/// Arguments arrive as strings ("50"), occasionally as numbers.
fn arg_num(a: &Value, k: &str) -> Option<f64> {
    match &a[k] {
        Value::String(s) => s.trim().parse().ok(),
        v => v.as_f64(),
    }
}

/// One socket frame (`MessageType` and its `Data`); `None` for anything unsupported or malformed.
pub fn parse(message_type: &str, d: &Value) -> Option<RemoteCmd> {
    Some(match message_type {
        "Play" => {
            let ids: Vec<String> = d["ItemIds"]
                .as_array()?
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .take(200)
                .collect();
            if ids.is_empty() {
                return None;
            }
            RemoteCmd::Play {
                start_index: (d["StartIndex"].as_i64().unwrap_or(0).max(0) as usize)
                    .min(ids.len() - 1),
                ids,
                start_secs: ticks(&d["StartPositionTicks"]).unwrap_or(0.0).max(0.0),
                queue_only: matches!(d["PlayCommand"].as_str(), Some("PlayNext" | "PlayLast")),
            }
        }
        "Playstate" => match d["Command"].as_str()? {
            "PlayPause" => RemoteCmd::PlayPause,
            "Pause" => RemoteCmd::Pause,
            "Unpause" => RemoteCmd::Unpause,
            "Stop" => RemoteCmd::Stop,
            "Seek" => RemoteCmd::SeekTo(ticks(&d["SeekPositionTicks"])?.max(0.0)),
            "Rewind" => RemoteCmd::SeekBy(-10.0),
            "FastForward" => RemoteCmd::SeekBy(30.0),
            "NextTrack" => RemoteCmd::Next,
            "PreviousTrack" => RemoteCmd::Previous,
            _ => return None,
        },
        "GeneralCommand" => {
            let a = &d["Arguments"];
            match d["Name"].as_str()? {
                "SetVolume" => RemoteCmd::Volume(arg_num(a, "Volume")?.clamp(0.0, 100.0)),
                "Mute" => RemoteCmd::Mute(Some(true)),
                "Unmute" => RemoteCmd::Mute(Some(false)),
                "ToggleMute" => RemoteCmd::Mute(None),
                "SetAudioStreamIndex" => RemoteCmd::Audio(arg_num(a, "Index")? as i64),
                "SetSubtitleStreamIndex" => RemoteCmd::Subtitle(arg_num(a, "Index")? as i64),
                "DisplayMessage" => {
                    let t = a["Text"].as_str()?.trim();
                    let h = a["Header"].as_str().unwrap_or("").trim();
                    let text = match (h.is_empty(), t.is_empty()) {
                        (_, true) => return None,
                        (true, false) => t.to_string(),
                        (false, false) => format!("{h}: {t}"),
                    };
                    RemoteCmd::Message(text.chars().take(200).collect())
                }
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// mpv track id for a Jellyfin stream index. Jellyfin numbers all streams of a file together;
/// mpv lists each kind from 1. Like `name_tracks`, it only maps when both sides agree on the
/// number of embedded tracks (a transcode may not). `-1` is "off" and always maps (to 0).
pub fn mpv_track(
    index: i64,
    kind: TrackKind,
    streams: &[jf::MediaStream],
    tracks: &[Track],
) -> Option<i64> {
    if index < 0 {
        return (kind == TrackKind::Sub).then_some(0);
    }
    let jk = if kind == TrackKind::Audio {
        "Audio"
    } else {
        "Subtitle"
    };
    let embedded: Vec<_> = streams
        .iter()
        .filter(|s| s.kind == jk && !s.is_external)
        .collect();
    let mine: Vec<&Track> = tracks
        .iter()
        .filter(|t| t.kind == kind && !t.external)
        .collect();
    if embedded.len() != mine.len() {
        return None;
    }
    let at = embedded.iter().position(|s| s.index == index)?;
    Some(mine[at].id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Option<RemoteCmd> {
        let v: Value = serde_json::from_str(s).unwrap();
        parse(v["MessageType"].as_str().unwrap(), &v["Data"])
    }

    #[test]
    fn play_request() {
        let c = p(
            r#"{"MessageType":"Play","Data":{"ItemIds":["a1","b2","c3"],"StartPositionTicks":600000000,
            "PlayCommand":"PlayNow","StartIndex":1,"AudioStreamIndex":2,
            "ControllingUserId":"u"}}"#,
        );
        assert_eq!(
            c,
            Some(RemoteCmd::Play {
                ids: vec!["a1".into(), "b2".into(), "c3".into()],
                start_index: 1,
                start_secs: 60.0,
                queue_only: false,
            })
        );
        let q = p(
            r#"{"MessageType":"Play","Data":{"ItemIds":["x"],"PlayCommand":"PlayLast","StartIndex":9}}"#,
        );
        assert!(matches!(
            q,
            Some(RemoteCmd::Play {
                queue_only: true,
                start_index: 0,
                ..
            })
        ));
        assert_eq!(p(r#"{"MessageType":"Play","Data":{"ItemIds":[]}}"#), None);
    }

    #[test]
    fn playstate() {
        let s = |c: &str| {
            p(&format!(
                r#"{{"MessageType":"Playstate","Data":{{"Command":"{c}"}}}}"#
            ))
        };
        assert_eq!(s("PlayPause"), Some(RemoteCmd::PlayPause));
        assert_eq!(s("Pause"), Some(RemoteCmd::Pause));
        assert_eq!(s("Unpause"), Some(RemoteCmd::Unpause));
        assert_eq!(s("Stop"), Some(RemoteCmd::Stop));
        assert_eq!(s("NextTrack"), Some(RemoteCmd::Next));
        assert_eq!(s("PreviousTrack"), Some(RemoteCmd::Previous));
        assert_eq!(s("Rewind"), Some(RemoteCmd::SeekBy(-10.0)));
        assert_eq!(s("Seek"), None, "a seek needs a position");
        assert_eq!(s("Bogus"), None);
        let seek = p(
            r#"{"MessageType":"Playstate","Data":{"Command":"Seek","SeekPositionTicks":1200000000}}"#,
        );
        assert_eq!(seek, Some(RemoteCmd::SeekTo(120.0)));
    }

    #[test]
    fn general_commands() {
        let g = |n: &str, a: &str| {
            p(&format!(
                r#"{{"MessageType":"GeneralCommand","Data":{{"Name":"{n}","Arguments":{a}}}}}"#
            ))
        };
        assert_eq!(
            g("SetVolume", r#"{"Volume":"35"}"#),
            Some(RemoteCmd::Volume(35.0))
        );
        assert_eq!(
            g("SetVolume", r#"{"Volume":"500"}"#),
            Some(RemoteCmd::Volume(100.0))
        );
        assert_eq!(g("SetVolume", "{}"), None);
        assert_eq!(g("Mute", "{}"), Some(RemoteCmd::Mute(Some(true))));
        assert_eq!(g("Unmute", "{}"), Some(RemoteCmd::Mute(Some(false))));
        assert_eq!(g("ToggleMute", "{}"), Some(RemoteCmd::Mute(None)));
        assert_eq!(
            g("SetAudioStreamIndex", r#"{"Index":"3"}"#),
            Some(RemoteCmd::Audio(3))
        );
        assert_eq!(
            g("SetSubtitleStreamIndex", r#"{"Index":"-1"}"#),
            Some(RemoteCmd::Subtitle(-1))
        );
        assert_eq!(
            g(
                "DisplayMessage",
                r#"{"Header":"Hi","Text":"Dinner","TimeoutMs":"5000"}"#
            ),
            Some(RemoteCmd::Message("Hi: Dinner".into()))
        );
        assert_eq!(g("DisplayMessage", r#"{"Text":""}"#), None);
        // Things we do not implement (and anything that could change data) are dropped.
        assert_eq!(g("Shutdown", "{}"), None);
        assert_eq!(g("SetRepeatMode", r#"{"RepeatMode":"RepeatAll"}"#), None);
        assert_eq!(p(r#"{"MessageType":"Sessions","Data":[]}"#), None);
    }

    #[test]
    fn capabilities_follow_the_switch() {
        assert_eq!(capabilities(true)["SupportsMediaControl"], true);
        assert_eq!(capabilities(true)["SupportedCommands"][0], "SetVolume");
        assert_eq!(capabilities(false)["SupportsMediaControl"], false);
        assert_eq!(capabilities(false)["SupportedCommands"], json!([]));
    }

    fn stream(kind: &str, index: i64, ext: bool) -> jf::MediaStream {
        jf::MediaStream {
            kind: kind.into(),
            index,
            is_external: ext,
            ..Default::default()
        }
    }
    fn track(id: i64, kind: TrackKind) -> Track {
        Track {
            id,
            kind,
            lang: String::new(),
            title: String::new(),
            codec: String::new(),
            channels: None,
            default: false,
            forced: false,
            external: false,
            selected: false,
            secondary: false,
            name: None,
        }
    }

    #[test]
    fn stream_index_to_track() {
        // Jellyfin: 0 video, 1 audio, 2 audio, 3 subtitle. mpv: audio 1, 2; sub 1.
        let streams = [
            stream("Video", 0, false),
            stream("Audio", 1, false),
            stream("Audio", 2, false),
            stream("Subtitle", 3, false),
        ];
        let tracks = [
            track(1, TrackKind::Audio),
            track(2, TrackKind::Audio),
            track(1, TrackKind::Sub),
        ];
        assert_eq!(mpv_track(2, TrackKind::Audio, &streams, &tracks), Some(2));
        assert_eq!(mpv_track(3, TrackKind::Sub, &streams, &tracks), Some(1));
        assert_eq!(mpv_track(-1, TrackKind::Sub, &streams, &tracks), Some(0));
        assert_eq!(mpv_track(-1, TrackKind::Audio, &streams, &tracks), None);
        assert_eq!(mpv_track(9, TrackKind::Audio, &streams, &tracks), None);
        assert_eq!(
            mpv_track(2, TrackKind::Audio, &streams, &tracks[..1]),
            None,
            "counts disagree"
        );
    }
}
