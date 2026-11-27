//! Rewriting what a server sends so that no server token, server address or file path reaches a client, and every id is a gateway id (plan sections 2 and 3, finding V5).
//! Pure functions over `serde_json::Value` and strings; the callers own the I/O. JSON is rewritten by field name (an explicit list), never by regex over every 32-hex string, because image tags and ETags look the same.
use crate::idmap::{Gid, IdMap, Member, key_media_source};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

/// Fields that hold an item id. `Id` is included: nested objects (people, genres, artists) carry item ids; the top-level `Id` and `MediaSources` are set by the merge step.
const REF_FIELDS: [&str; 14] = [
    "Id",
    "ParentId",
    "SeriesId",
    "SeasonId",
    "AlbumId",
    "ChannelId",
    "ItemId",
    "DisplayPreferencesId",
    "PrimaryImageItemId",
    "ParentBackdropItemId",
    "ParentLogoItemId",
    "ParentThumbItemId",
    "ParentPrimaryImageItemId",
    "ParentArtItemId",
];

/// Never forwarded to a client: file paths, headers that carry server tokens, the server's view of the client address, tokens.
const STRIP: [&str; 5] = [
    "Path",
    "RequiredHttpHeaders",
    "RemoteEndPoint",
    "AccessToken",
    "EncoderPath",
];

/// Fields holding a root-relative URL of the server (`/videos/{id}/master.m3u8?...`).
const URL_FIELDS: [&str; 3] = ["TranscodingUrl", "DirectStreamUrl", "DeliveryUrl"];

/// From a single-origin id to the merged id that now stands for it (set when a group of copies is merged), so references such as `SeriesId` point at the merged series.
/// ponytail: in memory only; after a restart an old single id still resolves to its one server until a list merges the group again.
#[derive(Default)]
pub struct Aliases(Mutex<HashMap<Gid, Gid>>);

impl Aliases {
    pub fn set(&self, single: Gid, merged: Gid) {
        if single != merged {
            self.0.lock().unwrap().insert(single, merged);
        }
    }
    pub fn get(&self, g: Gid) -> Gid {
        self.0.lock().unwrap().get(&g).copied().unwrap_or(g)
    }
}

/// What the JSON rewriter needs about the response's origin.
pub struct JsonCtx<'a> {
    pub ids: &'a IdMap,
    pub alias: &'a Aliases,
    /// The server's stable key (see `idmap`).
    pub src_key: &'a str,
    pub server_id: &'a str,
    pub user_id: &'a str,
}

fn id_like(s: &str) -> bool {
    Gid::parse(s).is_some()
}

/// Replace every item reference of `v` by a gateway id and drop what must not leave. The top-level `Id` and `MediaSources` are left alone (see [`REF_FIELDS`]).
pub fn rewrite_refs(v: &mut Value, c: &JsonCtx) {
    walk(v, c, true);
}

fn walk(v: &mut Value, c: &JsonCtx, top: bool) {
    match v {
        Value::Array(a) => a.iter_mut().for_each(|x| walk(x, c, false)),
        Value::Object(m) => {
            for k in STRIP {
                m.remove(k);
            }
            for (k, x) in m.iter_mut() {
                match (k.as_str(), &*x) {
                    ("MediaSources", _) if top => {}
                    ("Id", _) if top => {}
                    ("ServerId", Value::String(_)) => *x = c.server_id.into(),
                    ("UserId", Value::String(_)) => *x = c.user_id.into(),
                    (k, Value::String(s)) if REF_FIELDS.contains(&k) && id_like(s) => {
                        let g = c.ids.register_single(c.src_key, s);
                        *x = c.alias.get(g).to_string().into();
                    }
                    _ => walk(x, c, false),
                }
            }
        }
        _ => {}
    }
}

/// How to turn one server URL into a gateway URL.
pub struct UrlCtx<'a> {
    pub ids: &'a IdMap,
    pub src_key: &'a str,
    /// The gateway id the URL's item now has.
    pub item: Gid,
    /// The server's own id of that item (any form).
    pub up_item: &'a str,
    /// `host[:port]` of the server, lowercase: an absolute URL to any other host is dropped.
    pub authority: &'a str,
    /// The client's own gateway token, to be put where the server's was.
    pub token: Option<&'a str>,
}

