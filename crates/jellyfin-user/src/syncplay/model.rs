//! SyncPlay wire types and message decoding. Shapes verified against the Jellyfin 12.x OpenAPI
//! document and live traffic (see docs/syncplay.md). All times are UTC milliseconds since the Unix
//! epoch as `f64`; positions are 100 ns ticks (`i64`).
use serde::Deserialize;
use serde_json::Value;

pub const TICKS_PER_MS: f64 = 10_000.0;

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `2026-11-15T12:34:56.1234567Z` (fraction optional, `Z` or `+hh:mm`) to epoch milliseconds.
pub fn parse_utc_ms(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let num = |a: usize, n: usize| -> Option<i64> { s.get(a..a + n)?.parse().ok() };
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b' ') {
        return None;
    }
    let (y, mo, d, h, mi, se) = (
        num(0, 4)?,
        num(5, 2)?,
        num(8, 2)?,
        num(11, 2)?,
        num(14, 2)?,
        num(17, 2)?,
    );
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let mut i = 19;
    let mut frac = 0.0;
    if b.get(i) == Some(&b'.') {
        let digits = s[i + 1..].bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        frac = format!("0.{}", &s[i + 1..i + 1 + digits])
            .parse::<f64>()
            .ok()?;
        i += 1 + digits;
    }
    let offset_min = match &s[i..] {
        "Z" | "z" | "" => 0,
        t if t.len() == 6 && (t.starts_with('+') || t.starts_with('-')) => {
            let v = t[1..3].parse::<i64>().ok()? * 60 + t[4..6].parse::<i64>().ok()?;
            if t.starts_with('-') { -v } else { v }
        }
        _ => return None,
    };
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se - offset_min * 60;
    Some(secs as f64 * 1000.0 + frac * 1000.0)
}

