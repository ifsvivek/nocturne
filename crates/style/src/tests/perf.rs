//! Timings (ignored: run with `cargo test -p nocturne-style --release -- --ignored perf --nocapture`).
use super::*;
use std::time::Instant;

fn us(d: std::time::Duration) -> u128 {
    d.as_micros()
}

fn median(mut v: Vec<u128>) -> u128 {
    v.sort_unstable();
    v[v.len() / 2]
}

fn build(doc: &Doc, css: &str, vp: (f32, f32)) -> (Scene, u128) {
    let t = Instant::now();
    let s = scene(doc, css, vp);
    (s, t.elapsed().as_micros())
}

#[test]
#[ignore]
fn perf() {
    let vp = (1920.0, 1080.0);
    println!(
        "\n{:<28} {:>7} {:>9} {:>9} {:>9} {:>10} {:>10} {:>10}",
        "tree / theme",
        "nodes",
        "cascade",
        "layout",
        "total us",
        "hover us",
        "hover+lay",
        "resize us"
    );
    for (label, doc) in [
        ("home 146", demo::home()),
        ("big 1,000", demo::big(1_000)),
        ("big 10,000", demo::big(10_000)),
    ] {
        for (name, css) in nocturne_themes() {
            let runs: Vec<(u128, u128, u128)> = (0..5)
                .map(|_| {
                    let (s, total) = build(&doc, css, vp);
                    (us(s.timing.cascade), us(s.timing.layout), total)
                })
                .collect();
            let cascade = median(runs.iter().map(|r| r.0).collect());
            let layout = median(runs.iter().map(|r| r.1).collect());
            let total = median(runs.iter().map(|r| r.2).collect());
            let (mut s, _) = build(&doc, css, vp);
            let cards = find(&doc, "card");
            let mut hov = vec![];
            let mut hov_lay = vec![];
            for k in 0..8 {
                let c = cards[(k * 7) % cards.len()];
                let t = Instant::now();
                s.update(
                    Interaction {
                        hover: Some(c),
                        ..Default::default()
                    },
                    Instant::now(),
                    &mut approx,
                )
                .unwrap();
                hov.push(t.elapsed().as_micros());
                let t = Instant::now();
                s.update(Interaction::default(), Instant::now(), &mut approx)
                    .unwrap();
                hov_lay.push(t.elapsed().as_micros());
            }
            let mut rs = vec![];
            for k in 0..5 {
                let t = Instant::now();
                s.resize((1500.0 + k as f32 * 37.0, 900.0), &mut approx);
                rs.push(t.elapsed().as_micros());
            }
            println!(
                "{:<28} {:>7} {:>9} {:>9} {:>9} {:>10} {:>10} {:>10}",
                format!("{label} / {name}"),
                doc.len(),
                cascade,
                layout,
                total,
                median(hov),
                median(hov_lay),
                median(rs)
            );
        }
    }
    // Hover that changes layout (a card grows), and a virtual list of 10,000.
    let mut d = Doc::new("page");
    let row = d.add(0, "div.row");
    for _ in 0..2_000 {
        let c = d.add(row, "a.card");
        d.add(c, "div.thumb");
    }
    let mut s = scene(
        &d,
        ".row { display: flex; flex-wrap: wrap } .card { width: 100px } .thumb { height: 50px } .card:hover { width: 130px }",
        vp,
    );
    let t = Instant::now();
    s.update(
        Interaction {
            hover: Some(1),
            ..Default::default()
        },
        Instant::now(),
        &mut approx,
    )
    .unwrap();
    println!(
        "hover that relayouts a 4,000-node flex-wrap page: {} us",
        t.elapsed().as_micros()
    );
}

fn nocturne_themes() -> Vec<(&'static str, &'static str)> {
    crate::THEMES.to_vec()
}

#[test]
#[ignore]
fn perf_virtual() {
    use crate::virt::VirtBuilder;
    fn item(d: &mut Doc, parent: Id, idx: usize) -> Id {
        let c = d.add(parent, "a.card");
        d.attr(c, "data-i", &idx.to_string());
        d.focusable(c);
        d.add(c, "div.thumb");
        let t = d.add(c, "div.title");
        d.text(t, &format!("Item {idx}"));
        c
    }
    let mut d = Doc::new("page");
    let grid = d.add(0, "div.grid");
    d.virtual_list(grid, 50_000);
    let p = item(&mut d, grid, 0);
    d.virtual_proto(p);
    let mut b = Builders::new();
    b.insert(
        grid,
        Rc::new(|d: &mut Doc, p: Id, i: usize| {
            item(d, p, i);
        }) as VirtBuilder,
    );
    let css = ".grid { padding: 0 20px } .card { display: block; width: 100px; margin-right: 10px; margin-bottom: 20px } .thumb { aspect-ratio: 2 / 3 } .title { height: 20px }";
    let t = Instant::now();
    let mut s = Scene::build(
        d,
        Rc::new(Sheet::parse(css)),
        Rc::new(Fonts::new()),
        (1920.0, 1080.0),
        Interaction::default(),
        None,
        &b,
        &mut approx,
    )
    .unwrap();
    println!(
        "virtual 50,000: build {} us, {} nodes",
        t.elapsed().as_micros(),
        s.doc.len()
    );
    let mut v = vec![];
    for k in 0..40 {
        let t = Instant::now();
        s.wheel(Some(0), 0.0, -(190.0 * (1 + k % 3) as f32), &mut approx);
        v.push(t.elapsed().as_micros());
    }
    println!(
        "virtual 50,000: scroll one to three rows, median {} us, max {} us",
        median(v.clone()),
        v.iter().max().unwrap()
    );
}
