//! Full-window photo viewer: arrows step through the grid's photos, + / - zoom, Esc closes.
use super::cells::g;
use super::{Detail, Msg, Photo};
use crate::theme as t;
use crate::widget::mono;
use iced::widget::{Stack, column, container, image, space};
use iced::{Alignment, Color, ContentFit, Element, Length, Task};
use nocturne_core::source::Art;

impl Detail {
    pub(super) fn photo_key(&mut self, name: &str) -> Task<Msg> {
        let Some(p) = self.photo.as_mut() else {
            return Task::none();
        };
        match name {
            "escape" | "backspace" => self.photo = None,
            "left" | "p" if p.i > 0 => (p.i, p.z) = (p.i - 1, 1.0),
            "right" | "n" if p.i + 1 < p.items.len() => (p.i, p.z) = (p.i + 1, 1.0),
            "=" | "+" | "up" => p.z = (p.z * 1.25).min(4.0),
            "-" | "down" => p.z = (p.z / 1.25).max(1.0),
            "0" => p.z = 1.0,
            _ => {}
        }
        self.want()
    }

    pub(super) fn photo_overlay<'a>(&'a self, p: &'a Photo) -> Element<'a, Msg> {
        let Some(m) = p.items.get(p.i) else {
            return space().into();
        };
        let (w, h) = (self.win.width, self.win.height);
        let pic: Element<'a, Msg> = match self.img(m, Art::Poster, 2560) {
            Some(i) => image(i.clone())
                .width(w * p.z)
                .height(h * p.z)
                .content_fit(ContentFit::Contain)
                .into(),
            None => mono("loading").into(),
        };
        Stack::with_children([
            container(pic)
                .center(Length::Fill)
                .clip(true)
                .style(|_| container::Style {
                    background: Some(Color::BLACK.into()),
                    ..Default::default()
                })
                .into(),
            container(
                column![
                    g(m.title.clone()).size(t::BODY).color(t::text()),
                    mono(format!(
                        "{}/{}  \u{2190} \u{2192} step  + - zoom  esc close",
                        p.i + 1,
                        p.items.len()
                    )),
                ]
                .align_x(Alignment::Center)
                .spacing(t::S1),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::End)
            .padding(t::S6)
            .into(),
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
