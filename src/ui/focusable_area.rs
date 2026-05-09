use icy_ui::advanced::widget::tree::{self, Tree};
use icy_ui::advanced::widget::{Operation, operation};
use icy_ui::advanced::{Clipboard, Layout, Shell, Widget, mouse, overlay, renderer};
use icy_ui::widget::Id;
use icy_ui::widget::focus::FocusRing;
use icy_ui::{Element, Event, Length, Rectangle, Size, Vector, keyboard};

#[derive(Debug, Default)]
struct State {
    is_focused: bool,
}

impl operation::Focusable for State {
    fn is_focused(&self) -> bool {
        self.is_focused
    }

    fn focus(&mut self) {
        self.is_focused = true;
    }

    fn unfocus(&mut self) {
        self.is_focused = false;
    }

    fn focus_tier(&self) -> operation::FocusTier {
        operation::FocusTier::Control
    }
}

pub struct FocusableArea<'a, Message, Renderer = icy_ui::Renderer> {
    id: Option<Id>,
    content: Element<'a, Message, icy_ui::Theme, Renderer>,
    on_focus: Option<Message>,
    on_blur: Option<Message>,
    on_key: Option<Box<dyn Fn(keyboard::Key, keyboard::Modifiers) -> Message + 'a>>,
}

impl<'a, Message, Renderer> FocusableArea<'a, Message, Renderer>
where
    Renderer: renderer::Renderer + 'a,
{
    pub fn new(content: impl Into<Element<'a, Message, icy_ui::Theme, Renderer>>) -> Self {
        Self {
            id: None,
            content: content.into(),
            on_focus: None,
            on_blur: None,
            on_key: None,
        }
    }

    pub fn id(mut self, id: Id) -> Self {
        self.id = Some(id);
        self
    }

    pub fn on_focus(mut self, message: Message) -> Self {
        self.on_focus = Some(message);
        self
    }

    pub fn on_blur(mut self, message: Message) -> Self {
        self.on_blur = Some(message);
        self
    }

    pub fn on_key(mut self, f: impl Fn(keyboard::Key, keyboard::Modifiers) -> Message + 'a) -> Self {
        self.on_key = Some(Box::new(f));
        self
    }
}

impl<Message, Renderer> Widget<Message, icy_ui::Theme, Renderer> for FocusableArea<'_, Message, Renderer>
where
    Message: Clone,
    Renderer: renderer::Renderer,
{
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
        tree.diff_children(&[&self.content]);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &icy_ui::advanced::layout::Limits) -> icy_ui::advanced::layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
            let state = tree.state.downcast_ref::<State>();
            if state.is_focused {
                if let Some(on_key) = &self.on_key {
                    shell.publish(on_key(key.clone(), *modifiers));
                    shell.capture_event();
                }
                return;
            }
        }

        self.content
            .as_widget_mut()
            .update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);

        let press = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed {
                button: mouse::Button::Left,
                ..
            }) | Event::Touch(icy_ui::touch::Event::FingerPressed { .. })
        );

        if press {
            let state = tree.state.downcast_mut::<State>();
            let over = cursor.is_over(layout.bounds());

            if over {
                if !state.is_focused {
                    state.is_focused = true;
                    if let Some(message) = &self.on_focus {
                        shell.publish(message.clone());
                    }
                    shell.capture_event();
                    shell.request_redraw();
                }
            } else if state.is_focused {
                state.is_focused = false;
                if let Some(message) = &self.on_blur {
                    shell.publish(message.clone());
                }
                shell.request_redraw();
            }
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &icy_ui::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget()
            .draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);

        let state = tree.state.downcast_ref::<State>();
        if state.is_focused {
            FocusRing::from_theme(theme).width(3.0).offset(-3.0).radius(3.0).draw(renderer, layout.bounds());
        }
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        let state = tree.state.downcast_mut::<State>();
        operation.focusable(self.id.as_ref(), layout.bounds(), state);

        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, icy_ui::Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

impl<'a, Message, Renderer> From<FocusableArea<'a, Message, Renderer>> for Element<'a, Message, icy_ui::Theme, Renderer>
where
    Message: Clone + 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(area: FocusableArea<'a, Message, Renderer>) -> Self {
        Element::new(area)
    }
}
