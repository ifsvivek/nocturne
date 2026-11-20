//! The item detail page as jellyfin-web 12 builds it (Jellyfin layout mode): `#itemDetailPage.itemDetailPage` with `.itemBackdrop`, `.detailLogo`, `.detailPageWrapperContainer > .detailPagePrimaryContainer` (the poster in `.detailImageContainer`, the `.detailRibbon` with `.itemName`, `.itemMiscInfo` and `.mainDetailButtons`), `.detailPagePrimaryContent` (tagline, overview, the people lines) and the `.verticalSection.detailVerticalSection` strips of cards. The header is its own scene above (`app/jf`). Interactive leaves keep the page's own focus model (`DRow`, `Msg`).
use super::detail_tree::{DTree, kind_attr};
use super::model::{self, DRow, Sec};
use super::{Detail, Msg, geom};
use crate::app::CardIn;
use crate::app::jf::card::{Look, Shape, card_doc};
use crate::widget::cell::Pic;
use nocturne_core::source::{Art, Kind, Media, fmt_runtime};
use nocturne_style::Id;

impl Detail {
    /// The page as a tree of jellyfin-web's elements.
    pub(super) fn jf_tree(&self) -> DTree {
        let m = &self.media;
        let mut t_ = DTree::new(kind_attr(m.kind));
        let root = jf_root_doc(&mut t_);
        let d = &mut t_.doc;
        let sp = d.add(root, "div.jf-spacer");
        d.inline(sp, "height: 48px");
        let main = d.add(root, "main.MuiBox-root");
        let skin = d.add(main, "div.skinBody");
        let page = d.add(
            skin,
            "div.page.libraryPage.itemDetailPage.noSecondaryNavPage.selfBackdropPage.mainAnimatedPage.noBackdropTransparency",
        );
        d.set_id(page, "itemDetailPage");
        d.attr(page, "data-role", "page");
        // The backdrop: jellyfin-web's fixed `.backdropContainer > .backdropImage` behind the page (its script fades it to a low opacity), and `#itemBackdrop` as the spacer the ribbon overlaps.
        let art = geom::hero_art(m);
        if let Some(h) = self.img(m, art, 1920).or_else(|| self.blur(m, art)) {
            let bc = t_.doc.add(root, "div.backdropContainer");
            t_.pic(bc, "img.backdropImage.backdropImageFadeIn.jfImage", h);
            // jellyfin-web's script adds `withBackdrop` to the page-sized `.backgroundContainer` while a backdrop shows; the theme sheet sets its opacity (0.86 in the dark theme), so the backdrop is seen through a dark layer.
            t_.doc.add(root, "div.backgroundContainer.withBackdrop");
        }
        let d = &mut t_.doc;
        let bd = d.add(page, "div.itemBackdrop");
        d.set_id(bd, "itemBackdrop");
        // jellyfin-web always has `.detailLogo`; its script adds `hide` when the item has no logo (themes key off `.detailLogo:not(.hide)`, e.g. to hide the item name under a logo).
        let logo = m
            .logo
            .is_some()
            .then(|| self.img(m, Art::Logo, 800))
            .flatten();
        let lg = t_.doc.add(
            page,
            if logo.is_some() {
                "div.detailLogo"
            } else {
                "div.detailLogo.hide"
            },
        );
        if let Some(l) = logo {
            t_.pic(lg, "img.jfImage.jfContain", l);
        }
        let d = &mut t_.doc;
        let wrap = d.add(page, "div.detailPageWrapperContainer");
        let primary = d.add(wrap, "div.detailPagePrimaryContainer");
        // The poster.
        let ic = d.add(primary, "div.detailImageContainer.hide-mobile");
        let poster = self
            .img(m, Art::Poster, 480)
            .or_else(|| self.blur(m, Art::Poster));
        let kind_card = if m.kind == Kind::Episode {
            "div.card.backdropCard"
        } else {
            "div.card.portraitCard"
        };
        let card = d.add(ic, kind_card);
        let bx = d.add(card, "div.cardBox");
        let sc = d.add(bx, "div.cardScalable");
        d.add(
            sc,
            if m.kind == Kind::Episode {
                "div.cardPadder.cardPadder-backdrop"
            } else {
                "div.cardPadder.cardPadder-portrait"
            },
        );
        let ci = d.add(sc, "div.cardImageContainer.coveredImage.cardContent");
        if let Some(h) = poster {
            t_.pic(ci, "img.jfImage", h);
        }
        // The ribbon: title, facts, buttons.
        let d = &mut t_.doc;
        let rib = d.add(primary, "div.detailRibbon.padded-left.padded-right");
        let info = d.add(rib, "div.infoWrapper");
        let nc = d.add(info, "div.nameContainer");
        if let Some(parent) = m.series_name.as_deref().filter(|_| m.kind != Kind::Series) {
            let p = d.add(nc, "h3.parentName.infoText");
            let bdi = d.add(p, "bdi");
            d.text(bdi, parent);
        }
        let h1 = d.add(nc, "h1.itemName.infoText.parentNameLast");
        let bdi = d.add(h1, "bdi");
        d.text(bdi, &m.title);
        let misc = d.add(info, "div.itemMiscInfo.itemMiscInfo-primary");
        let item = |d: &mut nocturne_style::Doc, class: &str, s: &str| {
            let n = d.add(misc, &format!("div.mediaInfoItem{class}"));
            d.text(n, s);
        };
        if let Some(y) = m.year {
            item(d, "", &y.to_string());
        }
        if let Some(r) = m.runtime_min {
            item(d, "", &fmt_runtime(r));
        }
        if let Some(c) = self
            .full
            .as_ref()
            .and_then(|f| f.official_rating.as_deref())
        {
            item(d, ".mediaInfoText.mediaInfoOfficialRating", c);
        }
        if let Some(r) = m.rating {
            let n = d.add(misc, "div.mediaInfoItem.starRatingContainer");
            icon(d, n, "starIcon", "star", false, "\u{2605}");
            // An element holds either text or children here, so the number sits in its own span (the web has a bare text node).
            let v = d.add(n, "span.starRating");
            d.text(v, &format!("{r:.1}"));
        }
        if let Some(c) = self.full.as_ref().and_then(|f| f.critic) {
            item(
                d,
                ".mediaInfoCriticRating.mediaInfoCriticRatingFresh",
                &format!("{c:.0}"),
            );
        }
        // jellyfin-web ends the facts with "Ends at" (now + what is left), for anything with a video and a runtime.
        if let Some(r) = m.runtime_min
            && matches!(m.kind, Kind::Movie | Kind::Episode | Kind::Video)
        {
            let left = m
                .progress()
                .map_or(r, |p| ((1.0 - p) * r as f32).round() as u32);
            let n = d.add(misc, "div.mediaInfoItem.endsAt");
            d.text(
                n,
                &format!(
                    "Ends at {}",
                    nocturne_core::detail::ends_at(nocturne_core::detail::local_minutes(), left)
                ),
            );
        }
        let btns = d.add(rib, "div.mainDetailButtons.focuscontainer-x");
        for (i, a) in self.dacts().into_iter().enumerate() {
            let d = &mut t_.doc;
            let (label, _) = model::spec(a, m);
            let (class, name, fallback) = button_of(a);
            let b = jf_button(d, btns, class, label, name, fallback);
            t_.acts.insert(b, Msg::Act(a));
            t_.cell(b, (DRow::Actions, i));
            // Buttons jellyfin-web has and NOCTURNE's own page keeps in menus: they take no focus cell here (clicks only).
            if a == model::DAct::Play && m.progress().is_some() {
                let r = jf_button(&mut t_.doc, btns, "btnReplay", "Play", "replay", "\u{21BA}");
                t_.acts.insert(r, Msg::MoreAct(model::MoreAct::FromStart));
            }
            if a == model::DAct::Play && m.kind == Kind::Series {
                let (c, n, f) = button_of(model::DAct::Shuffle);
                let b = jf_button(&mut t_.doc, btns, c, "Shuffle", n, f);
                t_.acts.insert(b, Msg::Act(model::DAct::Shuffle));
                let (c, n, f) = button_of(model::DAct::Watched);
                let b = jf_button(&mut t_.doc, btns, c, "Mark watched", n, f);
                t_.acts.insert(b, Msg::Act(model::DAct::Watched));
            }
        }
        // Primary content: tagline, overview, the people lines.
        let d = &mut t_.doc;
        let pc = d.add(primary, "div.detailPagePrimaryContent.padded-right");
        let ds = d.add(pc, "div.detailSection");
        if let Some(info) = self
            .full
            .as_ref()
            .and_then(|f| f.info.first())
            .filter(|_| matches!(m.kind, Kind::Movie | Kind::Episode | Kind::Video))
        {
            jf_tracks(d, ds, info);
        }
        let dsc = d.add(ds, "div.detailSectionContent");
        if let Some(tag) = self.full.as_ref().and_then(|f| f.tagline.as_deref()) {
            let h = d.add(dsc, "h3.tagline");
            let b = d.add(h, "bdi");
            d.text(b, tag);
        }
        if let Some(o) = &m.overview {
            let p = d.add(dsc, "p.overview.detail-clamp-text");
            let b = d.add(p, "bdi");
            let t = d.add(b, "p");
            d.text(t, o);
        }
        let grp = d.add(pc, "div.itemDetailsGroup");
        let mut line = |label: &str, vals: Vec<String>| {
            if vals.is_empty() {
                return;
            }
            let w = d.add(grp, "div");
            let it = d.add(w, "div.detailsGroupItem");
            let l = d.add(it, "p.MuiTypography-root.MuiTypography-body1.label");
            d.text(l, label);
            let v = d.add(it, "div.focuscontainer-x");
            for (i, s) in vals.iter().enumerate() {
                let sp = d.add(v, "span");
                let a = d.add(sp, "a.emby-button.button-link");
                // No inline formatting here: the comma travels with the link.
                d.text(
                    a,
                    &if i + 1 < vals.len() {
                        format!("{s},")
                    } else {
                        s.clone()
                    },
                );
            }
        };
        if let Some(f) = &self.full {
            let by = |kind: &str| {
                f.people
                    .iter()
                    .filter(|p| p.sub.as_deref() == Some(kind))
                    .map(|p| p.title.clone())
                    .collect::<Vec<_>>()
            };
            line("Director", by("Director"));
            line("Writer", by("Writer"));
            line("Studio", f.studios.iter().map(|s| s.1.clone()).collect());
        }
        line("Genres", m.genres.clone());
        // Strips of cards below.
        let d = &mut t_.doc;
        let sec2 = d.add(
            wrap,
            "div.detailPageSecondaryContainer.padded-left.padded-bottom-page",
        );
        for s in Sec::ALL {
            let items = self.sec_items(s);
            if items.is_empty() {
                continue;
            }
            // Next up and the seasons sit in the primary column as wrapping grids; cast, similar items and the rest scroll in the secondary container.
            let inline = matches!(s, Sec::Next | Sec::Seasons | Sec::Kids | Sec::Kids2);
            self.jf_strip(&mut t_, if inline { pc } else { sec2 }, s, items, inline);
        }
        t_
    }

