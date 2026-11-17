//! Source marks: the small label that says where an item or library comes from, with a dot for
//! whether it can be reached right now. Used on cells, rows and the detail page as screens arrive.
use super::text::mono;
use crate::theme as t;
use iced::widget::{container, row, space};
use iced::{Alignment, Color, Element};
use nocturne_core::catalog::{Slot, Status};
use nocturne_core::profile::ProfileKind;

/// Dot colour of a source status: calm blue when fine, amber when unreachable, red when signed out.
pub fn status_color(s: &Status) -> Color {
    match s {
        Status::Online => t::ACCENT_TEXT,
        Status::Connecting | Status::Indexing { .. } => t::TEXT_SECONDARY,
        Status::Offline { .. } => t::WARN,
        Status::SignedOut => t::DANGER,
        Status::Disabled => t::TEXT_TERTIARY,
    }
}

pub fn dot<'a, M: 'a>(c: Color) -> Element<'a, M> {
    container(space())
        .width(7)
        .height(7)
        .style(move |_| container::Style {
            background: Some(c.into()),
            border: crate::style::border(t::NONE, 0.0, 3.5),
            ..Default::default()
        })
        .into()
}

/// What the mark says: LOCAL for the local library, the server's label otherwise.
pub fn mark_text(slot: &Slot) -> String {
    match slot.profile.kind {
        ProfileKind::Local { .. } => "LOCAL".into(),
        ProfileKind::Jellyfin { .. } => slot.profile.label.to_uppercase(),
    }
}

/// Mono micro-label with an availability dot. `item_available` is false for a local item whose
/// folder or file is gone (the dot turns amber whatever the source says).
pub fn source_mark<'a, M: 'a>(slot: &Slot, item_available: bool) -> Element<'a, M> {
    let c = if item_available {
        status_color(&slot.status)
    } else {
        t::WARN
    };
    row![dot(c), mono(mark_text(slot)).color(t::TEXT_SECONDARY)]
        .spacing(t::S1 + 2.0)
        .align_y(Alignment::Center)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocturne_core::profile::Profile;
    use nocturne_core::source::SourceId;

    fn slot(kind: ProfileKind, label: &str) -> Slot {
        Slot {
            id: SourceId(1),
            profile: Profile {
                id: SourceId(1),
                label: label.into(),
                enabled: true,
                online: Default::default(),
                kind,
            },
            backend: None,
            status: Status::Online,
        }
    }

    #[test]
    fn marks_say_local_or_the_server_label() {
        let local = slot(ProfileKind::Local { roots: vec![] }, "Local library");
        let jf = slot(
            ProfileKind::Jellyfin {
                url: "http://home.test".into(),
                user_id: "u".into(),
                user_name: "n".into(),
            },
            "home.test",
        );
        assert_eq!(mark_text(&local), "LOCAL");
        assert_eq!(mark_text(&jf), "HOME.TEST");
        assert_ne!(
            status_color(&Status::Online),
            status_color(&Status::SignedOut)
        );
    }
}
