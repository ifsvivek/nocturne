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
            let bi = t_.pic(bc, "img.backdropImage.backdropImageFadeIn.jfImage", h);
            t_.doc.inline(bi, "opacity: 0.2");
        }
        let d = &mut t_.doc;
        let bd = d.add(page, "div.itemBackdrop");
        d.set_id(bd, "itemBackdrop");
        if m.logo.is_some()
            && let Some(l) = self.img(m, Art::Logo, 800)
        {
            let lg = t_.doc.add(page, "div.detailLogo");
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
            item(d, ".starRatingContainer", &format!("\u{2605} {r:.1}"));
        }
        if let Some(c) = self.full.as_ref().and_then(|f| f.critic) {
            item(
                d,
                ".mediaInfoCriticRating.mediaInfoCriticRatingFresh",
                &format!("{c:.0}"),
            );
        }
        let btns = d.add(rib, "div.mainDetailButtons.focuscontainer-x");
        for (i, a) in self.dacts().into_iter().enumerate() {
            let d = &mut t_.doc;
            let (label, _) = model::spec(a, m);
            let class = match a {
                model::DAct::Play => "btnPlay",
                model::DAct::PlayAll => "btnPlay",
                model::DAct::Shuffle => "btnShuffle",
                model::DAct::Watched => "btnPlaystate",
                model::DAct::Fav => "btnUserRating",
                model::DAct::More => "btnMoreCommands",
                _ => "btnMoreCommands",
            };
            let b = d.add(
                btns,
                &format!("button.button-flat.{class}.detailButton.emby-button"),
            );
            d.attr(b, "title", label);
            let c = d.add(b, "div.detailButton-content");
            let icon = d.add(c, "span.detailButton-icon");
            d.inline(icon, "min-width: 1em; min-height: 1em");
            let name = match a {
                model::DAct::Play | model::DAct::PlayAll => "\u{25B6}",
                model::DAct::Watched => "\u{2713}",
                model::DAct::Fav => "\u{2665}",
                model::DAct::Shuffle => "\u{21C4}",
                model::DAct::Prev => "\u{2039}",
                model::DAct::Next => "\u{203A}",
                _ => "\u{22EE}",
            };
            d.text(icon, name);
            t_.acts.insert(b, Msg::Act(a));
            t_.cell(b, (DRow::Actions, i));
        }
        // Primary content: tagline, overview, the people lines.
        let d = &mut t_.doc;
        let pc = d.add(primary, "div.detailPagePrimaryContent.padded-right");
        let ds = d.add(pc, "div.detailSection");
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
    let body = d.add(0, "body.libraryDocument.withSectionTabs");
    d.attr(body, "dir", "ltr");
    let root = d.add(body, "div.jf-react");
    d.set_id(root, "reactRoot");
    root
}
