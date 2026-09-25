//! The generic builder every widget is made of.

use std::rc::Rc;

use mitsuami_reactive::{Value, effect};

use crate::a11y::{A11yProps, Role};
use crate::command::UiEvent;
use crate::style::{Align, Display, Edges, GridPlacement, Position, Style, TextDirection};
use crate::ui::{Handler, Ui};
use crate::units::Length;
use crate::view::{AnyView, Children, View};
use crate::widget::{NodeId, Prop, WidgetKind};

type Binder = Box<dyn FnOnce(&Ui, NodeId)>;

/// A node description: kind, style, props (static or reactive), handlers,
/// semantics and children. Widgets wrap an `Element` and add typed methods.
pub struct Element {
    pub kind: WidgetKind,
    pub style: Style,
    pub a11y: A11yProps,
    pub test_id: Option<String>,
    pub tab_index: Option<u32>,
    static_props: Vec<Prop>,
    binders: Vec<Binder>,
    handlers: Vec<Handler>,
    children: Vec<AnyView>,
}

impl Element {
    pub fn new(kind: WidgetKind) -> Element {
        Element {
            kind,
            style: Style::default(),
            a11y: A11yProps::default(),
            test_id: None,
            tab_index: None,
            static_props: Vec::new(),
            binders: Vec::new(),
            handlers: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Sets a prop from a static or reactive value. Reactive values get an
    /// effect that updates the native widget whenever they change.
    pub fn prop<T: Clone + 'static>(&mut self, value: Value<T>, to_prop: impl Fn(T) -> Prop + 'static) {
        match value {
            Value::Static(v) => {
                let prop = to_prop(v);
                self.static_props.retain(|p| p.key() != prop.key());
                self.static_props.push(prop);
            }
            Value::Dynamic(f) => self.binders.push(Box::new(move |ui, id| {
                let ui = ui.clone();
                effect(move || ui.set_prop(id, to_prop(f())));
            })),
        }
    }

    pub fn on(&mut self, handler: impl Fn(&UiEvent) + 'static) {
        self.handlers.push(Rc::new(handler));
    }

    /// Runs `f` right after the node is created, in the building scope.
    pub fn after_build(&mut self, f: impl FnOnce(&Ui, NodeId) + 'static) {
        self.binders.push(Box::new(f));
    }

    pub fn add_children(&mut self, children: impl Children) {
        children.into_views(&mut self.children);
    }
}

impl View for Element {
    fn build(self, ui: &Ui) -> NodeId {
        let id = ui.create(self.kind, self.static_props);
        ui.set_style(id, self.style);
        if !self.a11y.is_empty() {
            ui.set_a11y(id, self.a11y);
        }
        if let Some(test_id) = self.test_id {
            ui.set_test_id(id, test_id);
        }
        if self.tab_index.is_some() {
            ui.set_tab_index(id, self.tab_index);
        }
        for handler in self.handlers {
            ui.on_event(id, move |e| handler(e));
        }
        for binder in self.binders {
            binder(ui, id);
        }
        for child in self.children {
            let child = child.build(ui);
            ui.append_child(id, child);
        }
        id
    }
}

impl From<i32> for Length {
    fn from(v: i32) -> Length {
        Length::Px(v as f32)
    }
}

impl From<f32> for Length {
    fn from(v: f32) -> Length {
        Length::Px(v)
    }
}

/// Style, semantics and test hooks shared by every widget builder.
pub trait ElementBuilder: Sized {
    fn element(&mut self) -> &mut Element;

    /// Escape hatch: edit the style directly.
    fn style(mut self, f: impl FnOnce(&mut Style)) -> Self {
        f(&mut self.element().style);
        self
    }

