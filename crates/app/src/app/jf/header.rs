//! jellyfin-web 12's header: an MUI `AppBar` (`header.MuiAppBar-root`) with a toolbar of text buttons for the libraries, icon buttons at the right and the user's avatar, and on library pages a second toolbar with the library's title, count and the play, filter, sort and view buttons. Menus keep their NOCTURNE pop-overs (slots); the icons are NOCTURNE's own.
use super::super::browse::Browse;
use super::super::header_tree::{Slot, Tree, px, text_w};
use super::super::message::Message;
use super::super::{App, GLOBALS};
use crate::theme as t;
use nocturne_core::libview::ViewKind;
use nocturne_core::unified::Link;
use nocturne_style::Id;

const BTN: &str = "MuiButtonBase-root MuiButton-root MuiButton-text";

fn text_button(
    d: &mut nocturne_style::Doc,
    parent: Id,
    large: bool,
    active: bool,
    tag: &str,
) -> Id {
    let size = if large { "Large" } else { "Medium" };
    let color = if active { "Primary" } else { "Inherit" };
    let mut spec = format!("{tag}.{}", BTN.replace(' ', "."));
    spec += &format!(
        ".MuiButton-text{color}.MuiButton-size{size}.MuiButton-textSize{size}.MuiButton-color{color}"
    );
    let n = d.add(parent, &spec);
    d.attr(n, "tabindex", "0");
    n
}

impl App {
    /// The header as jellyfin-web draws it, under `parent`. `library`: the page's own toolbar row (title, count, buttons) is added.
    pub(in crate::app) fn jf_header_nodes(
        &self,
        tree: &mut Tree,
        parent: Id,
        library: Option<(&Browse, u64)>,
    ) -> Id {
        let home = self.stack.first();
        let libs = home.map_or(&[][..], |h| h.libs.items());
        let cur = self
            .stack
            .get(1)
            .and_then(|p| p.media.as_ref())
            .map(|m| (m.origin, m.id.as_str()));
        let at_home = self.stack.len() <= 1;
        let d = &mut tree.doc;
        let header = d.add(
            parent,
            "header.MuiPaper-root.MuiPaper-elevation.MuiPaper-elevation0.MuiAppBar-root.MuiAppBar-colorTransparent.MuiAppBar-positionFixed.mui-fixed.skinHeader",
        );
        let bar = d.add(
            header,
            "div.MuiToolbar-root.MuiToolbar-gutters.MuiToolbar-dense.padded-left.padded-right",
        );
        let left = d.add(bar, "div.MuiStack-root");
        let logo = self.jf_logo().is_some();
        let mut first = true;
        for (n, link) in self.nav_links().into_iter().enumerate() {
            let (label, active, msg): (String, bool, Option<Message>) = match link {
                Link::Home => (
                    self.brand_text(&Default::default()),
                    at_home,
                    Some(Message::GoHome),
                ),
                Link::Kind(k) => (
                    k.label().into(),
                    cur.is_some_and(|c| c.1 == k.scope().key),
                    Some(Message::OpenKind(k)),
                ),
                Link::Lib(i) => (
                    libs[i].title.clone(),
                    Some((libs[i].origin, libs[i].id.as_str())) == cur,
                    Some(Message::OpenLibrary(i)),
                ),
                Link::Global(gi) if GLOBALS[gi].0 != "playlists" => (
                    GLOBALS[gi].1.into(),
                    cur.is_some_and(|c| c.1 == GLOBALS[gi].0),
                    Some(Message::OpenGlobal(gi)),
                ),
                _ => continue,
            };
            let d = &mut tree.doc;
            let large = std::mem::take(&mut first);
            let a = text_button(d, left, large, active && !large, "a");
            d.attr(a, "data-link", &n.to_string());
            let mut icon = super::super::header::link_icon(&link, libs);
            // The Home button shows the web client's logo (a theme swaps it for its own through `content`).
            let brand = matches!(link, Link::Home) && logo;
            if brand {
                d.attr(a, "href", "#/");
                icon = None;
            }
            let ic = d.add(
                a,
                &format!(
                    "span.MuiButton-icon.MuiButton-startIcon.MuiButton-iconSize{}",
                    if large { "Large" } else { "Medium" }
                ),
            );
            let mut slot = None;
            if brand {
                d.add(ic, "img.jf-brand");
            }
            if let Some(name) = icon {
                d.inline(ic, "width: 22px; height: 22px");
                slot = Some((
                    ic,
                    Slot::Icon {
                        name,
                        side: 20,
                        lit: true,
                    },
                ));
            }
            let l = d.add(a, "span.jf-text");
            d.text(l, &label);
            if let Some((n, s)) = slot {
                tree.slot(n, s);
            }
            if let Some(m) = msg {
                tree.act(a, m);
            }
            tree.cell(a, (0, n));
        }
        let d = &mut tree.doc;
        let right = d.add(bar, "div.MuiBox-root.jf-header-right");
        let mut slots: Vec<(Id, Slot)> = vec![];
        if self.catalog.online().count() > 1 {
            let n = d.add(right, "div.slot.header-filter");
            let w = text_w(&self.filter_label(), t::CONTROL_TEXT, 500).min(180.0)
                + 2.0 * t::S3
                + t::S2
                + 12.0;
            d.inline(n, &format!("width: {}; height: 32px", px(w.min(220.0))));
            slots.push((n, Slot::Filter));
        }
        let s = d.add(
            right,
            "a.MuiButtonBase-root.MuiIconButton-root.MuiIconButton-colorInherit.MuiIconButton-sizeLarge",
        );
        d.attr(s, "aria-label", "Search");
        d.essential(s);
        d.focusable(s);
        let si = d.add(s, "span.jf-icon");
        d.inline(si, "width: 24px; height: 24px");
        slots.push((
            si,
            Slot::Icon {
                name: "search",
                side: 24,
                lit: true,
            },
        ));
        tree.act(s, Message::OpenSearch);
        let d = &mut tree.doc;
        let user = d.add(bar, "div.MuiBox-root.jf-header-user");
        let b = d.add(
            user,
            "button.MuiButtonBase-root.MuiIconButton-root.MuiIconButton-colorInherit.MuiIconButton-sizeLarge",
        );
        d.attr(b, "aria-label", "User Menu");
        let av = d.add(
            b,
            "div.MuiAvatar-root.MuiAvatar-circular.MuiAvatar-colorDefault",
        );
        d.inline(av, "width: 40px; height: 40px");
        d.essential(av);
        slots.push((av, Slot::Avatar));
        if let Some((br, pid)) = library {
            self.jf_library_toolbar(tree, header, br, pid, &mut slots);
        }
        for (n, s) in slots {
            tree.slot(n, s);
        }
        header
    }