fn norm(s: &str) -> String {
    s.to_ascii_lowercase().replace('-', "")
}

/// The gateway id of a media source of `item`.
pub fn media_source_gid(ids: &IdMap, item: Gid, src_key: &str, up_msid: &str) -> Gid {
    ids.register(
        &key_media_source(item, &norm(up_msid)),
        &[Member {
            source: src_key.into(),
            id: up_msid.into(),
        }],
    )
}

/// A path or an absolute URL of the server, rewritten: server address removed, ids replaced (the first id in the path is the item, later ones are media sources), `ApiKey`/`api_key` dropped, `MediaSourceId` mapped, the client's token added.
/// An absolute URL to another host gives "" (a server never gets to point a client elsewhere).
pub fn rewrite_url(u: &str, c: &UrlCtx) -> String {
    let rest = if let Some(i) = u.find("://") {
        let after = &u[i + 3..];
        let (auth, tail) = after.split_at(after.find(['/', '?']).unwrap_or(after.len()));
        if !auth.eq_ignore_ascii_case(c.authority) {
            return String::new();
        }
        tail
    } else {
        u
    };
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut seen_item = false;
    let path: Vec<String> = path
        .split('/')
        .map(|seg| {
            if !id_like(seg) {
                return seg.to_string();
            }
            let dashed = seg.len() == 36;
            let (g, first) = if !seen_item {
                seen_item = true;
                (c.item, true)
            } else {
                (media_source_gid(c.ids, c.item, c.src_key, seg), false)
            };
            let _ = first;
            if dashed { g.dashed() } else { g.to_string() }
        })
        .collect();
    let mut out = path.join("/");
    let mut pairs: Vec<String> = vec![];
    for p in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = p.split_once('=').unwrap_or((p, ""));
        match k.to_ascii_lowercase().as_str() {
            "apikey" | "api_key" => {}
            "mediasourceid" if id_like(v) => pairs.push(format!(
                "{k}={}",
                media_source_gid(c.ids, c.item, c.src_key, v)
            )),
            _ => pairs.push(p.to_string()),
        }
    }
    if let Some(t) = c.token {
        pairs.push(format!("ApiKey={t}"));
    }
    if !pairs.is_empty() {
        out.push('?');
        out.push_str(&pairs.join("&"));
    }
    out
}

/// An HLS playlist (master or variant) with every URL, in lines and in `URI="..."` attributes, passed through [`rewrite_url`].
pub fn rewrite_playlist(text: &str, c: &UrlCtx) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    for line in text.lines() {
        if line.is_empty() {
            out.push('\n');
        } else if let Some(tag) = line.strip_prefix('#') {
            out.push('#');
            out.push_str(&rewrite_uri_attrs(tag, c));
            out.push('\n');
        } else {
            out.push_str(&rewrite_url(line.trim(), c));
            out.push('\n');
        }
    }
    out
}

fn rewrite_uri_attrs(tag: &str, c: &UrlCtx) -> String {
    let mut out = String::new();
    let mut rest = tag;
    while let Some(i) = rest.find("URI=\"") {
        let (head, tail) = rest.split_at(i + 5);
        out.push_str(head);
        let end = tail.find('"').unwrap_or(tail.len());
        out.push_str(&rewrite_url(&tail[..end], c));
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// Remove [`STRIP`] fields at any depth (a media source's streams carry the paths of external subtitle files).
fn strip_deep(v: &mut Value) {
    match v {
        Value::Array(a) => a.iter_mut().for_each(strip_deep),
        Value::Object(m) => {
            for k in STRIP {
                m.remove(k);
            }
            m.values_mut().for_each(strip_deep);
        }
        _ => {}
    }
}

/// One media source of an item: id mapped, name labelled, path and tokens stripped, URLs rewritten. `label` is the origin to put in front of the name (merged items).
pub fn rewrite_source(s: &mut Value, c: &UrlCtx, label: Option<&str>) {
    strip_deep(s);
    let Some(m) = s.as_object_mut() else { return };
    let up = m
        .get("Id")
        .and_then(Value::as_str)
        .unwrap_or(c.up_item)
        .to_string();
    m.insert(
        "Id".into(),
        media_source_gid(c.ids, c.item, c.src_key, &up)
            .to_string()
            .into(),
    );
    m.insert("ItemId".into(), c.item.to_string().into());
    if let Some(l) = label {
        let name = m.get("Name").and_then(Value::as_str).unwrap_or("").trim();
        let name = if name.is_empty() {
            l.to_string()
        } else {
            format!("{l} · {name}")
        };
        m.insert("Name".into(), name.into());
    }
    rewrite_urls_in(s, c);
}

/// Rewrite the URL fields ([`URL_FIELDS`]) anywhere below `v` (a media source holds them in itself and in its streams).
pub fn rewrite_urls_in(v: &mut Value, c: &UrlCtx) {
    match v {
        Value::Array(a) => a.iter_mut().for_each(|x| rewrite_urls_in(x, c)),
        Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                match x {
                    Value::String(s) if URL_FIELDS.contains(&k.as_str()) => {
                        *x = rewrite_url(s, c).into()
                    }
                    _ => rewrite_urls_in(x, c),
                }
            }
        }
        _ => {}
    }
}

