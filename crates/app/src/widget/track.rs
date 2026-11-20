//! Seek / volume track: a thin bar that reports hover, press, drag and release as fractions
//! (0..1) of its width. Draws the faint track, an optional buffered range, chapter ticks, marked
//! segments (intro / credits) and the blue fill. The bar grows from `thin` to `thick` pixels
//! while hot (hovered or dragged).
use crate::style::border;
use crate::theme as t;
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, renderer};
use iced::{Color, Element, Event, Length, Rectangle, Size, mouse};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ev {
    /// Pointer over the track at this fraction, or left it.
    Hover(Option<f32>),
    Down(f32),
    Drag(f32),
    Up(f32),
}

pub struct Track<'a, M> {
    frac: f32,
    cache: f32,
    ticks: Vec<f32>,
    segs: Vec<(f32, f32)>,
    height: f32,
    thin: f32,
    thick: f32,
    on: Box<dyn Fn(Ev) -> M + 'a>,
}

#[derive(Default)]
struct State {
    dragging: bool,
    hot: bool,
}

pub fn track<'a, M: 'a>(frac: f32, on: impl Fn(Ev) -> M + 'a) -> Track<'a, M> {
    Track {
        frac: frac.clamp(0.0, 1.0),
        cache: 0.0,
        ticks: vec![],
        segs: vec![],
        height: 16.0,
        thin: 4.0,
        thick: 8.0,
        on: Box::new(on),
    }
}

impl<'a, M> Track<'a, M> {
    /// End of the buffered range (fraction).
    pub fn cache(mut self, c: f32) -> Self {
        self.cache = c.clamp(0.0, 1.0);
        self
    }
    pub fn ticks(mut self, t: Vec<f32>) -> Self {
        self.ticks = t;
        self
    }
    /// (start, width) fractions.
    pub fn segs(mut self, s: Vec<(f32, f32)>) -> Self {
        self.segs = s;
        self
    }
    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }
    pub fn thickness(mut self, thin: f32, thick: f32) -> Self {
        (self.thin, self.thick) = (thin, thick);
        self
    }
}

impl<'a, M: 'a> From<Track<'a, M>> for Element<'a, M> {
    fn from(t: Track<'a, M>) -> Self {
        Element::new(t)
    }
}

/// Fraction of `b` at pointer `x`.
pub fn frac_at(x: f32, b: Rectangle) -> f32 {
    if b.width <= 0.0 {
        0.0
    } else {
        ((x - b.x) / b.width).clamp(0.0, 1.0)
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Track<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(self.height))
    }

    fn layout(
        &mut self,
        _: &mut Tree,
        _: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fixed(self.height))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _: &iced::Renderer,
        _: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _: &Rectangle,
    ) {
        let st = tree.state.downcast_mut::<State>();
        let b = layout.bounds();
        let over = cursor.is_over(b);
        let x = cursor.position().map(|p| p.x);
        match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if st.dragging {
                    shell.publish((self.on)(Ev::Drag(frac_at(position.x, b))));
                    shell.capture_event();
                } else if over {
                    st.hot = true;
                    shell.publish((self.on)(Ev::Hover(Some(frac_at(position.x, b)))));
                } else if st.hot {
                    st.hot = false;
                    shell.publish((self.on)(Ev::Hover(None)));
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if over => {
                st.dragging = true;
                shell.publish((self.on)(Ev::Down(frac_at(x.unwrap_or(b.x), b))));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if st.dragging => {
                st.dragging = false;
                shell.publish((self.on)(Ev::Up(frac_at(x.unwrap_or(b.x), b))));
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _: &iced::Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let st = tree.state.downcast_ref::<State>();
        let b = layout.bounds();
        let th = if st.hot || st.dragging {
            self.thick
        } else {
            self.thin
        };
        let y = b.y + (b.height - th) / 2.0;
        let mut quad = |x: f32, w: f32, y: f32, h: f32, c: Color, round: bool| {
            if w <= 0.0 {
                return;
            }
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(iced::Point::new(x, y), Size::new(w, h)),
                    border: border(Color::TRANSPARENT, 0.0, if round { h / 2.0 } else { 0.0 }),
                    ..Default::default()
                },
                c,
            );
        };
        quad(b.x, b.width, y, th, t::PROGRESS_TRACK, true);
        let fx = b.x + b.width * self.frac;
        if self.cache > self.frac {
            quad(
                fx,
                b.width * (self.cache - self.frac),
                y + th * 0.25,
                th * 0.5,
                // Buffered range: a thinner, lighter line so it never reads as playback progress.
                Color {
                    a: 0.35,
                    ..Color::WHITE
                },
                false,
            );
        }
        quad(b.x, b.width * self.frac, y, th, t::accent(), true);
        for (s, w) in &self.segs {
            quad(
                b.x + b.width * s,
                b.width * w,
                y + th + 1.0,
                2.0,
                t::accent(),
                false,
            );
        }
        for f in &self.ticks {
            quad(
                b.x + b.width * f,
                2.0,
                y,
                th,
                Color {
                    a: 0.82,
                    ..t::frame()
                },
                false,
            );
        }
    }

    fn mouse_interaction(
        &self,
        _: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _: &Rectangle,
        _: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fraction_clamps_to_the_track() {
        let b = Rectangle::new(iced::Point::new(100.0, 0.0), Size::new(200.0, 16.0));
        assert_eq!(frac_at(50.0, b), 0.0);
        assert_eq!(frac_at(200.0, b), 0.5);
        assert_eq!(frac_at(900.0, b), 1.0);
        assert_eq!(
            frac_at(1.0, Rectangle::new(iced::Point::ORIGIN, Size::ZERO)),
            0.0
        );
    }
}
