//! A menu anchored to a widget, drawn as an overlay so no scrollable or container clips it. It
//! opens below the anchor (above when that runs off the window and there is more room), right
//! aligned. A press outside it publishes `on_dismiss` and is swallowed, so "clicking out of an
//! open menu only closes it".
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, overlay, renderer};
use iced::widget::space;
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, mouse};

pub struct Popover<'a, M> {
    base: Element<'a, M>,
    popup: Option<Element<'a, M>>,
    on_dismiss: Option<M>,
    gap: f32,
    left: bool,
    empty: Element<'a, M>,
}

impl<'a, M: Clone + 'a> Popover<'a, M> {
    pub fn new(
        base: impl Into<Element<'a, M>>,
        popup: Option<Element<'a, M>>,
        on_dismiss: M,
    ) -> Self {
        Self {
            base: base.into(),
            popup,
            on_dismiss: Some(on_dismiss),
            gap: 4.0,
            left: false,
            empty: space().into(),
        }
    }

    /// Align the menu's left edge with the anchor's (default: right edges).
    pub fn left(mut self) -> Self {
        self.left = true;
        self
    }
}

impl<'a, M: Clone + 'a> Widget<M, iced::Theme, iced::Renderer> for Popover<'a, M> {
    fn children(&self) -> Vec<Tree> {
        vec![
            Tree::new(&self.base),
            Tree::new(self.popup.as_ref().unwrap_or(&self.empty)),
        ]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.children[0].diff(&self.base);
        tree.children[1].diff(self.popup.as_ref().unwrap_or(&self.empty));
    }

    fn size(&self) -> Size<Length> {
        self.base.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.base.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.base
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
        self.base
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
        self.base.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
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
        self.base.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.base.as_widget().mouse_interaction(
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
        let (base_tree, popup_tree) = tree.children.split_at_mut(1);
        let base = self.base.as_widget_mut().overlay(
            &mut base_tree[0],
            layout,
            renderer,
            viewport,
            translation,
        );
        let popup = self.popup.as_mut().map(|p| {
            overlay::Element::new(Box::new(Menu {
                anchor: layout.bounds() + translation,
                element: p,
                tree: &mut popup_tree[0],
                on_dismiss: self.on_dismiss.clone(),
                gap: self.gap,
                left: self.left,
            }))
        });
        if base.is_none() && popup.is_none() {
            return None;
        }
        Some(overlay::Group::with_children(base.into_iter().chain(popup).collect()).overlay())
    }
}

struct Menu<'a, 'b, M> {
    anchor: Rectangle,
    element: &'b mut Element<'a, M>,
    tree: &'b mut Tree,
    on_dismiss: Option<M>,
    gap: f32,
    left: bool,
}

impl<M: Clone> overlay::Overlay<M, iced::Theme, iced::Renderer> for Menu<'_, '_, M> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let node = self.element.as_widget_mut().layout(
            self.tree,
            renderer,
            &layout::Limits::new(Size::ZERO, bounds),
        );
        let s = node.size();
        let below = self.anchor.y + self.anchor.height + self.gap;
        let above = self.anchor.y - self.gap - s.height;
        let y = if below + s.height > bounds.height && above >= 0.0 {
            above
        } else {
            below
        };
        let x = if self.left {
            self.anchor.x.min(bounds.width - s.width)
        } else {
            self.anchor.x + self.anchor.width - s.width
        }
        .max(0.0);
        node.move_to(Point::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.element.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &Rectangle::with_size(Size::INFINITE),
        );
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
    ) {
        self.element.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &Rectangle::with_size(Size::INFINITE),
        );
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event
            && !shell.is_event_captured()
            && !cursor.is_over(layout.bounds())
        {
            if let Some(m) = &self.on_dismiss {
                shell.publish(m.clone());
            }
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.element.as_widget().mouse_interaction(
            self.tree,
            layout,
            cursor,
            &Rectangle::with_size(Size::INFINITE),
            renderer,
        )
    }
}

impl<'a, M: Clone + 'a> From<Popover<'a, M>> for Element<'a, M> {
    fn from(p: Popover<'a, M>) -> Self {
        Element::new(p)
    }
}
