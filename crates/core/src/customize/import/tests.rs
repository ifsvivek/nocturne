//! Tests against a loopback HTTP server: nothing here touches the network.
use super::*;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct Route {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    delay: Duration,
}

fn css(body: &str) -> Route {
    route("text/css", body.as_bytes().to_vec())
}

fn route(ct: &str, body: Vec<u8>) -> Route {
    Route {
        status: 200,
        headers: vec![("Content-Type".into(), ct.into())],
        body,
        delay: Duration::ZERO,
    }
}

fn redirect(to: &str) -> Route {
    Route {
        status: 302,
        headers: vec![("Location".into(), to.into())],
        body: vec![],
        delay: Duration::ZERO,
    }
}

struct Srv {
    base: String,
    hits: Arc<Mutex<Vec<String>>>,
    routes: Arc<Mutex<HashMap<String, Route>>>,
}

impl Srv {
    fn hits(&self, path: &str) -> usize {
        self.hits
            .lock()
            .unwrap()
            .iter()
            .filter(|p| *p == path)
            .count()
    }
    fn total(&self) -> usize {
        self.hits.lock().unwrap().len()
    }
    fn remove(&self, path: &str) {
        self.routes.lock().unwrap().remove(path);
    }
    fn u(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

async fn serve(build: impl FnOnce(&str) -> Vec<(&'static str, Route)>) -> Srv {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let routes: HashMap<String, Route> = build(&base)
        .into_iter()
        .map(|(p, r)| (p.to_string(), r))
        .collect();
    let routes = Arc::new(Mutex::new(routes));
    let hits = Arc::new(Mutex::new(vec![]));
    let (r2, h2) = (routes.clone(), hits.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let (routes, hits) = (r2.clone(), h2.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut n = 0;
                while n < buf.len() {
                    let Ok(k) = s.read(&mut buf[n..]).await else {
                        return;
                    };
                    if k == 0 {
                        return;
                    }
                    n += k;
                    if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                hits.lock().unwrap().push(path.clone());
                // No credentials or referrer ever go out.
                let lower = head.to_ascii_lowercase();
                assert!(
                    !lower.contains("cookie:")
                        && !lower.contains("authorization:")
                        && !lower.contains("referer:"),
                    "{head}"
                );
                assert!(lower.contains("user-agent: nocturne\r\n"), "{head}");
                let (status, headers, body, delay) = match routes.lock().unwrap().get(&path) {
                    Some(r) => (r.status, r.headers.clone(), r.body.clone(), r.delay),
                    None => (404, vec![], b"no".to_vec(), Duration::ZERO),
                };
                tokio::time::sleep(delay).await;
                let mut out = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (k, v) in headers {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                out.push_str("\r\n");
                let _ = s.write_all(out.as_bytes()).await;
                let _ = s.write_all(&body).await;
                let _ = s.shutdown().await;
            });
        }
    });
    Srv { base, hits, routes }
}

fn opts() -> Options {
    Options {
        remote: true,
        refresh: false,
        allow_loopback: true,
        cache_dir: None,
        timeout: Duration::from_secs(5),
        budget: Duration::from_secs(20),
    }
}

fn reasons(r: &Resolved) -> String {
    r.notes
        .iter()
        .map(|n| n.reason.clone())
        .collect::<Vec<_>>()
        .join(" | ")
}

fn lines(s: &str) -> usize {
    s.matches('\n').count()
}

// ---- stylesheets -----------------------------------------------------------------------------

#[tokio::test]
async fn nested_relative_media_and_order_with_lines_kept() {
    let s = serve(|_| {
        vec![
            (
                "/a/one.css",
                css("/* c */ .one { color: red }\n@import url(sub/two.css);\n.one2 { x: y }"),
            ),
            ("/a/sub/two.css", css(".two { x: y }")),
            ("/print.css", css(".print { x: y }")),
        ]
    })
    .await;
    let src = format!(
        "a {{ }}\n@import url(\"{}\");\n@import '{}' print and (min-width: 10px);\nz {{ }}\n",
        s.u("/a/one.css"),
        s.u("/print.css")
    );
    let r = resolve(&src, None, &opts()).await;
    assert!(r.notes.is_empty(), "{}", reasons(&r));
    let o = &r.css;
    let (a, one, two, one2, pr, z) = (
        o.find("a {").unwrap(),
        o.find(".one {").unwrap(),
        o.find(".two").unwrap(),
        o.find(".one2").unwrap(),
        o.find(".print").unwrap(),
        o.find("z {").unwrap(),
    );
    assert!(
        a < one && one < two && two < one2 && one2 < pr && pr < z,
        "{o}"
    );
    assert!(o.contains("@media print and (min-width: 10px) {"));
    assert!(!o.contains("@import") && !o.contains("/* c */"));
    assert_eq!(lines(o), lines(&src), "line numbers must stay");
    assert_eq!(r.downloaded, 3);
    // the result still parses, and the media wrapper is reported by the parser, not lost silently
    let p = parse_resolved(&r).unwrap();
    assert!(p.warnings.iter().any(|w| w.message.contains("@media")));
}

#[tokio::test]
async fn relative_import_needs_a_base_and_uses_the_given_one() {
    let s = serve(|_| vec![("/t/x.css", css(".x { a: b }"))]).await;
    let r = resolve("@import 'x.css';", None, &opts()).await;
    assert!(
        reasons(&r).contains("no stylesheet address"),
        "{}",
        reasons(&r)
    );
    assert!(!r.css.contains(".x"));
    let r = resolve("@import 'x.css';", Some(&s.u("/t/theme.css")), &opts()).await;
    assert!(r.css.contains(".x") && r.notes.is_empty());
}

#[tokio::test]
async fn cycles_depth_count_and_unbalanced() {
    let s = serve(|b| {
        let mut v = vec![
            (
                "/a.css",
                css(&format!("@import \"{b}/b.css\"; .a {{ x: y }}")),
            ),
            (
                "/b.css",
                css(&format!("@import \"{b}/a.css\"; .b {{ x: y }}")),
            ),
            (
                "/d1.css",
                css(&format!("@import \"{b}/d2.css\"; .d1 {{ }}")),
            ),
            (
                "/d2.css",
                css(&format!("@import \"{b}/d3.css\"; .d2 {{ }}")),
            ),
            (
                "/d3.css",
                css(&format!("@import \"{b}/d4.css\"; .d3 {{ }}")),
            ),
            ("/d4.css", css(".d4 { }")),
            ("/bad.css", css(".open { x: y ")),
            ("/bad2.css", css("} .escape { x: y }")),
            ("/ok.css", css(".ok { x: y }")),
        ];
        v.push(("/c.css", css(".c { }")));
        v
    })
    .await;
    // cycle
    let r = resolve(&format!("@import '{}';", s.u("/a.css")), None, &opts()).await;
    assert!(reasons(&r).contains("cycle"), "{}", reasons(&r));
    assert!(r.css.contains(".a") && r.css.contains(".b"));
    // depth: main -> d1 (1) -> d2 (2) -> d3 (3) -> d4 would be 4
    let r = resolve(&format!("@import '{}';", s.u("/d1.css")), None, &opts()).await;
    assert!(r.css.contains(".d3") && !r.css.contains(".d4"), "{}", r.css);
    assert!(reasons(&r).contains("deeper than 3"));
    // unbalanced braces never leak into the parent
    let r = resolve(
        &format!(
            "@import '{}'; .after {{ x: y }} @import '{}'; @import '{}';",
            s.u("/bad.css"),
            s.u("/bad2.css"),
            s.u("/ok.css")
        ),
        None,
        &opts(),
    )
    .await;
    assert!(
        !r.css.contains(".open")
            && !r.css.contains(".escape")
            && r.css.contains(".ok")
            && r.css.contains(".after")
    );
    assert_eq!(
        r.notes
            .iter()
            .filter(|n| n.reason.contains("unbalanced"))
            .count(),
        2
    );
    // 18 imports: 16 are fetched, 2 refused
    let many: String = (0..18)
        .map(|i| format!("@import '{}?{i}';", s.u("/c.css")))
        .collect();
    let r = resolve(&many, None, &opts()).await;
    assert_eq!(
        r.notes
            .iter()
            .filter(|n| n.reason.contains("more than 16"))
            .count(),
        2,
        "{}",
        reasons(&r)
    );
    assert_eq!(s.hits("/c.css?0"), 1);
    assert_eq!(s.hits("/c.css?17"), 0);
}

#[tokio::test]
async fn size_limits_content_type_and_odd_statements() {
    let big = format!(".x {{ a: b }} /*{}*/", "x".repeat(256 * 1024));
    let part = format!(".y {{ a: b }} /*{}*/", "y".repeat(250_000));
    let s = serve(|_| {
        vec![
            ("/big.css", css(&big)),
            ("/html.css", route("text/html", b".h { }".to_vec())),
            (
                "/plain.css",
                route("text/plain; charset=utf-8", b".plain { a: b }".to_vec()),
            ),
            (
                "/none.css",
                Route {
                    status: 200,
                    headers: vec![],
                    body: b".n { }".to_vec(),
                    delay: Duration::ZERO,
                },
            ),
            ("/p1.css", css(&part)),
            ("/p2.css", css(&part)),
            ("/p3.css", css(&part)),
            ("/p4.css", css(&part)),
            ("/p5.css", css(&part)),
            (
                "/e.css",
                Route {
                    status: 500,
                    headers: vec![],
                    body: vec![],
                    delay: Duration::ZERO,
                },
            ),
        ]
    })
    .await;
    let r = resolve(&format!("@import '{}';", s.u("/big.css")), None, &opts()).await;
    assert!(reasons(&r).contains("larger than 256 KB") && !r.css.contains(".x"));
    let r = resolve(
        &format!(
            "@import '{}'; @import '{}'; @import '{}'; @import '{}';",
            s.u("/html.css"),
            s.u("/plain.css"),
            s.u("/none.css"),
            s.u("/e.css")
        ),
        None,
        &opts(),
    )
    .await;
    assert!(r.css.contains(".plain") && !r.css.contains(".h ") && !r.css.contains(".n "));
    assert_eq!(r.notes.len(), 3, "{}", reasons(&r));
    // 1 MB together: the fifth 250 KB file does not fit
    let five: String = (1..=5)
        .map(|i| format!("@import '{}';", s.u(&format!("/p{i}.css"))))
        .collect();
    let r = resolve(&five, None, &opts()).await;
    assert_eq!(r.css.matches(".y {").count(), 4);
    assert!(reasons(&r).contains("1 MB limit"));
    // statements that are not followed
    let r = resolve("@import url(x.css) supports(display: grid);\n@import url(\"https://a.example/x.css\" layer;\n.k { }", None, &opts()).await;
    assert!(r.css.contains(".k"));
    assert!(reasons(&r).contains("supports"), "{}", reasons(&r));
}

#[tokio::test]
async fn only_https_public_hosts_and_no_credentials() {
    let strict = Options {
        allow_loopback: false,
        ..opts()
    };
    let src = "@import 'http://example.com/a.css';\n@import 'https://127.0.0.1/a.css';\n@import 'https://localhost/a.css';\n@import 'https://192.168.1.2/a.css';\n@import 'https://[::1]/a.css';\n@import 'https://user:pw@example.com/a.css';\n@import 'file:///etc/passwd';\n@import 'ftp://x/y.css';";
    let r = resolve(src, None, &strict).await;
    assert_eq!(r.notes.len(), 8, "{}", reasons(&r));
    assert!(r.notes.iter().all(|n| n.line >= 1) && r.notes[7].line == 8);
    // the test server is plain http on loopback: refused unless the test switch is on
    let s = serve(|_| vec![("/a.css", css(".a { }"))]).await;
    let r = resolve(&format!("@import '{}';", s.u("/a.css")), None, &strict).await;
    assert_eq!(s.total(), 0);
    assert!(!r.css.contains(".a"));
}

#[tokio::test]
async fn redirects_are_limited_and_stay_on_https() {
    let s = serve(|b| {
        vec![
            ("/ok", redirect(&format!("{b}/final.css"))),
            ("/final.css", css(".final { a: b }")),
            ("/to-http", redirect("http://example.com/x.css")),
            ("/h1", redirect(&format!("{b}/h2"))),
            ("/h2", redirect(&format!("{b}/h3"))),
            ("/h3", redirect(&format!("{b}/h4"))),
            ("/h4", redirect(&format!("{b}/final.css"))),
        ]
    })
    .await;
    let r = resolve(&format!("@import '{}';", s.u("/ok")), None, &opts()).await;
    assert!(r.css.contains(".final") && r.notes.is_empty());
    let r = resolve(&format!("@import '{}';", s.u("/to-http")), None, &opts()).await;
    assert!(reasons(&r).contains("redirect refused"), "{}", reasons(&r));
    // 3 hops are fine, a fourth is not (h1 -> h2 -> h3 -> h4 -> final is 4)
    let r = resolve(&format!("@import '{}';", s.u("/h1")), None, &opts()).await;
    assert!(!r.css.contains(".final"), "{}", r.css);
    assert!(reasons(&r).contains("redirect refused"));
    let r = resolve(&format!("@import '{}';", s.u("/h2")), None, &opts()).await;
    assert!(r.css.contains(".final"), "{}", reasons(&r));
}

#[tokio::test]
async fn slow_servers_time_out() {
    let s = serve(|_| {
        vec![(
            "/slow.css",
            Route {
                delay: Duration::from_millis(1500),
                ..css(".s { }")
            },
        )]
    })
    .await;
    let o = Options {
        timeout: Duration::from_millis(200),
        ..opts()
    };
    let t = Instant::now();
    let r = resolve(
        &format!("@import '{}'; .after {{ }}", s.u("/slow.css")),
        None,
        &o,
    )
    .await;
    assert!(t.elapsed() < Duration::from_millis(1200));
    assert!(reasons(&r).contains("timed out") && r.css.contains(".after"));
}

#[tokio::test]
async fn cache_hit_refresh_and_stale_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let s = serve(|_| vec![("/c.css", css(".c { a: b }"))]).await;
    let o = Options {
        cache_dir: Some(dir.path().to_path_buf()),
        ..opts()
    };
    let src = format!("@import '{}';", s.u("/c.css"));
    let r = resolve(&src, None, &o).await;
    assert_eq!((r.downloaded, r.from_cache, s.hits("/c.css")), (1, 0, 1));
    assert!(o.fetched_at(&s.u("/c.css")).is_some());
    // second time, even with the server gone: the cache answers
    s.remove("/c.css");
    let r = resolve(&src, None, &o).await;
    assert_eq!((r.downloaded, r.from_cache, s.hits("/c.css")), (0, 1, 1));
    assert!(r.css.contains(".c") && r.notes.is_empty());
    // manual refresh with the server broken: the saved copy is used and the note says so
    let r = resolve(
        &src,
        None,
        &Options {
            refresh: true,
            ..o.clone()
        },
    )
    .await;
    assert_eq!(s.hits("/c.css"), 2);
    assert!(
        r.css.contains(".c") && reasons(&r).contains("saved copy"),
        "{}",
        reasons(&r)
    );
    // a fresh refresh replaces the cache
    s.routes
        .lock()
        .unwrap()
        .insert("/c.css".into(), css(".c2 { a: b }"));
    let r = resolve(
        &src,
        None,
        &Options {
            refresh: true,
            ..o.clone()
        },
    )
    .await;
    assert!(r.css.contains(".c2") && r.notes.is_empty());
    assert!(resolve(&src, None, &o).await.css.contains(".c2"));
}

#[test]
fn cache_is_bounded_and_tamper_proof() {
    let d = tempfile::tempdir().unwrap();
    for i in 0..(CACHE_FILES + 20) {
        cache_write(d.path(), &format!("https://e.example/{i}"), "x", b"1");
    }
    assert!(std::fs::read_dir(d.path()).unwrap().count() <= CACHE_FILES);
    cache_write(
        d.path(),
        "https://e.example/k",
        "https://e.example/k",
        b"body",
    );
    assert_eq!(
        cache_read(d.path(), "https://e.example/k").unwrap().bytes,
        b"body"
    );
    std::fs::write(cache_file(d.path(), "https://e.example/k"), b"garbage").unwrap();
    assert!(cache_read(d.path(), "https://e.example/k").is_none());
}

#[tokio::test]
async fn setting_off_fetches_nothing_and_keeps_the_parser_warning() {
    let s = serve(|_| vec![("/a.css", css(".a { }"))]).await;
    let src = format!(
        "@import '{}';\n@font-face {{ font-family: X; src: url(x.ttf) }}",
        s.u("/a.css")
    );
    let r = resolve(
        &src,
        None,
        &Options {
            remote: false,
            ..opts()
        },
    )
    .await;
    assert_eq!(
        (r.css.as_str(), s.total(), r.fonts.len()),
        (src.as_str(), 0, 0)
    );
    let p = parse_resolved(&r).unwrap();
    assert!(
        p.warnings
            .iter()
            .any(|w| w.message.contains("`@import` is not fetched"))
    );
    assert!(!has_remote(".a { color: red }") && has_remote("@IMPORT 'x';"));
}

// ---- fonts -----------------------------------------------------------------------------------

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}
fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

