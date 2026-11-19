//! Hover zoom for a card: the content is drawn larger around its centre while the pointer is over
//! it, eased over the theme's transition time. Iced has no CSS transform, so this draws the child
//! through a renderer transformation in its own layer (it may overflow the cell and its clip, and
//! sits above the neighbours) while the layout never changes, so virtualised grids stay exact.
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Renderer as _, Shell, Widget, layout, overlay, renderer};
use iced::animation::{Animation, Easing};
use iced::{Element, Event, Length, Rectangle, Size, Transformation, Vector, mouse, window};
use std::time::{Duration, Instant};

pub struct Zoom<'a, M> {
    content: Element<'a, M>,
    scale: f32,
    ms: u32,
}

struct State {
    over: bool,
    hot: Animation<bool>,
}

/// Wrap `content` so it grows to `scale` on hover; returns it untouched for a scale of 1 or less.
pub fn zoom<'a, M: 'a>(
    content: Element<'a, M>,
    scale: Option<f32>,
    ms: Option<u32>,
) -> Element<'a, M> {
    match scale {
        Some(s) if s > 1.0 => Element::new(Zoom {
            content,
            scale: s,
            ms: ms.unwrap_or(140),
        }),
        _ => content,
    }
}

impl<M> Zoom<'_, M> {
    fn dur(&self) -> Duration {
        crate::motion::dur(Duration::from_millis(self.ms as u64))
    }
}

impl<'a, M: 'a> Widget<M, iced::Theme, iced::Renderer> for Zoom<'a, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            over: false,
            hot: Animation::new(false)
                .easing(Easing::EaseOut)
                .duration(self.dur()),
        })
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        let dur = self.dur();
        let state = tree.state.downcast_mut::<State>();
        let over = cursor.is_over(layout.bounds());
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            if state.hot.value() != state.over {
                // Re-read the motion preference so a Settings change applies to live cards.
                state.hot = state.hot.clone().duration(dur);
                state.hot.go_mut(state.over, *now);
            }
            if state.hot.is_animating(*now) {
                shell.request_redraw();
            }
        } else if over != state.over {
            state.over = over;
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let k = state.hot.interpolate(1.0, self.scale, Instant::now());
        let draw = |r: &mut iced::Renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                r,
                theme,
                style,
                layout,
                cursor,
                viewport,
            )
        };
        if k <= 1.001 {
            return draw(renderer);
        }
        let b = layout.bounds();
        let c = b.center();
        let (dx, dy) = (
            b.width * (k - 1.0) / 2.0 + 4.0,
            b.height * (k - 1.0) / 2.0 + 4.0,
        );
        let grown = Rectangle {
            x: b.x - dx,
            y: b.y - dy,
            width: b.width + 2.0 * dx,
            height: b.height + 2.0 * dy,
        };
        let t = Transformation::translate(c.x, c.y)
            * Transformation::scale(k)
            * Transformation::translate(-c.x, -c.y);
        renderer.with_layer(grown, |r| r.with_transformation(t, draw));
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}
