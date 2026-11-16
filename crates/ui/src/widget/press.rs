//! Button with eased hover: the look is a pure function of [`Status`] (built from theme tokens by
//! the callers), and the widget cross-fades Active -> Hovered over `theme::HOVER`. It asks for the
//! next frame only while the fade runs, so an idle window draws nothing.
use crate::theme as t;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, overlay, renderer};
use iced::animation::{Animation, Easing};
use iced::border::Border;
use iced::widget::button::{Status, Style};
use iced::{
    Background, Color, Element, Event, Length, Padding, Rectangle, Shadow, Size, Vector, mouse,
    touch, window,
};
use std::time::Instant;

pub struct Press<'a, M> {
    content: Element<'a, M>,
    on_press: Option<M>,
    width: Length,
    height: Length,
    padding: Padding,
    style: Box<dyn Fn(Status) -> Style + 'a>,
    status: Option<Status>,
}

struct State {
    pressed: bool,
    hot: Animation<bool>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            pressed: false,
            hot: Animation::new(false)
                .easing(Easing::EaseOut)
                .duration(t::HOVER),
        }
    }
}

impl<'a, M: Clone + 'a> Press<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>) -> Self {
        let content = content.into();
        let size = content.as_widget().size_hint();
        Self {
            content,
            on_press: None,
            width: size.width.fluid(),
            height: size.height.fluid(),
            padding: Padding::ZERO,
            style: Box::new(|_| Style::default()),
            status: None,
        }
    }

    pub fn width(mut self, w: impl Into<Length>) -> Self {
        self.width = w.into();
        self
    }

    pub fn height(mut self, h: impl Into<Length>) -> Self {
        self.height = h.into();
        self
    }

    pub fn padding(mut self, p: impl Into<Padding>) -> Self {
        self.padding = p.into();
        self
    }

    pub fn on_press(mut self, m: M) -> Self {
        self.on_press = Some(m);
        self
    }

    pub fn on_press_maybe(mut self, m: Option<M>) -> Self {
        self.on_press = m;
        self
    }

    pub fn style(mut self, f: impl Fn(Status) -> Style + 'a) -> Self {
        self.style = Box::new(f);
        self
    }
}

fn mix(a: Color, b: Color, k: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * k,
        g: a.g + (b.g - a.g) * k,
        b: a.b + (b.b - a.b) * k,
        a: a.a + (b.a - a.a) * k,
    }
}

fn mix_style(a: Style, b: Style, k: f32) -> Style {
    let bg = match (a.background, b.background) {
        (Some(Background::Color(x)), Some(Background::Color(y))) => {
            Some(Background::Color(mix(x, y, k)))
        }
        // A fill that fades in from nothing keeps its own colour and only its alpha moves.
        (None, Some(Background::Color(y))) => {
            Some(Background::Color(mix(Color { a: 0.0, ..y }, y, k)))
        }
        (Some(Background::Color(x)), None) => {
            Some(Background::Color(mix(x, Color { a: 0.0, ..x }, k)))
        }
        (_, y) => y,
    };
    Style {
        background: bg,
        text_color: mix(a.text_color, b.text_color, k),
        border: Border {
            color: mix(a.border.color, b.border.color, k),
            width: a.border.width + (b.border.width - a.border.width) * k,
            radius: b.border.radius,
        },
        shadow: Shadow {
            color: mix(a.shadow.color, b.shadow.color, k),
            offset: b.shadow.offset,
            blur_radius: a.shadow.blur_radius + (b.shadow.blur_radius - a.shadow.blur_radius) * k,
        },
        snap: b.snap,
    }
}

impl<'a, M: Clone + 'a> Widget<M, iced::Theme, iced::Renderer> for Press<'a, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::padded(limits, self.width, self.height, self.padding, |l| {
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, l)
        })
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |op| {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout.children().next().unwrap(),
                renderer,
                op,
            );
        });
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
            layout.children().next().unwrap(),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if shell.is_event_captured() {
            return;
        }
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. })
                if self.on_press.is_some() && cursor.is_over(layout.bounds()) =>
            {
                state.pressed = true;
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. })
                if state.pressed =>
            {
                state.pressed = false;
                if let (Some(m), true) = (&self.on_press, cursor.is_over(layout.bounds())) {
                    shell.publish(m.clone());
                }
                shell.capture_event();
            }
            Event::Touch(touch::Event::FingerLost { .. }) => state.pressed = false,
            _ => {}
        }
        let over = cursor.is_over(layout.bounds());
        let now_status = if self.on_press.is_none() {
            Status::Disabled
        } else if over && state.pressed {
            Status::Pressed
        } else if over {
            Status::Hovered
        } else {
            Status::Active
        };
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            self.status = Some(now_status);
            let hot = matches!(now_status, Status::Hovered | Status::Pressed);
            if state.hot.value() != hot {
                state.hot.go_mut(hot, *now);
            }
            if state.hot.is_animating(*now) {
                shell.request_redraw();
            }
        } else if self.status.is_some_and(|s| s != now_status) {
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let state = tree.state.downcast_ref::<State>();
        let status = self.status.unwrap_or(Status::Disabled);
        let look = match status {
            Status::Pressed | Status::Disabled => (self.style)(status),
            _ => mix_style(
                (self.style)(Status::Active),
                (self.style)(Status::Hovered),
                state.hot.interpolate(0.0, 1.0, Instant::now()),
            ),
        };
        let bounds = layout.bounds();
        if look.background.is_some() || look.border.width > 0.0 || look.shadow.color.a > 0.0 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: look.border,
                    shadow: look.shadow,
                    snap: look.snap,
                },
                look.background
                    .unwrap_or(Background::Color(Color::TRANSPARENT)),
            );
        }
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            &renderer::Style {
                text_color: look.text_color,
            },
            layout.children().next().unwrap(),
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if self.on_press.is_some() && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
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
            layout.children().next().unwrap(),
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M: Clone + 'a> From<Press<'a, M>> for Element<'a, M> {
    fn from(p: Press<'a, M>) -> Self {
        Element::new(p)
    }
}