/// The smallest table set `ttf-parser` accepts, with `family` in the name table.
fn tables(family: &str) -> Vec<([u8; 4], Vec<u8>)> {
    let mut head = vec![0u8; 54];
    head[12..16].copy_from_slice(&be32(0x5F0F3CF5));
    head[18..20].copy_from_slice(&be16(1000));
    let mut hhea = vec![0u8; 36];
    hhea[4..6].copy_from_slice(&be16(800));
    hhea[6..8].copy_from_slice(&be16(0xFF38)); // -200
    let mut maxp = be32(0x0000_5000).to_vec();
    maxp.extend(be16(1));
    let utf16: Vec<u8> = family
        .encode_utf16()
        .flat_map(|u| u.to_be_bytes())
        .collect();
    let mut name = vec![];
    name.extend(be16(0));
    name.extend(be16(1));
    name.extend(be16(6 + 12));
    for v in [3u16, 1, 0x409, 1, utf16.len() as u16, 0] {
        name.extend(be16(v));
    }
    name.extend(utf16);
    vec![
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"maxp", maxp),
        (*b"name", name),
    ]
}

fn pad4(v: &mut Vec<u8>) {
    while !v.len().is_multiple_of(4) {
        v.push(0);
    }
}

fn ttf(family: &str) -> Vec<u8> {
    let t = tables(family);
    let mut out = vec![0, 1, 0, 0];
    out.extend(be16(t.len() as u16));
    out.extend([0u8; 6]);
    let mut off = 12 + 16 * t.len();
    let mut dir = vec![];
    let mut data = vec![];
    for (tag, body) in &t {
        dir.extend(tag);
        dir.extend(be32(0));
        dir.extend(be32(off as u32));
        dir.extend(be32(body.len() as u32));
        let mut b = body.clone();
        pad4(&mut b);
        off += b.len();
        data.extend(b);
    }
    out.extend(dir);
    out.extend(data);
    out
}

