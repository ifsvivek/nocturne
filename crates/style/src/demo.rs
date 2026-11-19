//! A static Home screen as the app would build it: header, sidebar, hero and two rows of cards, plus a generator for big trees.
//! Artwork is a per-card gradient in `style=""`: the app sets data-driven values inline, the theme decides everything else.
use crate::doc::{Doc, Id};

fn art(h: u32) -> String {
    format!(
        "background: linear-gradient(135deg, hsl({}, 55%, 42%), hsl({}, 60%, 16%))",
        h % 360,
        (h + 50) % 360
    )
}

const RESUME: &[(&str, &str)] = &[
    ("The Long Dark Harbour", "S1 E4 · 23 min left"),
    ("Paper Lanterns", "S2 E2 · 41 min left"),
    ("Midnight at the Observatory", "1h 12m left"),
    ("Salt and Iron", "S3 E7 · 9 min left"),
    ("Northern Static", "S1 E1 · 36 min left"),
    ("A Quiet Machine", "52 min left"),
    ("Kestrel Row", "S4 E3 · 18 min left"),
    ("The Glass Orchard", "S1 E6 · 44 min left"),
];

const LATEST: &[(&str, &str)] = &[
    ("Ember Season", "2024 · Drama"),
    ("The Cartographer", "2023 · Adventure"),
    ("Lowlight", "2024 · Thriller"),
    ("Hollow Sun", "2022 · Sci-Fi"),
    ("Tidewater", "2024 · Documentary"),
    ("Fox and Fathom", "2023 · Animation"),
    ("Brass Meridian", "2021 · Mystery"),
    ("Last Light Diner", "2024 · Comedy"),
    ("Orchid Protocol", "2023 · Thriller"),
    ("Winter Orchard", "2022 · Drama"),
];

fn card(d: &mut Doc, parent: Id, kind: &str, title: &str, meta: &str, hue: u32, badge: &str) -> Id {
    let c = d.add(parent, "a.card");
    d.attr(c, "data-kind", kind);
    let t = d.add(c, "div.thumb");
    d.inline(t, &art(hue));
    let b = d.add(t, "div.badge");
    d.text(b, badge);
    let p = d.add(t, "div.progress");
    d.attr(
        p,
        "data-watched",
        if kind == "episode" { "true" } else { "false" },
    );
    let ti = d.add(c, "div.card-title");
    d.text(ti, title);
    let m = d.add(c, "div.card-meta");
    d.text(m, meta);
    c
}

pub fn home() -> Doc {
    let mut d = Doc::new("page");
    let root = 0;
    d.attr(root, "data-screen", "home");

    let header = d.add(root, "header.header");
    let logo = d.add(header, "a.logo");
    d.text(logo, "NOCTURNE");
    let nav = d.add(header, "nav.nav");
    for (i, l) in ["Home", "Movies", "Shows", "Music", "Live"]
        .iter()
        .enumerate()
    {
        let a = d.add(
            nav,
            if i == 0 {
                "a.nav-link.active"
            } else {
                "a.nav-link"
            },
        );
        d.text(a, l);
    }
    let s = d.add(header, "div.search");
    d.text(s, "Search titles, people, genres");
    let av = d.add(header, "div.avatar");
    d.text(av, "A");

    let side = d.add(root, "aside.sidebar");
    let h = d.add(side, "div.side-title");
    d.text(h, "Browse");
    for (i, l) in [
        "Movies",
        "Shows",
        "Music",
        "Kids",
        "Documentaries",
        "Collections",
        "Downloads",
    ]
    .iter()
    .enumerate()
    {
        let a = d.add(
            side,
            if i == 0 {
                "a.side-link.active"
            } else {
                "a.side-link"
            },
        );
        d.text(a, l);
    }

    let main = d.add(root, "main.main");
    let hero = d.add(main, "section.hero");
    d.attr(hero, "data-kind", "movie");
    let ha = d.add(hero, "div.hero-art");
    d.inline(
        ha,
        "background: linear-gradient(120deg, hsl(212, 60%, 30%), hsl(262, 55%, 20%) 55%, hsl(335, 60%, 28%))",
    );
    let hb = d.add(hero, "div.hero-body");
    let tag = d.add(hb, "div.hero-tag");
    d.text(tag, "FEATURED FILM");
    let t = d.add(hb, "h1.hero-title");
    d.text(t, "The Long Dark Harbour");
    let m = d.add(hb, "div.hero-meta");
    d.text(m, "2024  ·  2h 08m  ·  Drama, Mystery");
    let p = d.add(hb, "p.hero-text");
    d.text(
        p,
        "When the last ferry stops running, a lighthouse keeper and a stranded cartographer discover that the harbour keeps a record of everyone who ever left it.",
    );
    let acts = d.add(hb, "div.actions");
    let play = d.add(acts, "a.button.primary");
    d.text(play, "Play");
    let more = d.add(acts, "a.button");
    d.text(more, "More info");

    for (sect, title, items, kind) in [
        ("resume", "Continue watching", RESUME, "episode"),
        ("latest", "Latest movies", LATEST, "movie"),
    ] {
        let row = d.add(main, "section.row");
        d.attr(row, "data-section", sect);
        let rt = d.add(row, "h2.row-title");
        d.text(rt, title);
        let cards = d.add(row, "div.cards");
        for (i, (t, m)) in items.iter().enumerate() {
            let badge = if kind == "episode" { "S1 E4" } else { "2h 08m" };
            card(&mut d, cards, kind, t, m, 200 + i as u32 * 37, badge);
        }
    }
    d
}

/// `n` cards in rows of 50 under the same header/hero shell: for cascade and layout timing.
pub fn big(n: usize) -> Doc {
    let mut d = home();
    let main = 0;
    let main = d.nodes[main]
        .children
        .iter()
        .copied()
        .find(|&c| d.nodes[c].tag == "main")
        .expect("main");
    let mut made = 0;
    let mut r = 0;
    while made < n {
        let row = d.add(main, "section.row");
        let rt = d.add(row, "h2.row-title");
        d.text(rt, &format!("Row {r}"));
        let cards = d.add(row, "div.cards");
        for i in 0..50 {
            if made >= n {
                break;
            }
            card(
                &mut d,
                cards,
                "movie",
                "A Title That Is Long Enough To Wrap",
                "2024 · Drama",
                (i * 17) as u32,
                "1h 40m",
            );
            made += 6;
        }
        r += 1;
    }
    d
}