    /// The second toolbar of a library page: `Movies v  (12)   [Play All][shuffle] [filter][sort][view] [<][>]`.
    fn jf_library_toolbar(
        &self,
        tree: &mut Tree,
        header: Id,
        br: &Browse,
        pid: u64,
        slots: &mut Vec<(Id, Slot)>,
    ) {
        let _ = pid;
        let d = &mut tree.doc;
        let bar = d.add(
            header,
            "div.MuiToolbar-root.MuiToolbar-gutters.MuiToolbar-dense.padded-left.padded-right",
        );
        let title = text_button(d, bar, true, false, "button");
        let h = d.add(title, "span.MuiTypography-root.MuiTypography-h2");
        d.text(h, &br.scope.title);
        let ei = d.add(
            title,
            "span.MuiButton-icon.MuiButton-endIcon.MuiButton-iconSizeLarge",
        );
        d.inline(ei, "width: 20px; height: 20px");
        slots.push((
            ei,
            Slot::Icon {
                name: "caret_down",
                side: 16,
                lit: true,
            },
        ));
        let chipbox = d.add(bar, "div.MuiBox-root.jf-chip-box");
        let chip = d.add(
            chipbox,
            "div.MuiChip-root.MuiChip-filled.MuiChip-sizeMedium.MuiChip-colorDefault.MuiChip-filledDefault",
        );
        let l = d.add(chip, "span.MuiChip-label.MuiChip-labelMedium");
        d.text(l, &br.len().to_string());
        let stack = d.add(bar, "div.MuiStack-root.jf-toolbar-stack");
        let g1 = d.add(stack, "div.MuiBox-root.jf-play-box");
        let grp = d.add(
            g1,
            "div.MuiButtonGroup-root.MuiButtonGroup-contained.MuiButtonGroup-horizontal.MuiButtonGroup-colorPrimary",
        );
        for label in ["Play All", ""] {
            let b = d.add(
                grp,
                "button.MuiButtonBase-root.MuiButton-root.MuiButton-contained.MuiButton-containedPrimary.MuiButton-sizeMedium.MuiButton-containedSizeMedium.MuiButton-colorPrimary.MuiButtonGroup-grouped.MuiButtonGroup-groupedHorizontal.MuiButtonGroup-groupedContained",
            );
            d.attr(b, "title", if label.is_empty() { "Shuffle" } else { label });
            if label.is_empty() {
                d.inline(b, "width: 40px");
            } else {
                let l = d.add(b, "span.jf-text");
                d.text(l, label);
            }
        }
        let g2 = d.add(
            stack,
            "div.MuiButtonGroup-root.MuiButtonGroup-text.MuiButtonGroup-horizontal.MuiButtonGroup-colorInherit",
        );
        let w_of = |s: &str| text_w(s, t::CAPTION + 1.0, 600) + 2.0 * t::S3;
        if br.smart.is_none() {
            let (sort, _) = Self::sort_label(br);
            for (w, s) in [
                (w_of(&Self::filters_label(br)), Slot::Filters),
                (w_of(&sort), Slot::Sort),
            ] {
                let n = d.add(g2, "div.slot.MuiButton-root.MuiButtonGroup-grouped");
                d.inline(n, &format!("width: {}; height: 32px", px(w)));
                slots.push((n, s));
            }
        }
        for v in ViewKind::ALL {
            let n = d.add(g2, "div.slot.MuiButton-root.MuiButtonGroup-grouped");
            d.inline(n, &format!("width: {}; height: 32px", px(w_of(v.label()))));
            slots.push((n, Slot::View(v)));
        }
        let g3 = d.add(
            stack,
            "div.MuiButtonGroup-root.MuiButtonGroup-text.MuiButtonGroup-horizontal.MuiButtonGroup-colorInherit",
        );
        for title in ["Previous", "Next"] {
            let b = d.add(
                g3,
                "button.MuiButtonBase-root.MuiButton-root.MuiButton-text.MuiButton-textInherit.MuiButton-sizeMedium.MuiButton-colorInherit.Mui-disabled.MuiButtonGroup-grouped",
            );
            d.attr(b, "title", title);
            d.inline(b, "width: 40px");
        }
    }
}
