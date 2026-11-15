//! SyncPlay REST calls. Everything that changes server state takes a [`UserConfirmed`] (minted by
//! the app's single write choke point, after the master switch and the dev-mode check); the list,
//! group and clock reads do not.
use super::engine::Report;
use super::model::*;
use super::time::Sample;
use crate::{Result, UserClient, UserConfirmed};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct UtcTime {
    request_reception_time: String,
    response_transmission_time: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct Me {
    policy: Policy,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct Policy {
    sync_play_access: SyncAccess,
}

/// What `SetNewQueue` needs: item ids in play order, which one to start with, and where.
#[derive(Debug, Clone)]
pub struct NewQueue {
    pub item_ids: Vec<String>,
    pub start_index: usize,
    pub start_ticks: i64,
}

impl UserClient {
    async fn sp(&self, path: &str, body: Value, _c: UserConfirmed) -> Result<()> {
        self.run(Method::POST, &["SyncPlay", path], &[], Some(body))
            .await
    }

    /// READ `GET /Users/Me`: what this user may do with SyncPlay.
    pub async fn syncplay_access(&self) -> Result<SyncAccess> {
        let m: Me = self.json(Method::GET, &["Users", "Me"], &[], None).await?;
        Ok(m.policy.sync_play_access)
    }

    /// READ `GET /SyncPlay/List`: groups this user may join.
    pub async fn syncplay_list(&self) -> Result<Vec<GroupInfo>> {
        self.json(Method::GET, &["SyncPlay", "List"], &[], None)
            .await
    }

    /// READ `GET /SyncPlay/{id}`; a missing group is `Error::Status { status: 404 }`.
    pub async fn syncplay_group(&self, id: &str) -> Result<GroupInfo> {
        self.json(Method::GET, &["SyncPlay", id], &[], None).await
    }

    /// READ `GET /GetUtcTime`: one clock round trip, timed on the local wall clock.
    pub async fn syncplay_time_sample(&self) -> Result<Sample> {
        let sent = now_ms();
        let t: UtcTime = self.json(Method::GET, &["GetUtcTime"], &[], None).await?;
        let received = now_ms();
        let bad = || crate::Error::Config("GetUtcTime answered with unreadable times".into());
        Ok(Sample {
            sent,
            server_received: parse_utc_ms(&t.request_reception_time).ok_or_else(bad)?,
            server_sent: parse_utc_ms(&t.response_transmission_time).ok_or_else(bad)?,
            received,
        })
    }

    /// WRITE `POST /SyncPlay/New`. Newer servers answer with the group, older ones with 204 and
    /// a `GroupJoined` socket message; either way the caller learns the group from the socket.
    pub async fn syncplay_new(&self, name: &str, _c: UserConfirmed) -> Result<Option<GroupInfo>> {
        let name = name.trim();
        if name.is_empty() || name.len() > 100 {
            return Err(crate::Error::Config(
                "a group needs a name (up to 100 characters)".into(),
            ));
        }
        let r = self
            .send(
                Method::POST,
                &["SyncPlay", "New"],
                &[],
                Some(json!({"GroupName": name})),
            )
            .await?;
        let text = r.text().await.unwrap_or_default();
        Ok(serde_json::from_str(&text).ok())
    }

    /// WRITE `POST /SyncPlay/Join`.
    pub async fn syncplay_join(&self, group_id: &str, c: UserConfirmed) -> Result<()> {
        self.sp("Join", json!({"GroupId": group_id}), c).await
    }

    /// WRITE `POST /SyncPlay/Leave`.
    pub async fn syncplay_leave(&self, c: UserConfirmed) -> Result<()> {
        self.sp("Leave", json!({}), c).await
    }

    /// WRITE `POST /SyncPlay/SetNewQueue`: replaces the group's queue and starts it for everyone.
    pub async fn syncplay_set_new_queue(&self, q: &NewQueue, c: UserConfirmed) -> Result<()> {
        let body = json!({
            "PlayingQueue": q.item_ids,
            "PlayingItemPosition": q.start_index,
            "StartPositionTicks": q.start_ticks.max(0),
        });
        self.sp("SetNewQueue", body, c).await
    }

    /// WRITE `POST /SyncPlay/SetPlaylistItem`: jump to a queue entry.
    pub async fn syncplay_set_item(&self, playlist_item_id: &str, c: UserConfirmed) -> Result<()> {
        self.sp(
            "SetPlaylistItem",
            json!({"PlaylistItemId": playlist_item_id}),
            c,
        )
        .await
    }

    pub async fn syncplay_pause(&self, c: UserConfirmed) -> Result<()> {
        self.sp("Pause", json!({}), c).await
    }

    pub async fn syncplay_unpause(&self, c: UserConfirmed) -> Result<()> {
        self.sp("Unpause", json!({}), c).await
    }

    pub async fn syncplay_stop(&self, c: UserConfirmed) -> Result<()> {
        self.sp("Stop", json!({}), c).await
    }

    pub async fn syncplay_seek(&self, ticks: i64, c: UserConfirmed) -> Result<()> {
        self.sp("Seek", json!({"PositionTicks": ticks.max(0)}), c)
            .await
    }

    /// `NextItem` / `PreviousItem` carry the playlist item the caller is on, so several clients
    /// asking at once (end of file) advance the group only once.
    pub async fn syncplay_next(&self, playlist_item_id: &str, c: UserConfirmed) -> Result<()> {
        self.sp("NextItem", json!({"PlaylistItemId": playlist_item_id}), c)
            .await
    }

    pub async fn syncplay_previous(&self, playlist_item_id: &str, c: UserConfirmed) -> Result<()> {
        self.sp(
            "PreviousItem",
            json!({"PlaylistItemId": playlist_item_id}),
            c,
        )
        .await
    }

    /// WRITE `POST /SyncPlay/Buffering` (`buffering`) or `/Ready`: tells the group this client is
    /// stalled / can play again at `r.pos_ticks`.
    pub async fn syncplay_report(
        &self,
        buffering: bool,
        r: &Report,
        c: UserConfirmed,
    ) -> Result<()> {
        let body = json!({
            "When": format_utc(r.when_ms),
            "PositionTicks": r.pos_ticks,
            "IsPlaying": r.playing,
            "PlaylistItemId": r.playlist_item_id,
        });
        self.sp(if buffering { "Buffering" } else { "Ready" }, body, c)
            .await
    }

    /// WRITE `POST /SyncPlay/Ping`: our latency, which the server uses to lead commands.
    pub async fn syncplay_ping(&self, ms: i64, c: UserConfirmed) -> Result<()> {
        self.sp("Ping", json!({"Ping": ms.max(0)}), c).await
    }

    /// WRITE `POST /SyncPlay/SetIgnoreWait`: stop (or resume) holding the group back.
    pub async fn syncplay_ignore_wait(&self, ignore: bool, c: UserConfirmed) -> Result<()> {
        self.sp("SetIgnoreWait", json!({"IgnoreWait": ignore}), c)
            .await
    }

    /// WRITE `POST /SyncPlay/Queue`: add items at the end (`next` = right after the current one).
    pub async fn syncplay_queue(&self, ids: &[String], next: bool, c: UserConfirmed) -> Result<()> {
        let mode = if next { "QueueNext" } else { "Queue" };
        self.sp("Queue", json!({"ItemIds": ids, "Mode": mode}), c)
            .await
    }

    /// WRITE `POST /SyncPlay/RemoveFromPlaylist`.
    pub async fn syncplay_remove(
        &self,
        playlist_item_ids: &[String],
        c: UserConfirmed,
    ) -> Result<()> {
        self.sp(
            "RemoveFromPlaylist",
            json!({"PlaylistItemIds": playlist_item_ids}),
            c,
        )
        .await
    }

    /// WRITE `POST /SyncPlay/SetRepeatMode` (`RepeatOne`, `RepeatAll`, `RepeatNone`).
    pub async fn syncplay_repeat(&self, mode: &str, c: UserConfirmed) -> Result<()> {
        self.sp("SetRepeatMode", json!({"Mode": mode}), c).await
    }

    /// WRITE `POST /SyncPlay/SetShuffleMode` (`Sorted`, `Shuffle`).
    pub async fn syncplay_shuffle(&self, mode: &str, c: UserConfirmed) -> Result<()> {
        self.sp("SetShuffleMode", json!({"Mode": mode}), c).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::serve;

    fn c() -> UserConfirmed {
        UserConfirmed::i_have_asked_the_user()
    }

    fn body(log: &crate::mock::Log, i: usize) -> Value {
        serde_json::from_str(&log.lock().unwrap()[i].2).unwrap()
    }

    #[tokio::test]
    async fn writes_send_the_documented_bodies() {
        let ok = |p| ("POST", p, 204, String::new());
        let (u, log) = serve(vec![
            ("POST", "/SyncPlay/New", 200, r#"{"GroupId":"g","GroupName":"Night","State":"Idle","Participants":["me"],"LastUpdatedAt":"2026-11-15T12:00:00Z"}"#.into()),
            ok("/SyncPlay/Join"),
            ok("/SyncPlay/Leave"),
            ok("/SyncPlay/SetNewQueue"),
            ok("/SyncPlay/Seek"),
            ok("/SyncPlay/Ready"),
            ok("/SyncPlay/Buffering"),
            ok("/SyncPlay/NextItem"),
            ok("/SyncPlay/Ping"),
            ok("/SyncPlay/Queue"),
        ])
        .await;
        let g = u.syncplay_new("  Night ", c()).await.unwrap().unwrap();
        assert_eq!((g.group_id.as_str(), g.group_name.as_str()), ("g", "Night"));
        u.syncplay_join("g", c()).await.unwrap();
        u.syncplay_leave(c()).await.unwrap();
        u.syncplay_set_new_queue(
            &NewQueue {
                item_ids: vec!["a".into(), "b".into()],
                start_index: 1,
                start_ticks: -5,
            },
            c(),
        )
        .await
        .unwrap();
        u.syncplay_seek(42, c()).await.unwrap();
        let r = Report {
            when_ms: 1_794_746_096_123.0,
            pos_ticks: 9,
            playing: false,
            playlist_item_id: "p".into(),
        };
        u.syncplay_report(false, &r, c()).await.unwrap();
        u.syncplay_report(true, &r, c()).await.unwrap();
        u.syncplay_next("p", c()).await.unwrap();
        u.syncplay_ping(37, c()).await.unwrap();
        u.syncplay_queue(&["x".into()], true, c()).await.unwrap();
        assert_eq!(body(&log, 0), json!({"GroupName": "Night"}));
        assert_eq!(body(&log, 1), json!({"GroupId": "g"}));
        assert_eq!(
            body(&log, 3),
            json!({"PlayingQueue": ["a","b"], "PlayingItemPosition": 1, "StartPositionTicks": 0})
        );
        assert_eq!(body(&log, 4), json!({"PositionTicks": 42}));
        assert_eq!(
            body(&log, 5),
            json!({"When":"2026-11-15T12:34:56.123Z","PositionTicks":9,"IsPlaying":false,"PlaylistItemId":"p"})
        );
        assert_eq!(log.lock().unwrap()[6].1, "/SyncPlay/Buffering");
        assert_eq!(body(&log, 7), json!({"PlaylistItemId": "p"}));
        assert_eq!(body(&log, 8), json!({"Ping": 37}));
        assert_eq!(
            body(&log, 9),
            json!({"ItemIds": ["x"], "Mode": "QueueNext"})
        );
    }

    #[tokio::test]
    async fn new_accepts_a_204_and_rejects_blank_names() {
        let (u, log) = serve(vec![("POST", "/SyncPlay/New", 204, String::new())]).await;
        assert!(u.syncplay_new("x", c()).await.unwrap().is_none());
        assert!(u.syncplay_new("   ", c()).await.is_err());
        assert_eq!(
            log.lock().unwrap().len(),
            1,
            "a blank name never reaches the server"
        );
    }

    #[tokio::test]
    async fn reads_list_access_group_and_clock() {
        let (u, _) = serve(vec![
            ("GET", "/SyncPlay/List", 200, r#"[{"GroupId":"g","GroupName":"A","State":"Playing","Participants":["x","y"],"LastUpdatedAt":"2026-11-15T12:00:00.0000000Z"}]"#.into()),
            ("GET", "/Users/Me", 200, r#"{"Name":"n","Policy":{"IsAdministrator":false,"SyncPlayAccess":"JoinGroups"}}"#.into()),
            ("GET", "/GetUtcTime", 200, r#"{"RequestReceptionTime":"2026-11-15T12:00:00.5000000Z","ResponseTransmissionTime":"2026-11-15T12:00:00.5010000Z"}"#.into()),
        ])
        .await;
        let l = u.syncplay_list().await.unwrap();
        assert_eq!(
            (l[0].state, l[0].participants.len()),
            (GroupState::Playing, 2)
        );
        let a = u.syncplay_access().await.unwrap();
        assert_eq!(a, SyncAccess::JoinGroups);
        assert!(a.can_join() && !a.can_create());
        let s = u.syncplay_time_sample().await.unwrap();
        assert!((s.server_sent - s.server_received - 1.0).abs() < 1e-6);
        assert!(s.received >= s.sent);
        assert!(matches!(
            u.syncplay_group("nope").await,
            Err(crate::Error::Status { status: 404, .. })
        ));
    }
}