    /// One `.verticalSection.detailVerticalSection` of cards.
    fn jf_strip(&self, t_: &mut DTree, parent: Id, s: Sec, items: &[Media], wrapping: bool) {
        let (title, _) = s.title(self.media.kind);
        let title = if s == Sec::Next { "Next Up" } else { title };
        let d = &mut t_.doc;
        let sec = d.add(parent, "div.verticalSection.detailVerticalSection");
        if s == Sec::Cast {
            d.set_id(sec, "castCollapsible");
        }
        let h = d.add(sec, "h2.sectionTitle.sectionTitle-cards");
        d.text(h, title);
        let cont = if wrapping {
            d.add(sec, "div.vertical-wrap.padded-right.itemsContainer")
        } else {
            let sc = d.add(
                sec,
                "div.padded-top-focusscale.padded-bottom-focusscale.no-padding.emby-scroller.scrollX",
            );
            d.add(sc, "div.scrollSlider.focuscontainer-x.itemsContainer")
        };
        let (art, pxw) = geom::strip_art(s);
        let shape = if s.landscape() {
            Shape::Backdrop
        } else {
            Shape::Portrait
        };
        for (i, m) in items.iter().enumerate() {
            let pic = match self.img(m, art, pxw) {
                Some(h) => Pic::Ready(h.clone()),
                None => Pic::Missing,
            };
            let id = match &pic {
                Pic::Ready(h) => t_.reg(h),
                _ => 0,
            };
            let c = CardIn {
                pic,
                pic_id: id,
                key: None,
                title: m.title.clone(),
                meta: String::new(),
                index: i,
                progress: m.progress(),
                badge: None,
                subtitle: false,
                mark: None,
                fav: m.favorite,
                played: m.played,
                writes: false,
                at: None,
                play: false,
                kind: kind_attr(m.kind),
                source: m.origin.0 as usize,
            };
            let look = Look {
                shape,
                overflow: !wrapping,
                second: true,
                folder: matches!(m.kind, Kind::Series | Kind::Season | Kind::Folder),
                person: m.kind == Kind::Person,
            };
            let second = if m.kind == Kind::Person {
                m.sub.clone().unwrap_or_default()
            } else {
                crate::app::jf::second_line(m)
            };
            let b = card_doc(&mut t_.doc, cont, &c, look, &second);
            t_.acts.insert(b.card, Msg::Sec(s, i));
            t_.hover.insert(b.card, Msg::Hover(DRow::Strip(s), i));
            t_.cell(b.card, (DRow::Strip(s), i));
        }
    }
}