/// WOFF 1 with stored (uncompressed) tables, which the format allows.
fn woff1(family: &str) -> Vec<u8> {
    let t = tables(family);
    let sfnt = ttf(family);
    let mut out = b"wOFF".to_vec();
    out.extend([0, 1, 0, 0]);
    let total = 44 + 20 * t.len() + t.iter().map(|(_, b)| (b.len() + 3) & !3).sum::<usize>();
    out.extend(be32(total as u32));
    out.extend(be16(t.len() as u16));
    out.extend(be16(0));
    out.extend(be32(sfnt.len() as u32));
    out.extend([0u8; 4 + 20]);
    let mut off = 44 + 20 * t.len();
    let mut data = vec![];
    for (tag, body) in &t {
        out.extend(tag);
        out.extend(be32(off as u32));
        out.extend(be32(body.len() as u32));
        out.extend(be32(body.len() as u32));
        out.extend(be32(0));
        let mut b = body.clone();
        pad4(&mut b);
        off += b.len();
        data.extend(b);
    }
    out.extend(data);
    out
}

fn base128(mut v: u32) -> Vec<u8> {
    let mut b = vec![(v & 0x7f) as u8];
    v >>= 7;
    while v > 0 {
        b.push((v & 0x7f) as u8 | 0x80);
        v >>= 7;
    }
    b.reverse();
    b
}

