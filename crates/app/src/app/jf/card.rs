//! One jellyfin-web card (`cardBuilder`): `.card > .cardBox > .cardScalable > (.cardPadder + a.cardImageContainer.cardContent + .cardOverlayContainer) + .cardText`s, with the count, played and progress indicators. The picture is an `img` inside the image container (a browser draws it as the container's background image).
use super::super::header_tree::{Tree, dims};
use super::super::home_tree::CardIn;
use super::super::message::{At, CellAct, Message};
use crate::widget::cell::Pic;
use nocturne_style::Id;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    Backdrop,
    Portrait,
    Square,
}

impl Shape {
    fn word(self) -> &'static str {
        match self {
            Shape::Backdrop => "Backdrop",
            Shape::Portrait => "Portrait",
            Shape::Square => "Square",
        }
    }
}

/// How a card sits: in a horizontal scroller (`overflow*Card`) or in a wrapping grid (`*Card`).
#[derive(Clone, Copy)]
pub(crate) struct Look {
    pub shape: Shape,
    pub overflow: bool,
    /// Two caption lines (title, then the second line) or only the title (My Media tiles).
    pub second: bool,
    /// `data-isfolder`: libraries, series and seasons.
    pub folder: bool,
    /// The library tiles show the folder icon over the picture; unused without the icon font.
    pub person: bool,
}

/// jellyfin-web's `getDefaultColorIndex`: which of the five `defaultCardBackground` classes a title gets (the digit sum of the middle character's code, last digit, modulo 5, plus 1).
fn default_bg_index(title: &str) -> usize {
    let u: Vec<u16> = title.encode_utf16().collect();
    let Some(&c) = u.get(u.len() / 2) else {
        return 1;
    };
    let sum: u32 = c.to_string().bytes().map(|b| u32::from(b - b'0')).sum();
    (sum % 10 % 5) as usize + 1
}

/// The icon a card without a picture shows: `CardIn::meta` when the caller knows better (a library's type), else by kind.
fn missing_icon(c: &CardIn) -> &'static str {
    match (c.meta.as_str(), c.kind) {
        ("music", _) | (_, "audio") => "music",
        ("movies", _) | (_, "movie") => "film",
        ("tvshows", _) | (_, "series" | "season" | "episode") => "tv",
        ("boxsets", _) => "layers",
        ("playlists", _) => "queue",
        (_, "person") => "people",
        _ => "folder",
    }
}