/// The detail tree's root as jellyfin-web's `html > body > #reactRoot`.
fn jf_root_doc(t_: &mut DTree) -> Id {
    let d = &mut t_.doc;
    d.nodes[0].tag = "html".into();
    d.nodes[0].classes.retain(|c| c != "page-detail");
    d.nodes[0].classes.push(
        if crate::couch::on() {
            "layout-tv"
        } else {
            "layout-desktop"
        }
        .into(),
    );
    d.attr(0, "data-theme", "dark");
    d.attr(0, "dir", "ltr");
    let body = d.add(0, "body.libraryDocument");
    d.attr(body, "dir", "ltr");
    let root = d.add(body, "div.jf-react");
    d.set_id(root, "reactRoot");
    root
}

/// `(class, material icon, fallback glyph)` of a page action, as jellyfin-web names them.
fn button_of(a: model::DAct) -> (&'static str, &'static str, &'static str) {
    use model::DAct::*;
    match a {
        Play | PlayAll | View => ("btnPlay", "play_arrow", "\u{25B6}"),
        Shuffle => ("btnShuffle", "shuffle", "\u{21C4}"),
        Watched => ("btnPlaystate", "check", "\u{2713}"),
        Fav => ("btnUserRating", "favorite", "\u{2665}"),
        Prev => ("btnPrevious", "navigate_before", "\u{2039}"),
        Next => ("btnNext", "navigate_next", "\u{203A}"),
        Series => ("btnSeries", "tv", "\u{25A3}"),
        Versions => ("btnSplitVersions", "call_split", "\u{2442}"),
        More => ("btnMoreCommands", "more_vert", "\u{22EE}"),
    }
}