/// WOFF 2 whose Brotli stream is one uncompressed meta-block (valid Brotli, no encoder needed).
fn woff2(family: &str) -> Vec<u8> {
    let t = tables(family);
    let known = |tag: &[u8; 4]| match tag {
        b"head" => 1u8,
        b"hhea" => 2,
        b"maxp" => 4,
        b"name" => 5,
        _ => unreachable!(),
    };
    let mut dir = vec![];
    let mut raw = vec![];
    for (tag, body) in &t {
        dir.push(known(tag));
        dir.extend(base128(body.len() as u32));
        raw.extend(body);
    }
    let mut z = ((raw.len() as u32 - 1) << 4 | 1 << 20).to_le_bytes()[..3].to_vec();
    z.extend(&raw);
    z.push(0x03);
    let sfnt = ttf(family);
    let mut out = b"wOF2".to_vec();
    out.extend([0, 1, 0, 0]);
    let len = 48 + dir.len() + ((z.len() + 3) & !3);
    out.extend(be32(len as u32));
    out.extend(be16(t.len() as u16));
    out.extend(be16(0));
    out.extend(be32(sfnt.len() as u32));
    out.extend(be32(z.len() as u32));
    out.extend([0u8; 4 + 20]);
    out.extend(dir);
    out.extend(&z);
    pad4(&mut out);
    out
}