    fn width(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.width = v.into())
    }
    fn height(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.height = v.into())
    }
    fn size(self, width: impl Into<Length>, height: impl Into<Length>) -> Self {
        self.style(|s| {
            s.width = width.into();
            s.height = height.into();
        })
    }
    fn min_width(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.min_width = v.into())
    }
    fn min_height(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.min_height = v.into())
    }
    fn max_width(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.max_width = v.into())
    }
    fn max_height(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.max_height = v.into())
    }
    fn aspect_ratio(self, ratio: f32) -> Self {
        self.style(|s| s.aspect_ratio = Some(ratio))
    }

    fn padding(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.padding = Edges::all(v.into()))
    }
    fn padding_x(self, v: impl Into<Length>) -> Self {
        let v = v.into();
        self.style(|s| {
            s.padding.start = v;
            s.padding.end = v;
        })
    }
    fn padding_y(self, v: impl Into<Length>) -> Self {
        let v = v.into();
        self.style(|s| {
            s.padding.top = v;
            s.padding.bottom = v;
        })
    }
    fn padding_start(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.padding.start = v.into())
    }
    fn padding_end(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.padding.end = v.into())
    }
    fn padding_top(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.padding.top = v.into())
    }
    fn padding_bottom(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.padding.bottom = v.into())
    }

    fn margin(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.margin = Edges::all(v.into()))
    }
    fn margin_x(self, v: impl Into<Length>) -> Self {
        let v = v.into();
        self.style(|s| {
            s.margin.start = v;
            s.margin.end = v;
        })
    }
    fn margin_y(self, v: impl Into<Length>) -> Self {
        let v = v.into();
        self.style(|s| {
            s.margin.top = v;
            s.margin.bottom = v;
        })
    }
    fn margin_start(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.margin.start = v.into())
    }
    fn margin_end(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.margin.end = v.into())
    }
    fn margin_top(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.margin.top = v.into())
    }
    fn margin_bottom(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.margin.bottom = v.into())
    }

    fn grow(self, factor: f32) -> Self {
        self.style(|s| s.flex_grow = factor)
    }
    fn shrink(self, factor: f32) -> Self {
        self.style(|s| s.flex_shrink = factor)
    }
    fn basis(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.flex_basis = v.into())
    }
    fn align_self(self, align: Align) -> Self {
        self.style(|s| s.align_self = Some(align))
    }

    /// Takes the node out of flow; position it with `top`/`start`/….
    fn absolute(self) -> Self {
        self.style(|s| s.position = Position::Absolute)
    }
    fn top(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.inset.top = v.into())
    }
    fn bottom(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.inset.bottom = v.into())
    }
    fn start(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.inset.start = v.into())
    }
    fn end(self, v: impl Into<Length>) -> Self {
        self.style(|s| s.inset.end = v.into())
    }

    fn grid_column(self, placement: GridPlacement) -> Self {
        self.style(|s| s.grid_column = placement)
    }
    fn grid_row(self, placement: GridPlacement) -> Self {
        self.style(|s| s.grid_row = placement)
    }

    fn direction(self, direction: TextDirection) -> Self {
        self.style(|s| s.direction = direction)
    }
    /// `display: none`: not laid out, not visible, not in the a11y tree.
    fn hidden(self, hidden: bool) -> Self {
        self.style(|s| s.display = if hidden { Display::None } else { Display::Flex })
    }

    fn a11y_label(mut self, label: impl Into<String>) -> Self {
        self.element().a11y.label = Some(label.into());
        self
    }
    fn a11y_description(mut self, description: impl Into<String>) -> Self {
        self.element().a11y.description = Some(description.into());
        self
    }
    fn a11y_role(mut self, role: Role) -> Self {
        self.element().a11y.role = Some(role);
        self
    }
    fn a11y_hidden(mut self) -> Self {
        self.element().a11y.hidden = true;
        self
    }
    /// Moves this control ahead in the Tab order. Controls with a tab index
    /// come first (lowest first); the rest follow in reading order. Prefer
    /// arranging the tree in the order users should visit it.
    fn tab_index(mut self, index: u32) -> Self {
        self.element().tab_index = Some(index);
        self
    }

    /// Last-resort handle for tests; prefer finding nodes by role and name.
    fn test_id(mut self, id: impl Into<String>) -> Self {
        self.element().test_id = Some(id.into());
        self
    }
}

impl ElementBuilder for Element {
    fn element(&mut self) -> &mut Element {
        self
    }
}
