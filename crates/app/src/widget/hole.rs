//! The video hole: a widget that fills its parent, draws nothing (so the compositor shows the
//! video subsurface beneath the transparent window through it) and reports its absolute layout
//! bounds whenever they change. Nothing opaque may be drawn over it except by design (the
//! loading overlay); `black` paints it solid for the moments before the first frame, and for
//! the external-mpv case where there is no video under it.
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Clipboard, Shell, renderer};
use iced::{Color, Element, Event, Length, Rectangle, Size, mouse, window};

pub struct Hole<'a, M> {
    last: Rectangle,
    black: bool,
    on_bounds: Box<dyn Fn(Rectangle) -> M + 'a>,
}

/// `last`: the bounds already known to the owner, so an unchanged layout publishes nothing.
pub fn hole<'a, M: 'a>(
    last: Rectangle,
    black: bool,
    on_bounds: impl Fn(Rectangle) -> M + 'a,
) -> Element<'a, M> {
    Element::new(Hole {
        last,
        black,
        on_bounds: Box::new(on_bounds),
    })
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Hole<'_, M> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        _: &mut Tree,
        _: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fill)
    }

    fn draw(
        &self,
        _: &Tree,
        renderer: &mut iced::Renderer,
        _: &iced::Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        if self.black {
            use iced::advanced::Renderer as _;
            renderer.fill_quad(
                renderer::Quad {
                    bounds: layout.bounds(),
                    ..Default::default()
                },
                Color::BLACK,
            );
        }
    }

    fn update(
        &mut self,
        _: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &iced::Renderer,
        _: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            let b = layout.bounds();
            if b != self.last {
                self.last = b;
                shell.publish((self.on_bounds)(b));
            }
        }
    }
}