/// Last line of defence for a serialized response: removes any secret (a server token, a server address) that is still in the text, whatever field it was in.
pub fn scrub(mut s: String, secrets: &[String]) -> String {
    for x in secrets.iter().filter(|x| x.len() >= 8) {
        if s.contains(x.as_str()) {
            s = s.replace(x.as_str(), "");
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TOKEN: &str = "SRVTOKEN0123456789abcdef";
    const BASE: &str = "http://10.9.8.7:8096";
    const FILE: &str = "/srv/media/Private Movies/Secret Film (2020).mkv";
    const ITEM: &str = concat!("9e3dc8bc2a5f4b5b", "8b0a3f2d9d5e7c11");
    const ITEM_D: &str = "9e3dc8bc-2a5f-4b5b-8b0a-3f2d9d5e7c11";

    fn ctx<'a>(ids: &'a IdMap, item: Gid) -> UrlCtx<'a> {
        UrlCtx {
            ids,
            src_key: "srvA",
            item,
            up_item: ITEM,
            authority: "10.9.8.7:8096",
            token: Some("CLIENTTOK"),
        }
    }

    fn assert_clean(s: &str) {
        for bad in [TOKEN, "10.9.8.7", FILE, "Private Movies", ITEM, ITEM_D] {
            assert!(!s.contains(bad), "{bad} leaked in {s}");
        }
    }

    #[test]
    fn transcoding_url_and_master_playlist() {
        let ids = IdMap::in_memory();
        let g = ids.register_single("srvA", ITEM);
        let c = ctx(&ids, g);
        let t = format!(
            "/videos/{ITEM_D}/master.m3u8?DeviceId=d1&MediaSourceId={ITEM}&VideoCodec=h264&api_key={TOKEN}&PlaySessionId=ps1&ApiKey={TOKEN}"
        );
        let r = rewrite_url(&t, &c);
        assert_clean(&r);
        assert!(r.starts_with(&format!("/videos/{}/master.m3u8?", g.dashed())));
        assert!(r.contains("DeviceId=d1") && r.contains("PlaySessionId=ps1"));
        assert!(r.ends_with("&ApiKey=CLIENTTOK"));
        // The media source id is a gateway id the id map can take back.
        let ms = r
            .split("MediaSourceId=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap();
        let (_, members) = ids.resolve_str(ms).unwrap();
        assert_eq!(members[0].id, ITEM);

        let pl = format!(
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000,RESOLUTION=160x90\nmain.m3u8?DeviceId=d1&MediaSourceId={ITEM}&ApiKey={TOKEN}\n#EXT-X-MAP:URI=\"{BASE}/videos/{ITEM_D}/hls1/main/-1.mp4?ApiKey={TOKEN}\"\n/videos/{ITEM_D}/hls1/main/0.ts?runtimeTicks=0&ApiKey={TOKEN}\n#EXT-X-ENDLIST\n"
        );
        let out = rewrite_playlist(&pl, &c);
        assert_clean(&out);
        assert!(out.contains("main.m3u8?DeviceId=d1"));
        assert!(out.contains(&format!(
            "URI=\"/videos/{}/hls1/main/-1.mp4?ApiKey=CLIENTTOK\"",
            g.dashed()
        )));
        assert!(out.contains("#EXT-X-ENDLIST"));
    }

    #[test]
    fn foreign_host_is_dropped_and_without_client_token_none_is_added() {
        let ids = IdMap::in_memory();
        let g = ids.register_single("srvA", ITEM);
        let mut c = ctx(&ids, g);
        assert_eq!(rewrite_url("http://evil.test/x?ApiKey=1", &c), "");
        c.token = None;
        assert_eq!(
            rewrite_url(&format!("{BASE}/a/b?x=1&api_key={TOKEN}"), &c),
            "/a/b?x=1"
        );
    }

    #[test]
    fn item_json_has_no_token_url_or_path() {
        let ids = IdMap::in_memory();
        let alias = Aliases::default();
        let jc = JsonCtx {
            ids: &ids,
            alias: &alias,
            src_key: "srvA",
            server_id: "GWSERVER",
            user_id: "GWUSER",
        };
        let series = concat!("1111111122223333", "4444555555555555");
        let mut v = json!({
            "Id": ITEM, "Name": "Secret Film", "ServerId": "realserverid", "ParentId": series, "SeriesId": series,
            "Path": FILE, "ImageTags": {"Primary": concat!("0123456789abcdef", "0123456789abcdef")},
            "People": [{"Id": concat!("aaaaaaaabbbbcccc", "ddddeeeeeeeeeeee"), "Name": "X"}],
            "UserData": {"Key": "k", "ItemId": ITEM},
            "MediaSources": [{"Id": ITEM, "Path": FILE, "RequiredHttpHeaders": {"X-Emby-Token": TOKEN}, "Name": "v",
                "TranscodingUrl": format!("/videos/{ITEM_D}/master.m3u8?MediaSourceId={ITEM}&ApiKey={TOKEN}"),
                "MediaStreams": [{"Index": 2, "Path": "/srv/sub.srt", "DeliveryUrl": format!("/Videos/{ITEM}/{ITEM}/Subtitles/2/0/Stream.vtt?ApiKey={TOKEN}")}]}],
        });
        rewrite_refs(&mut v, &jc);
        let g = ids.register_single("srvA", ITEM);
        let c = ctx(&ids, g);
        let sources = v["MediaSources"].as_array_mut().unwrap();
        for s in sources.iter_mut() {
            rewrite_source(s, &c, Some("Server A"));
        }
        v["Id"] = g.to_string().into();
        let text = v.to_string();
        assert_clean(&text);
        assert!(!text.contains("/srv/") && !text.contains("realserverid"));
        // Tags stay as they are; references became gateway ids that map back.
        assert_eq!(
            v["ImageTags"]["Primary"],
            concat!("0123456789abcdef", "0123456789abcdef")
        );
        let sid = v["SeriesId"].as_str().unwrap();
        assert_ne!(sid, series);
        assert_eq!(ids.resolve_str(sid).unwrap().1[0].id, series);
        assert_eq!(v["ServerId"], "GWSERVER");
        assert_eq!(v["MediaSources"][0]["Name"], "Server A · v");
        assert_eq!(v["MediaSources"][0]["ItemId"], g.to_string());
        // The subtitle URL: item first, then the (distinct) media source.
        let d = v["MediaSources"][0]["MediaStreams"][0]["DeliveryUrl"]
            .as_str()
            .unwrap();
        assert!(d.starts_with(&format!("/Videos/{g}/")) && d.contains("/Subtitles/2/0/Stream.vtt"));
    }

    #[test]
    fn alias_points_references_at_the_merged_id() {
        let ids = IdMap::in_memory();
        let alias = Aliases::default();
        let single = ids.register_single("srvA", ITEM);
        let merged = Gid::of("merge:tmdb:1");
        alias.set(single, merged);
        let jc = JsonCtx {
            ids: &ids,
            alias: &alias,
            src_key: "srvA",
            server_id: "S",
            user_id: "U",
        };
        let mut v = json!({"Id": "x", "SeriesId": ITEM});
        rewrite_refs(&mut v, &jc);
        assert_eq!(v["SeriesId"], merged.to_string());
    }

    #[test]
    fn scrub_removes_leftovers() {
        let s = scrub(
            format!("a {TOKEN} b {BASE} c"),
            &[TOKEN.into(), BASE.into(), "x".into()],
        );
        assert_eq!(s, "a  b  c");
    }
}
