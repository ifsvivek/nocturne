//! HDR playback rules (docs/HDR.md): what counts as HDR, whether the display path can show it, and
//! what to do when an HDR video starts (ask, play in the window in SDR, or open the HDR pop-out).
//! Pure, so it is unit-tested without mpv or a window.
use serde::{Deserialize, Serialize};

/// The `HDR video` setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HdrMode {
    #[default]
    Ask,
    /// Always play in the app window, tone-mapped to SDR.
    Window,
    /// Always open the HDR pop-out window.
    Popout,
}

impl HdrMode {
    pub const ALL: [HdrMode; 3] = [HdrMode::Ask, HdrMode::Window, HdrMode::Popout];

    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::Window => "Play in the window in SDR",
            Self::Popout => "Open in HDR pop-out window",
        }
    }
}

/// HDR from mpv's `video-params/gamma` and `video-params/primaries` (the source's, not the
/// output's): a PQ or HLG transfer is HDR. BT.2020 primaries alone count only when the transfer
/// is unknown; BT.2020 with an SDR transfer is a wide-gamut SDR video, which tone-maps fine.
pub fn is_hdr(gamma: &str, primaries: &str) -> bool {
    match gamma {
        "pq" | "hlg" => true,
        "" => primaries == "bt.2020",
        _ => false,
    }
}

/// mpv's output side: `video-target-params/gamma` is PQ/HLG only when the window really got an HDR
/// target (compositor colour management and `target-colorspace-hint`).
pub fn target_is_hdr(gamma: &str) -> bool {
    matches!(gamma, "pq" | "hlg")
}

/// `mpv-version` ("mpv v0.41.0-dirty") as (major, minor).
fn mpv_version(v: &str) -> Option<(u32, u32)> {
    let v = v.trim().strip_prefix("mpv")?.trim().trim_start_matches('v');
    let mut it = v.split(|c: char| !c.is_ascii_digit());
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    Some((major, minor))
}

/// mpv talks to the Wayland colour-management protocol from 0.42 on.
pub fn mpv_can_signal_hdr(version: &str) -> bool {
    mpv_version(version).is_some_and(|v| v >= (0, 42))
}

/// Can the display path show HDR? Needs the compositor's colour-management protocol and an mpv
/// that uses it. Detected, never guessed; the pop-out re-checks with mpv's own target report.
pub fn display_ok(compositor_colour_management: bool, mpv_version_str: &str) -> bool {
    compositor_colour_management && mpv_can_signal_hdr(mpv_version_str)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Window,
    Popout,
    /// Ask the viewer; `capable`: the display can show HDR (otherwise the prompt says so).
    Prompt {
        capable: bool,
    },
}

/// What to do when an HDR video starts. SyncPlay never waits on a question; an SDR-only display
/// never gets a pop-out without being told it would also be SDR.
pub fn decide(mode: HdrMode, group: bool, capable: bool) -> Decision {
    match (mode, group) {
        (_, true) | (HdrMode::Window, _) => Decision::Window,
        (HdrMode::Popout, _) if capable => Decision::Popout,
        _ => Decision::Prompt { capable },
    }
}

pub const PROMPT_TITLE: &str = "This video is HDR";
pub const PROMPT_BODY: &str =
    "Video inside the NOCTURNE window is shown in SDR (a limitation of mpv's embedding API).";
pub const PROMPT_NO_DISPLAY: &str = "This display does not report HDR, so the pop-out would be SDR too. Playing here is recommended.";

/// State of the open prompt: which button is lit and the "Remember my choice" box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub capable: bool,
    /// 0 = Play here in SDR, 1 = Open in HDR pop-out window.
    pub sel: usize,
    pub remember: bool,
    /// Playback was paused by the viewer before the question (so it stays paused afterwards).
    pub was_paused: bool,
}

impl Prompt {
    pub fn new(capable: bool, was_paused: bool) -> Self {
        Self {
            capable,
            // The recommended answer is lit: the pop-out only when it can really show HDR.
            sel: usize::from(capable),
            remember: false,
            was_paused,
        }
    }

    /// Left / Right / Tab move between the two buttons; `r` toggles remember. `None`: not our key.
    pub fn key(&mut self, name: &str) -> bool {
        match name {
            "left" => self.sel = 0,
            "right" => self.sel = 1,
            "tab" => self.sel = 1 - self.sel,
            "r" => self.remember ^= true,
            _ => return false,
        }
        true
    }

    /// The setting a remembered answer turns into (`None` when the box is not ticked).
    pub fn remembered(&self, popout: bool) -> Option<HdrMode> {
        self.remember.then_some(if popout {
            HdrMode::Popout
        } else {
            HdrMode::Window
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_hdr_from_transfer_then_primaries() {
        assert!(is_hdr("pq", "bt.2020"));
        assert!(is_hdr("hlg", "bt.2020"));
        assert!(is_hdr("pq", ""));
        assert!(!is_hdr("bt.1886", "bt.2020"), "wide-gamut SDR");
        assert!(!is_hdr("bt.1886", "bt.709"));
        assert!(!is_hdr("", "bt.709"));
        assert!(is_hdr("", "bt.2020"), "transfer unknown, wide gamut");
        assert!(target_is_hdr("pq") && !target_is_hdr("bt.1886") && !target_is_hdr(""));
    }

    #[test]
    fn mpv_version_gate() {
        assert!(!mpv_can_signal_hdr("mpv v0.41.0-dirty"));
        assert!(mpv_can_signal_hdr("mpv v0.42.0"));
        assert!(mpv_can_signal_hdr("mpv v0.101.2"));
        assert!(mpv_can_signal_hdr("mpv v1.0.0"));
        assert!(!mpv_can_signal_hdr("mpv-git"));
        assert!(!mpv_can_signal_hdr(""));
        assert!(display_ok(true, "mpv v0.42.0"));
        assert!(!display_ok(false, "mpv v0.42.0"));
        assert!(!display_ok(true, "mpv v0.41.0"));
    }

    #[test]
    fn decisions() {
        use Decision as D;
        use HdrMode as M;
        assert_eq!(decide(M::Ask, false, true), D::Prompt { capable: true });
        assert_eq!(decide(M::Ask, false, false), D::Prompt { capable: false });
        assert_eq!(decide(M::Window, false, true), D::Window);
        assert_eq!(decide(M::Popout, false, true), D::Popout);
        assert_eq!(
            decide(M::Popout, false, false),
            D::Prompt { capable: false },
            "an SDR display is told, not silently given an SDR pop-out"
        );
        for m in HdrMode::ALL {
            assert_eq!(decide(m, true, true), D::Window, "SyncPlay never waits");
        }
    }

    #[test]
    fn prompt_keys_and_remember() {
        let mut p = Prompt::new(true, false);
        assert_eq!(p.sel, 1, "capable: pop-out is the lit answer");
        assert_eq!(Prompt::new(false, false).sel, 0, "incapable: play here");
        assert!(p.key("left") && p.sel == 0);
        assert!(p.key("tab") && p.sel == 1);
        assert!(!p.key("x"));
        assert_eq!(p.remembered(true), None);
        assert!(p.key("r"));
        assert_eq!(p.remembered(true), Some(HdrMode::Popout));
        assert_eq!(p.remembered(false), Some(HdrMode::Window));
    }

    #[test]
    fn setting_round_trips() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct W {
            m: HdrMode,
        }
        let s = toml::to_string(&W { m: HdrMode::Popout }).unwrap();
        assert_eq!(s.trim(), "m = \"popout\"");
        assert_eq!(toml::from_str::<W>(&s).unwrap().m, HdrMode::Popout);
    }
}