fn face(family: &str, weight: u16, url: &str) -> String {
    format!(
        "@font-face {{ font-family: '{family}'; font-weight: {weight}; src: url({url}) format('truetype'); }}\n"
    )
}

#[test]
fn the_fixtures_are_real_fonts() {
    for f in [
        ttf("Fixture Sans"),
        woff1("Fixture Sans"),
        woff2("Fixture Sans"),
    ] {
        let sfnt = fonts::to_sfnt(&f).unwrap_or_else(|e| panic!("{e}: {:?}", &f[..4]));
        assert_eq!(
            fonts::validate(&sfnt).unwrap().as_deref(),
            Some("Fixture Sans")
        );
    }
}

#[tokio::test]
async fn fonts_in_every_format_with_internal_names() {
    let s = serve(|_| {
        vec![
            ("/a.ttf", route("font/ttf", ttf("Alpha"))),
            ("/b.woff", route("application/font-woff", woff1("Beta"))),
            ("/c.woff2", route("font/woff2", woff2("Gamma"))),
            ("/d.bin", route("application/octet-stream", ttf("Delta"))),
        ]
    })
    .await;
    let src = format!(
        "@font-face {{ font-family: Alpha; src: local(Alpha), url({}) format('truetype') }}\n\
         @font-face {{ font-family: \"Beta\"; font-weight: bold; font-style: italic; src: url({}) format('woff') }}\n\
         @font-face {{ font-family: Gamma; src: url({}) format('woff2') }}\n\
         @font-face {{ font-family: Delta; src: url({}) }}\n.k {{ font-family: Alpha }}",
        s.u("/a.ttf"),
        s.u("/b.woff"),
        s.u("/c.woff2"),
        s.u("/d.bin")
    );
    let r = resolve(&src, None, &opts()).await;
    assert!(r.notes.is_empty(), "{}", reasons(&r));
    let f: Vec<_> = r
        .fonts
        .iter()
        .map(|f| {
            (
                f.family.as_str(),
                f.weight,
                f.style,
                f.internal_family.clone().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        f,
        [
            ("Alpha", 400, FontStyle::Normal, "Alpha".to_string()),
            ("Beta", 700, FontStyle::Italic, "Beta".to_string()),
            ("Gamma", 400, FontStyle::Normal, "Gamma".to_string()),
            ("Delta", 400, FontStyle::Normal, "Delta".to_string()),
        ]
    );
    assert!(
        r.fonts.iter().all(|f| f.bytes.starts_with(&[0, 1, 0, 0])),
        "woff and woff2 are decoded to TTF"
    );
    // the @font-face blocks are gone, the rest and the line count stay
    assert!(!r.css.contains("@font-face") && r.css.contains(".k {"));
    assert_eq!(lines(&r.css), lines(&src));
    assert!(
        !parse_resolved(&r)
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.kind == WarningKind::FontNotDownloaded)
    );
}

#[tokio::test]
async fn ttf_is_preferred_over_woff2_and_a_bad_source_falls_through() {
    let s = serve(|_| {
        vec![
            (
                "/bad.ttf",
                route("font/ttf", b"<html>not a font</html>".to_vec()),
            ),
            ("/good.woff2", route("font/woff2", woff2("Good"))),
            ("/both.ttf", route("font/ttf", ttf("Both"))),
            ("/both.woff2", route("font/woff2", woff2("Both"))),
        ]
    })
    .await;
    let src = format!(
        "@font-face {{ font-family: A; src: url({}) format('woff2'), url({}) format('truetype') }}\n\
         @font-face {{ font-family: B; src: url({}) format('truetype'), url({}) format('woff2') }}",
        s.u("/good.woff2"),
        s.u("/bad.ttf"),
        s.u("/both.ttf"),
        s.u("/both.woff2")
    );
    let r = resolve(&src, None, &opts()).await;
    assert_eq!(r.fonts.len(), 2, "{}", reasons(&r));
    // A: the TTF is tried first, fails its signature check, the WOFF2 is used
    assert_eq!(s.hits("/bad.ttf"), 1);
    assert!(r.fonts[0].url.ends_with("/good.woff2"));
    // B: only the TTF was downloaded
    assert_eq!((s.hits("/both.ttf"), s.hits("/both.woff2")), (1, 0));
}

#[tokio::test]
async fn font_checks_content_type_magic_bytes_and_size() {
    let mut huge = ttf("Huge");
    huge.resize(fonts::MAX_FILE + 1, 0);
    let s = serve(|_| {
        vec![
            ("/html.ttf", route("text/html", ttf("X"))),
            (
                "/magic.ttf",
                route("font/ttf", b"%PDF-1.4 not a font at all".to_vec()),
            ),
            (
                "/empty.ttf",
                route("font/ttf", vec![0, 1, 0, 0, 0, 0, 0, 0]),
            ),
            ("/huge.ttf", route("font/ttf", huge)),
            ("/nope.eot", route("font/ttf", ttf("X"))),
        ]
    })
    .await;
    let src = format!(
        "{}{}{}{}@font-face {{ font-family: E; src: url({}) format('embedded-opentype') }}",
        face("A", 400, &s.u("/html.ttf")),
        face("B", 400, &s.u("/magic.ttf")),
        face("C", 400, &s.u("/empty.ttf")),
        face("D", 400, &s.u("/huge.ttf")),
        s.u("/nope.eot")
    );
    let r = resolve(&src, None, &opts()).await;
    assert!(r.fonts.is_empty());
    let why = reasons(&r);
    for want in [
        "not a font (content type",
        "unknown file signature",
        "not a valid font",
        "larger than 2048 KB",
        "no loadable source",
    ] {
        assert!(why.contains(want), "{want} in {why}");
    }
    assert_eq!(s.hits("/nope.eot"), 0, "EOT is never downloaded");
    assert!(r.notes.iter().all(|n| n.line >= 1));
}

#[tokio::test]
async fn font_file_count_and_total_size_limits() {
    let mut padded = ttf("Pad");
    padded.resize(1_900_000, 0);
    let mut routes: Vec<(&'static str, Route)> = vec![];
    let names: Vec<&'static str> = (0..26)
        .map(|i| &*Box::leak(format!("/f{i}.ttf").into_boxed_str()))
        .collect();
    for n in &names {
        routes.push((n, route("font/ttf", ttf("Small"))));
    }
    let p: Vec<&'static str> = (0..5)
        .map(|i| &*Box::leak(format!("/p{i}.ttf").into_boxed_str()))
        .collect();
    for n in &p {
        routes.push((n, route("font/ttf", padded.clone())));
    }
    let s = serve(|_| routes).await;
    // 26 different weights: 24 files are loaded
    let src: String = names
        .iter()
        .enumerate()
        .map(|(i, n)| face("Small", 100 + i as u16 * 10, &s.u(n)))
        .collect();
    let r = resolve(&src, None, &opts()).await;
    assert_eq!(r.fonts.len(), fonts::MAX_FILES, "{}", reasons(&r));
    let capped: Vec<_> = r
        .notes
        .iter()
        .filter(|n| n.reason.contains("font file(s) were not loaded"))
        .collect();
    assert_eq!(capped.len(), 1, "one note, not one per file");
    assert!(
        capped[0].reason.starts_with("2 more"),
        "{}",
        capped[0].reason
    );
    // 5 x 1.9 MB: the fifth passes 8 MB
    let src: String = p
        .iter()
        .enumerate()
        .map(|(i, n)| face("Pad", 100 + i as u16 * 100, &s.u(n)))
        .collect();
    let r = resolve(&src, None, &opts()).await;
    assert_eq!(r.fonts.len(), 4, "{}", reasons(&r));
    assert!(reasons(&r).contains("8 MB limit"));
}

#[tokio::test]
async fn google_style_import_with_subsets_relative_urls_and_data_uri() {
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(ttf("Inline"));
    let s = serve(|b| {
        vec![
            (
                "/css2",
                css(&format!(
                    "/* cyrillic */\n@font-face {{ font-family: 'Inter'; font-weight: 400; src: url({b}/fonts/cyr.ttf) format('truetype'); unicode-range: U+0400-045F; }}\n\
                     /* latin */\n@font-face {{ font-family: 'Inter'; font-weight: 400; src: url(fonts/latin.ttf) format('truetype'); unicode-range: U+0000-00FF, U+0131; }}\n\
                     @font-face {{ font-family: 'Inline'; src: url(data:font/ttf;base64,{data}); }}"
                )),
            ),
            ("/fonts/cyr.ttf", route("font/ttf", ttf("Inter"))),
            ("/fonts/latin.ttf", route("font/ttf", ttf("Inter"))),
        ]
    })
    .await;
    let r = resolve(&format!("@import url('{}');", s.u("/css2")), None, &opts()).await;
    assert_eq!(r.fonts.len(), 2, "{}", reasons(&r));
    assert!(
        r.fonts[0].url.ends_with("/fonts/latin.ttf"),
        "relative to the importing sheet"
    );
    assert_eq!(
        (s.hits("/fonts/cyr.ttf"), s.hits("/fonts/latin.ttf")),
        (0, 1)
    );
    assert!(
        r.notes.iter().all(|n| n.info),
        "only the skipped-subsets note: {}",
        reasons(&r)
    );
    assert!(r.fonts[1].url.starts_with("data:"));
}

#[tokio::test]
async fn font_cache_and_setting_off() {
    let dir = tempfile::tempdir().unwrap();
    let s = serve(|_| vec![("/a.woff2", route("font/woff2", woff2("Cached")))]).await;
    let o = Options {
        cache_dir: Some(dir.path().to_path_buf()),
        ..opts()
    };
    let src = face("Cached", 400, &s.u("/a.woff2")).replace("truetype", "woff2");
    assert_eq!(resolve(&src, None, &o).await.fonts.len(), 1);
    s.remove("/a.woff2");
    let r = resolve(&src, None, &o).await;
    assert_eq!((r.fonts.len(), r.from_cache, s.hits("/a.woff2")), (1, 1, 1));
    let r = resolve(&src, None, &Options { remote: false, ..o }).await;
    assert!(r.fonts.is_empty() && r.css == src);
}

#[test]
fn built_in_themes_are_only_the_default_and_glass() {
    let ids: Vec<_> = BUILTIN.iter().map(|t| t.id).collect();
    assert_eq!(ids, ["nocturne", "glass"]);
    assert!(builtin("nocturne").unwrap().css.is_empty() && builtin("missing").is_none());
    // No third-party theme address is baked into the code.
    assert!(
        !include_str!("../import.rs")
            .to_ascii_lowercase()
            .contains("elegantfin")
    );
}

#[tokio::test]
async fn a_pasted_import_of_a_large_minified_theme_with_fonts() {
    // ~90 KB on one line, as minified Jellyfin themes are, with its own @import of a font sheet.
    let rule = ".skinHeader{background:#111;color:#eee}.cardBox{border-radius:12px}";
    let big = format!(
        "@import url(\"/fonts.css\");:root{{--accentColor:#00a4dc}}{}",
        rule.repeat(1300)
    );
    assert!(big.len() > 85_000);
    let s = serve(|_| {
        vec![
            ("/theme.css", css(&big)),
            (
                "/fonts.css",
                css("@font-face{font-family:Tiny;src:url(/t.ttf) format('truetype')}"),
            ),
            ("/t.ttf", route("font/ttf", ttf("Tiny"))),
        ]
    })
    .await;
    let pasted = format!("@import url(\"{}\");", s.u("/theme.css"));
    let r = resolve(&pasted, None, &opts()).await;
    assert!(r.notes.is_empty(), "{}", reasons(&r));
    assert!(r.css.len() > 85_000 && r.fonts.len() == 1);
    assert!(parse_resolved(&r).unwrap().theme.accent.is_some());
}

#[test]
fn no_write_verbs_and_no_credentials_in_the_fetch_code() {
    let src = include_str!("../import.rs");
    for bad in [
        ".post(",
        ".put(",
        ".delete(",
        ".patch(",
        "cookie_store",
        "bearer_auth",
        ".basic_auth(",
        "danger_accept",
    ] {
        assert!(!src.to_ascii_lowercase().contains(bad), "{bad}");
    }
}

/// Google Fonts' `css2` format for a variable family: one `@font-face` per script subset and
/// weight, the same file for every weight of a subset.
#[tokio::test]
async fn google_variable_family_is_fetched_once_per_wanted_subset() {
    let s = serve(|b| {
        let mut sheet = String::new();
        for w in [400, 500, 700] {
            for (name, file, range) in [
                ("cyrillic", "cyr", "U+0301, U+0400-045F, U+0490-0491"),
                ("greek", "greek", "U+0370-0377, U+037A-037F"),
                ("latin-ext", "ext", "U+0100-02BA, U+02BD-02C5, U+1E00-1EFF"),
                ("latin", "latin", "U+0000-00FF, U+0131, U+0152-0153"),
            ] {
                sheet += &format!(
                    "/* {name} */\n@font-face {{\n  font-family: 'Inter';\n  font-style: normal;\n  font-weight: {w};\n  font-display: swap;\n  src: url({b}/s/inter/{file}.ttf) format('truetype');\n  unicode-range: {range};\n}}\n"
                );
            }
        }
        vec![
            ("/css2", css(&sheet)),
            ("/s/inter/cyr.ttf", route("font/ttf", ttf("Inter"))),
            ("/s/inter/greek.ttf", route("font/ttf", ttf("Inter"))),
            ("/s/inter/ext.ttf", route("font/ttf", ttf("Inter"))),
            ("/s/inter/latin.ttf", route("font/ttf", ttf("Inter"))),
        ]
    })
    .await;
    let r = resolve(&format!("@import url('{}');", s.u("/css2")), None, &opts()).await;
    assert_eq!(
        r.fonts.len(),
        6,
        "latin and latin-ext for 3 weights: {}",
        reasons(&r)
    );
    assert!(r.fonts[0].url.ends_with("/latin.ttf"), "Latin comes first");
    assert_eq!(
        (
            s.hits("/s/inter/latin.ttf"),
            s.hits("/s/inter/ext.ttf"),
            s.hits("/s/inter/cyr.ttf"),
            s.hits("/s/inter/greek.ttf")
        ),
        (1, 1, 0, 0),
        "one download per distinct file, other scripts never"
    );
    assert_eq!(
        r.notes.len(),
        1,
        "one line, not one per file: {}",
        reasons(&r)
    );
    assert!(r.notes[0].info && r.notes[0].reason.starts_with("Inter: 2 subset"));
}