pub(crate) struct Built {
    /// The icon node of a card without a picture and its name.
    pub icon: Option<(Id, &'static str)>,
    pub card: Id,
    pub play: Id,
    pub fav: Id,
    pub played: Id,
}

/// A card into `tree`: its picture is registered for a later arrival, its clicks and focus cell bound.
pub(crate) fn card(
    tree: &mut Tree,
    parent: Id,
    mut c: CardIn,
    look: Look,
    subtitle: &str,
) -> Built {
    c.pic_id = tree.pic(c.key.take(), &c.pic);
    let b = card_doc(&mut tree.doc, parent, &c, look, subtitle);
    if let Some((n, name)) = b.icon {
        tree.slot(
            n,
            super::super::header_tree::Slot::Icon {
                name,
                side: 64,
                lit: false,
            },
        );
    }
    if let Some((ri, ci)) = c.at {
        let at = At::Home(ri, ci);
        tree.act(b.played, Message::Cell(at, CellAct::Played));
        tree.act(b.fav, Message::Cell(at, CellAct::Fav));
        tree.act(b.play, Message::Cell(at, CellAct::Play));
        tree.act(b.card, Message::Cell(at, CellAct::Open));
        tree.cell(b.card, (ri, ci));
    }
    b
}

/// The nodes of one card under `parent` (a `Doc` only: the library grid's virtual items are built this way). The picture node uses `c.pic_id`.
pub(crate) fn card_doc(
    d: &mut nocturne_style::Doc,
    parent: Id,
    c: &CardIn,
    look: Look,
    subtitle: &str,
) -> Built {
    let pic_id = c.pic_id;
    let w = look.shape.word();
    let class = if look.overflow {
        format!("div.card.overflow{w}Card")
    } else {
        format!("div.card.{}Card", w.to_lowercase())
    };
    let card = d.add(parent, &class);
    d.nodes[card].classes.push("card-hoverable".into());
    if look.person {
        d.nodes[card].classes.push("personCard".into());
    }
    if c.at.is_some() || c.kind != "other" {
        d.nodes[card].classes.push("card-withuserdata".into());
    }
    d.attr(card, "data-type", &type_of(c.kind));
    d.attr(
        card,
        "data-isfolder",
        if look.folder { "true" } else { "false" },
    );
    d.attr(card, "data-source", &c.source.to_string());
    d.attr(card, "data-index", &c.index.to_string());
    // jellyfin-web sets it on cards of items with a resume position (themes tell Continue Watching apart by it).
    if c.progress.is_some() {
        d.attr(card, "data-positionticks", "1");
    }
    let bx = d.add(card, "div.cardBox.cardBox-bottompadded");
    let scal = d.add(bx, "div.cardScalable");
    let pad = if look.overflow {
        format!("div.cardPadder.cardPadder-overflow{w}")
    } else {
        format!("div.cardPadder.cardPadder-{}", w.to_lowercase())
    };
    d.add(scal, &pad);
    let missing = matches!(c.pic, Pic::Missing);
    let img = d.add(
        scal,
        &if missing {
            format!(
                "a.cardImageContainer.cardContent.itemAction.defaultCardBackground.defaultCardBackground{}",
                default_bg_index(&c.title)
            )
        } else {
            "a.cardImageContainer.coveredImage.cardContent.itemAction".to_string()
        },
    );
    let mut icon = None;
    if missing {
        let n = d.add(img, "span.cardImageIcon.material-icons");
        d.inline(n, "width: 64px; height: 64px");
        icon = Some((n, missing_icon(c)));
    }
    d.attr(img, "data-action", "link");
    if !matches!(c.pic, Pic::Missing) {
        let n = d.add(img, "img.jfImage");
        let (pw, ph) = match &c.pic {
            Pic::Ready(h) | Pic::Blur(h) => dims(h),
            _ => (1, 1),
        };
        d.image(n, pic_id, pw, ph);
    }
    if c.played || c.badge.is_some() {
        let ind = d.add(img, "div.cardIndicators");
        if c.played {
            let n = d.add(ind, "div.playedIndicator.indicator");
            d.text(n, "\u{2713}");
        } else if let Some(b) = &c.badge {
            let n = d.add(ind, "div.countIndicator.indicator");
            d.text(n, b);
        }
    }
    if let Some(p) = c.progress {
        let foot = d.add(
            img,
            "div.innerCardFooter.fullInnerCardFooter.innerCardFooterClear",
        );
        let bar = d.add(foot, "div.itemProgressBar");
        let fg = d.add(bar, "div.itemProgressBarForeground");
        d.inline(
            fg,
            &format!("width: {}%", (p.clamp(0.0, 1.0) * 1000.0).round() / 10.0),
        );
    }
    let over = d.add(scal, "div.cardOverlayContainer.itemAction");
    d.attr(over, "data-action", "link");
    let play = d.add(
        over,
        "button.cardOverlayButton.cardOverlayButton-hover.itemAction.paper-icon-button-light",
    );
    d.attr(play, "data-action", "resume");
    let br = d.add(over, "div.cardOverlayButton-br.flex");
    let played = d.add(
        br,
        "button.cardOverlayButton.cardOverlayButton-hover.itemAction.paper-icon-button-light",
    );
    d.attr(played, "data-action", "none");
    let fav = d.add(
        br,
        "button.cardOverlayButton.cardOverlayButton-hover.itemAction.paper-icon-button-light",
    );
    d.attr(fav, "data-action", "none");
    let first = d.add(bx, "div.cardText.cardTextCentered.cardText-first");
    let bdi = d.add(first, "bdi");
    let a = d.add(bdi, "a.itemAction.textActionButton");
    d.text(a, &c.title);
    if look.second && !subtitle.is_empty() {
        let s = d.add(bx, "div.cardText.cardTextCentered.cardText-secondary");
        let bdi = d.add(s, "bdi");
        d.text(bdi, subtitle);
    }
    Built {
        icon,
        card,
        play,
        fav,
        played,
    }
}

/// `data-type` as jellyfin-web writes it.
fn type_of(kind: &str) -> String {
    match kind {
        "movie" => "Movie",
        "series" => "Series",
        "season" => "Season",
        "episode" => "Episode",
        "photo" => "Photo",
        "book" => "Book",
        "folder" => "CollectionFolder",
        "person" => "Person",
        _ => "Item",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_has_jellyfin_webs_structure() {
        let c = CardIn {
            pic: Pic::Missing,
            pic_id: 0,
            key: None,
            title: "T".into(),
            meta: String::new(),
            index: 0,
            progress: Some(0.5),
            badge: Some("3".into()),
            subtitle: false,
            mark: None,
            fav: false,
            played: false,
            writes: false,
            at: None,
            play: false,
            kind: "movie",
            source: 0,
        };
        let mut d = nocturne_style::Doc::new("html");
        let look = Look {
            shape: Shape::Portrait,
            overflow: true,
            second: true,
            folder: false,
            person: false,
        };
        let b = card_doc(&mut d, 0, &c, look, "2020");
        let has = |d: &nocturne_style::Doc, n: Id, class: &str| {
            d.nodes[n].classes.iter().any(|c| c == class)
        };
        assert!(has(&d, b.card, "overflowPortraitCard") && has(&d, b.card, "card"));
        let bx = d.nodes[b.card].children[0];
        assert!(has(&d, bx, "cardBox"));
        let scal = d.nodes[bx].children[0];
        assert!(has(&d, scal, "cardScalable"));
        let kids: Vec<_> = d.nodes[scal].children.clone();
        assert!(has(&d, kids[0], "cardPadder-overflowPortrait"));
        assert!(has(&d, kids[1], "cardImageContainer"));
        assert!(has(&d, *kids.last().unwrap(), "cardOverlayContainer"));
        // The captions follow the scalable box inside the card box.
        assert!(has(&d, d.nodes[bx].children[1], "cardText-first"));
        assert!(has(&d, d.nodes[bx].children[2], "cardText-secondary"));
        let all: Vec<String> = d.nodes.iter().flat_map(|n| n.classes.clone()).collect();
        assert!(all.contains(&"countIndicator".to_string()));
        assert!(all.contains(&"itemProgressBarForeground".to_string()));
    }
}