/// Epoch milliseconds as `2026-11-15T12:34:56.789Z`.
pub fn format_utc(ms: f64) -> String {
    let t = ms.floor() as i64;
    let (secs, milli) = (t.div_euclid(1000), t.rem_euclid(1000));
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{milli:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Local wall clock now, epoch milliseconds.
pub fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum GroupState {
    #[default]
    Idle,
    Waiting,
    Paused,
    Playing,
}

/// One entry of `GET /SyncPlay/List` and the `GroupJoined` payload (`GroupInfoDto`).
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct GroupInfo {
    pub group_id: String,
    pub group_name: String,
    pub state: GroupState,
    pub participants: Vec<String>,
    last_updated_at: String,
}

impl GroupInfo {
    pub fn last_updated_ms(&self) -> Option<f64> {
        parse_utc_ms(&self.last_updated_at)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Unpause,
    Pause,
    Stop,
    Seek,
}

/// `SendCommand`: do `command` at server time `when_ms`; the group is at `position_ticks` then.
#[derive(Debug, Clone, PartialEq)]
pub struct SendCommand {
    pub group_id: String,
    pub playlist_item_id: String,
    pub when_ms: f64,
    pub emitted_at_ms: f64,
    pub position_ticks: i64,
    pub command: CommandKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct QueueItem {
    pub item_id: String,
    pub playlist_item_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum QueueReason {
    NewPlaylist,
    SetCurrentItem,
    RemoveItems,
    MoveItem,
    Queue,
    QueueNext,
    NextItem,
    PreviousItem,
    RepeatMode,
    ShuffleMode,
}

/// `PlayQueueUpdate`: the group's playlist. `playing_item_index` is -1 when nothing plays.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayQueue {
    pub reason: QueueReason,
    pub last_update_ms: f64,
    pub playlist: Vec<QueueItem>,
    pub playing_item_index: i32,
    pub start_position_ticks: i64,
    pub is_playing: bool,
}

impl PlayQueue {
    pub fn playing(&self) -> Option<&QueueItem> {
        usize::try_from(self.playing_item_index)
            .ok()
            .and_then(|i| self.playlist.get(i))
    }
}

/// A `SyncPlayGroupUpdate` message body.
#[derive(Debug, Clone, PartialEq)]
pub enum GroupUpdate {
    UserJoined(String),
    UserLeft(String),
    GroupJoined(GroupInfo),
    GroupLeft,
    StateUpdate { state: GroupState, reason: String },
    PlayQueue(PlayQueue),
    NotInGroup,
    GroupDoesNotExist,
    LibraryAccessDenied,
}

/// What the server pushes over the socket that SyncPlay cares about.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Command(SendCommand),
    Group(GroupUpdate),
    /// `ForceKeepAlive`: answer with `KeepAlive` within this many seconds.
    ForceKeepAlive(u64),
    /// Remote control (`Play`, `Playstate`, `GeneralCommand`): message type and its `Data`,
    /// interpreted by the app (nocturne_core::remote).
    Remote(String, Value),
    Other,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct RawCommand {
    group_id: String,
    playlist_item_id: String,
    when: String,
    emitted_at: String,
    position_ticks: Option<i64>,
    command: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct RawQueue {
    reason: Option<QueueReason>,
    last_update: String,
    playlist: Vec<QueueItem>,
    playing_item_index: i32,
    start_position_ticks: i64,
    is_playing: bool,
}

/// Decodes one text frame. Anything that is not SyncPlay or a keep-alive request is `Other`;
/// malformed SyncPlay bodies are also `Other` (a bad message must not drop the socket).
pub fn decode(text: &str) -> Incoming {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Incoming::Other;
    };
    let data = &v["Data"];
    match v["MessageType"].as_str() {
        Some("ForceKeepAlive") => Incoming::ForceKeepAlive(data.as_u64().unwrap_or(60)),
        Some("SyncPlayCommand") => decode_command(data).map_or(Incoming::Other, Incoming::Command),
        Some("SyncPlayGroupUpdate") => decode_group(data).map_or(Incoming::Other, Incoming::Group),
        Some(t @ ("Play" | "Playstate" | "GeneralCommand")) => {
            Incoming::Remote(t.to_string(), data.clone())
        }
        _ => Incoming::Other,
    }
}

fn decode_command(d: &Value) -> Option<SendCommand> {
    let r: RawCommand = serde_json::from_value(d.clone()).ok()?;
    let command = match r.command.as_str() {
        "Unpause" => CommandKind::Unpause,
        "Pause" => CommandKind::Pause,
        "Stop" => CommandKind::Stop,
        "Seek" => CommandKind::Seek,
        _ => return None,
    };
    Some(SendCommand {
        group_id: r.group_id,
        playlist_item_id: r.playlist_item_id,
        when_ms: parse_utc_ms(&r.when)?,
        emitted_at_ms: parse_utc_ms(&r.emitted_at)?,
        position_ticks: r.position_ticks.unwrap_or(0),
        command,
    })
}

fn decode_group(d: &Value) -> Option<GroupUpdate> {
    let inner = &d["Data"];
    Some(match d["Type"].as_str()? {
        "UserJoined" => GroupUpdate::UserJoined(inner.as_str()?.to_string()),
        "UserLeft" => GroupUpdate::UserLeft(inner.as_str()?.to_string()),
        "GroupJoined" => GroupUpdate::GroupJoined(serde_json::from_value(inner.clone()).ok()?),
        "GroupLeft" => GroupUpdate::GroupLeft,
        "StateUpdate" => GroupUpdate::StateUpdate {
            state: serde_json::from_value(inner["State"].clone()).ok()?,
            reason: inner["Reason"].as_str().unwrap_or_default().to_string(),
        },
        "PlayQueue" => {
            let q: RawQueue = serde_json::from_value(inner.clone()).ok()?;
            GroupUpdate::PlayQueue(PlayQueue {
                reason: q.reason?,
                last_update_ms: parse_utc_ms(&q.last_update)?,
                playlist: q.playlist,
                playing_item_index: q.playing_item_index,
                start_position_ticks: q.start_position_ticks,
                is_playing: q.is_playing,
            })
        }
        "NotInGroup" => GroupUpdate::NotInGroup,
        "GroupDoesNotExist" => GroupUpdate::GroupDoesNotExist,
        "LibraryAccessDenied" => GroupUpdate::LibraryAccessDenied,
        _ => return None,
    })
}

/// `UserPolicy.SyncPlayAccess`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum SyncAccess {
    CreateAndJoinGroups,
    JoinGroups,
    #[default]
    None,
}

impl SyncAccess {
    pub fn can_create(self) -> bool {
        self == Self::CreateAndJoinGroups
    }
    pub fn can_join(self) -> bool {
        self != Self::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip_and_match_known_values() {
        // 2026-11-15T12:34:56Z = 1_794_746_096 s (checked with `date -u -d`).
        assert_eq!(
            parse_utc_ms("2026-11-15T12:34:56Z"),
            Some(1_794_746_096_000.0)
        );
        let ms = parse_utc_ms("2026-11-15T12:34:56.1234567Z").unwrap();
        assert!((ms - 1_794_746_096_123.456_7).abs() < 0.01);
        assert_eq!(format_utc(1_794_746_096_123.0), "2026-11-15T12:34:56.123Z");
        assert_eq!(parse_utc_ms("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(
            parse_utc_ms("2000-02-29T00:00:00Z"),
            Some(951_782_400_000.0)
        );
        assert_eq!(
            parse_utc_ms("2026-11-15T14:34:56+02:00"),
            parse_utc_ms("2026-11-15T12:34:56Z")
        );
        assert_eq!(parse_utc_ms("garbage"), None);
        assert_eq!(parse_utc_ms("2026-13-15T12:34:56Z"), None);
        let t = 1_700_000_000_123.0;
        assert_eq!(parse_utc_ms(&format_utc(t)), Some(t));
    }

    #[test]
    fn decodes_a_command() {
        let j = r#"{"MessageType":"SyncPlayCommand","MessageId":"5e0e4f2c-0000-0000-0000-000000000001","Data":{"GroupId":"11111111-2222-3333-4444-555555555555","PlaylistItemId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","When":"2026-11-15T12:00:01.5000000Z","PositionTicks":300000000,"Command":"Unpause","EmittedAt":"2026-11-15T12:00:00.0000000Z"}}"#;
        let Incoming::Command(c) = decode(j) else {
            panic!("not a command")
        };
        assert_eq!(c.command, CommandKind::Unpause);
        assert_eq!(c.position_ticks, 300_000_000);
        assert_eq!(c.when_ms - c.emitted_at_ms, 1500.0);
        assert_eq!(c.playlist_item_id, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    }

    #[test]
    fn decodes_group_updates() {
        let wrap = |t: &str, d: &str| {
            format!(
                r#"{{"MessageType":"SyncPlayGroupUpdate","MessageId":"x","Data":{{"GroupId":"g","Type":"{t}","Data":{d}}}}}"#
            )
        };
        assert_eq!(
            decode(&wrap("UserJoined", "\"bob\"")),
            Incoming::Group(GroupUpdate::UserJoined("bob".into()))
        );
        assert_eq!(
            decode(&wrap("UserLeft", "\"bob\"")),
            Incoming::Group(GroupUpdate::UserLeft("bob".into()))
        );
        assert_eq!(
            decode(&wrap("GroupLeft", "\"g\"")),
            Incoming::Group(GroupUpdate::GroupLeft)
        );
        assert_eq!(
            decode(&wrap("NotInGroup", "\"\"")),
            Incoming::Group(GroupUpdate::NotInGroup)
        );
        assert_eq!(
            decode(&wrap("GroupDoesNotExist", "\"g\"")),
            Incoming::Group(GroupUpdate::GroupDoesNotExist)
        );
        assert_eq!(
            decode(&wrap("LibraryAccessDenied", "\"g\"")),
            Incoming::Group(GroupUpdate::LibraryAccessDenied)
        );
        assert_eq!(
            decode(&wrap(
                "StateUpdate",
                r#"{"State":"Waiting","Reason":"Buffer"}"#
            )),
            Incoming::Group(GroupUpdate::StateUpdate {
                state: GroupState::Waiting,
                reason: "Buffer".into()
            })
        );
        let Incoming::Group(GroupUpdate::GroupJoined(g)) = decode(&wrap(
            "GroupJoined",
            r#"{"GroupId":"g","GroupName":"Movie night","State":"Idle","Participants":["alice"],"LastUpdatedAt":"2026-11-15T12:00:00.0000000Z"}"#,
        )) else {
            panic!("not joined")
        };
        assert_eq!(g.group_name, "Movie night");
        assert_eq!(g.participants, ["alice"]);
        assert!(g.last_updated_ms().is_some());
        let q = wrap(
            "PlayQueue",
            r#"{"Reason":"NewPlaylist","LastUpdate":"2026-11-15T12:00:00.0000000Z","Playlist":[{"ItemId":"i1","PlaylistItemId":"p1"},{"ItemId":"i2","PlaylistItemId":"p2"}],"PlayingItemIndex":1,"StartPositionTicks":50,"IsPlaying":false,"ShuffleMode":"Sorted","RepeatMode":"RepeatNone"}"#,
        );
        let Incoming::Group(GroupUpdate::PlayQueue(q)) = decode(&q) else {
            panic!("not a queue")
        };
        assert_eq!(q.reason, QueueReason::NewPlaylist);
        assert_eq!(q.playing().unwrap().playlist_item_id, "p2");
        assert_eq!(q.start_position_ticks, 50);
    }

    #[test]
    fn keepalive_and_noise() {
        assert_eq!(
            decode(r#"{"MessageType":"ForceKeepAlive","Data":60}"#),
            Incoming::ForceKeepAlive(60)
        );
        assert_eq!(
            decode(r#"{"MessageType":"Sessions","Data":[]}"#),
            Incoming::Other
        );
        assert_eq!(decode("not json"), Incoming::Other);
        assert_eq!(
            decode(r#"{"MessageType":"SyncPlayCommand","Data":{"Command":"Dance"}}"#),
            Incoming::Other
        );
    }
}