/// An icon of the web client's icon font. `named`: an empty `span.material-icons.<name>` whose glyph is its `:before` rule (the buttons); else the name as ligature text in the span (`star` shapes to the glyph). While the font is not available `fallback` is drawn instead, never boxes.
fn icon(
    d: &mut nocturne_style::Doc,
    parent: Id,
    class: &str,
    name: &str,
    named: bool,
    fallback: &str,
) -> Id {
    let on = super::JF_ICONS.load(std::sync::atomic::Ordering::Relaxed);
    let by_class = on && named;
    let s = d.add(
        parent,
        &format!(
            "span.material-icons.{class}{}",
            if by_class {
                format!(".{name}")
            } else {
                String::new()
            }
        ),
    );
    d.attr(s, "aria-hidden", "true");
    if on && !named {
        d.text(s, name);
    } else if !on {
        d.text(s, fallback);
    }
    s
}

/// One `.detailButton` of `.mainDetailButtons`: `button > .detailButton-content > span.material-icons.detailButton-icon`.
fn jf_button(
    d: &mut nocturne_style::Doc,
    parent: Id,
    class: &str,
    title: &str,
    name: &str,
    fallback: &str,
) -> Id {
    let b = d.add(
        parent,
        &format!("button.button-flat.{class}.detailButton.emby-button"),
    );
    d.attr(b, "type", "button");
    d.attr(b, "title", title);
    let c = d.add(b, "div.detailButton-content");
    icon(d, c, "detailButton-icon", name, true, fallback);
    b
}

/// `form.trackSelections`: the Video, Audio and Subtitles lines as `.selectContainer > label.selectLabel + select.detailTrackSelect + .selectArrowContainer`. Read-only here (the select is `disabled`, as jellyfin-web disables Video and a single audio track); the tracks played are chosen in the player.
fn jf_tracks(d: &mut nocturne_style::Doc, parent: Id, info: &nocturne_core::detail::InfoBlock) {
    use nocturne_core::detail::StreamRow;
    let rows = |k: &str| {
        info.streams
            .iter()
            .filter(|s| s.kind == k)
            .collect::<Vec<_>>()
    };
    let (video, audio, subs) = (rows("Video"), rows("Audio"), rows("Subtitle"));
    if video.is_empty() && audio.is_empty() && subs.is_empty() {
        return;
    }
    let form = d.add(parent, "form.trackSelections.focuscontainer-x");
    let title = |r: &StreamRow| {
        if r.title.is_empty() {
            format!("{} {}", r.detail, r.codec).trim().to_string()
        } else {
            r.title.clone()
        }
    };
    let mut line = |name: &str, label: &str, text: &str, enabled: bool| {
        let c = d.add(
            form,
            &format!(
                "div.selectContainer.select{name}Container.trackSelectionFieldContainer.flex-shrink-zero"
            ),
        );
        let l = d.add(c, "label.selectLabel");
        d.text(l, label);
        let sel = d.add(
            c,
            &format!("select.select{name}.detailTrackSelect.emby-select-withcolor.emby-select"),
        );
        if !enabled {
            d.attr(sel, "disabled", "disabled");
        }
        d.text(sel, text);
        if enabled {
            let ac = d.add(c, "div.selectArrowContainer");
            icon(
                d,
                ac,
                "selectArrow",
                "keyboard_arrow_down",
                true,
                "\u{2304}",
            );
        }
    };
    if let Some(v) = video.first() {
        line("Video", "Video", &title(v), false);
    }
    if let Some(a) = audio.first() {
        line("Audio", "Audio", &title(a), audio.len() > 1);
    }
    if !subs.is_empty() {
        line("Subtitles", "Subtitles", "Off", true);
    }
}
