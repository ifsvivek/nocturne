//! Optional `ffprobe` probe for runtime and streams. Never required; the caller decides when to call it.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Stdio},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StreamInfo {
    pub kind: String,
    pub codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub channels: Option<u32>,
    pub language: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Probe {
    pub runtime_secs: Option<f64>,
    pub streams: Vec<StreamInfo>,
}

pub fn available() -> bool {
    Command::new("ffprobe")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Run ffprobe on a media file (read-only). `None` if ffprobe is missing or fails.
pub fn probe(path: &Path) -> Option<Probe> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg("--")
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse(json: &str) -> Option<Probe> {
    let v: Value = serde_json::from_str(json).ok()?;
    let runtime_secs = v["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse().ok());
    let streams = v["streams"]
        .as_array()?
        .iter()
        .map(|s| StreamInfo {
            kind: s["codec_type"].as_str().unwrap_or("").into(),
            codec: s["codec_name"].as_str().map(Into::into),
            width: s["width"].as_u64().map(|n| n as u32),
            height: s["height"].as_u64().map(|n| n as u32),
            channels: s["channels"].as_u64().map(|n| n as u32),
            language: s["tags"]["language"].as_str().map(Into::into),
        })
        .collect();
    Some(Probe {
        runtime_secs,
        streams,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_ffprobe_json() {
        let p = super::parse(
            r#"{"streams":[{"codec_type":"video","codec_name":"h264","width":1920,"height":1080},
            {"codec_type":"audio","codec_name":"aac","channels":2,"tags":{"language":"eng"}}],
            "format":{"duration":"5400.5"}}"#,
        )
        .unwrap();
        assert_eq!(p.runtime_secs, Some(5400.5));
        assert_eq!(p.streams.len(), 2);
        assert_eq!(p.streams[1].language.as_deref(), Some("eng"));
    }
}
