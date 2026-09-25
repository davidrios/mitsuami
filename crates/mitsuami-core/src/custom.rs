//! Custom widgets: one platform-free definition, rendered natively on each
//! platform or drawn with the [`Canvas`] API.
//!
//! - [`CustomWidget`]: the shared part. Props, events, semantics, and what
//!   accessibility actions mean.
//! - [`Drawn`]: the drawn tier. Measure, draw and hit-test in shared code;
//!   runs everywhere, and stands in for the widget in headless tests.
//! - A native render per platform, e.g. `mitsuami_appkit::NativeRender`.
//! - [`Render`]: says which render each platform uses. Using a widget
//!   requires it, so a platform with no render doesn't compile.

use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

use mitsuami_reactive::{IntoValue, Value};

use crate::a11y::{A11yAction, A11yProps};
use crate::any_value::{AnyValue, Opaque};
use crate::backend::{MeasureRequest, PlatformMetrics};
use crate::command::{PointerEvent, UiEvent};
use crate::draw::{Canvas, DisplayList};
use crate::element::{Element, ElementBuilder};
use crate::geometry::Size;
use crate::ui::Ui;
use crate::view::View;
use crate::widget::{NodeId, Prop, WidgetKind};

/// The shared, platform-free definition of a custom widget.
pub trait CustomWidget: Sized + 'static {
    /// Shown in trees, logs and snapshots.
    const NAME: &'static str;
    type Props: Clone + PartialEq + fmt::Debug + 'static;
    type Event: Clone + PartialEq + fmt::Debug + 'static;

    /// Semantics, shared by every render. Apps can still override the
    /// label and description where they use the widget.
    fn a11y(props: &Self::Props) -> A11yProps;

    /// What an accessibility action (increment, set value, …) means for
    /// this widget. Used unless the native render performs it itself.
    fn action(props: &Self::Props, action: &A11yAction) -> Option<Self::Event> {
        let _ = (props, action);
        None
    }
}

/// The drawn tier: measured, drawn and hit-tested in shared code.
pub trait Drawn: CustomWidget {
    /// Intrinsic size, like a backend measures a native control.
    fn measure(props: &Self::Props, request: &MeasureRequest, metrics: &PlatformMetrics) -> Size;

    /// Draws into `canvas`, sized to the widget's frame. Called again
    /// whenever the props, the size or the platform metrics change.
    fn draw(props: &Self::Props, canvas: &mut Canvas);

    /// What a pointer event at `event.position` (widget coordinates) means.
    fn pointer(props: &Self::Props, size: Size, event: &PointerEvent) -> Option<Self::Event> {
        let _ = (props, size, event);
        None
    }
}

/// Which render this platform uses. Implement it once, choosing per
/// platform with `platform!`:
///
/// ```ignore
/// impl Render for Rating {
///     fn renderer() -> Renderer<Self> {
///         platform! {
///             macos => mitsuami_appkit::native::<Self>().with_drawn(),
///             _ => Renderer::drawn(),
///         }
///     }
/// }
/// ```
pub trait Render: CustomWidget {
    fn renderer() -> Renderer<Self>;
}

/// A widget's renders on the current platform: a native one, a drawn one,
/// or both (native, with the drawn one for headless tests and `.drawn()`).
pub struct Renderer<W> {
    native: Option<Opaque>,
    drawn: Option<DrawnFns>,
    _widget: PhantomData<fn() -> W>,
}

impl<W: CustomWidget> Renderer<W> {
    /// Draws the widget with its [`Drawn`] implementation.
    pub fn drawn() -> Renderer<W>
    where
        W: Drawn,
    {
        Renderer { native: None, drawn: Some(DrawnFns::of::<W>()), _widget: PhantomData }
    }

    /// For backend crates: a native render, in whatever form the backend
    /// understands. Apps use the backend's constructor instead.
    pub fn native(render: Opaque) -> Renderer<W> {
        Renderer { native: Some(render), drawn: None, _widget: PhantomData }
    }

    /// Keeps the drawn render too: headless tests lay the widget out with
    /// it, and `Custom::drawn` can pick it.
    pub fn with_drawn(mut self) -> Renderer<W>
    where
        W: Drawn,
    {
        self.drawn = Some(DrawnFns::of::<W>());
        self
    }
}

/// The drawn tier, type-erased.
#[derive(Clone, Copy)]
struct DrawnFns {
    measure: fn(&AnyValue, &MeasureRequest, &PlatformMetrics) -> Size,
    draw: fn(&AnyValue, &mut Canvas),
    pointer: fn(&AnyValue, Size, &PointerEvent) -> Option<AnyValue>,
}

impl DrawnFns {
    fn of<W: Drawn>() -> DrawnFns {
        DrawnFns {
            measure: |props, request, metrics| W::measure(props_of::<W>(props), request, metrics),
            draw: |props, canvas| W::draw(props_of::<W>(props), canvas),
            pointer: |props, size, event| W::pointer(props_of::<W>(props), size, event).map(AnyValue::new),
        }
    }
}

fn props_of<W: CustomWidget>(props: &AnyValue) -> &W::Props {
    props.downcast_ref().unwrap_or_else(|| panic!("{}: props of another type: {props:?}", W::NAME))
}

/// Everything about one custom widget instance that is fixed at build time.
struct Definition {
    name: &'static str,
    a11y: fn(&AnyValue) -> A11yProps,
    action: fn(&AnyValue, &A11yAction) -> Option<AnyValue>,
    native: Option<Opaque>,
    drawn: Option<DrawnFns>,
    /// Draw it even where a native render exists.
    force_drawn: bool,
}

/// The props of a custom widget, as carried by [`Prop::Custom`]: the value
/// plus the widget's type-erased definition (semantics and renders).
#[derive(Clone)]
pub struct CustomProps {
    props: AnyValue,
    definition: Rc<Definition>,
}

impl CustomProps {
    pub fn name(&self) -> &'static str {
        self.definition.name
    }

    pub fn props(&self) -> &AnyValue {
        &self.props
    }

    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.props.downcast_ref()
    }

    /// The same widget with other props, e.g. read back from a native view.
    pub fn with_props(&self, props: AnyValue) -> CustomProps {
        CustomProps { props, definition: self.definition.clone() }
    }

    /// Whether the widget is drawn (by the core's display list) rather than
    /// rendered natively.
    pub fn is_drawn(&self) -> bool {
        self.definition.drawn.is_some() && (self.definition.force_drawn || self.definition.native.is_none())
    }

    /// The native render, unless the widget is drawn.
    pub fn native(&self) -> Option<&Opaque> {
        if self.is_drawn() { None } else { self.definition.native.as_ref() }
    }

    pub fn a11y(&self) -> A11yProps {
        (self.definition.a11y)(&self.props)
    }

    /// The event an accessibility action stands for, if any.
    pub fn action(&self, action: &A11yAction) -> Option<AnyValue> {
        (self.definition.action)(&self.props, action)
    }

    /// Size from the drawn render, if the widget has one.
    pub fn measure_drawn(&self, request: &MeasureRequest, metrics: &PlatformMetrics) -> Option<Size> {
        self.definition.drawn.map(|d| (d.measure)(&self.props, request, metrics))
    }

    pub fn draw(&self, size: Size, metrics: &PlatformMetrics) -> Option<DisplayList> {
        let drawn = self.definition.drawn?;
        let mut canvas = Canvas::new(size, metrics);
        (drawn.draw)(&self.props, &mut canvas);
        Some(canvas.finish())
    }

    pub fn pointer(&self, size: Size, event: &PointerEvent) -> Option<AnyValue> {
        self.definition.drawn.and_then(|d| (d.pointer)(&self.props, size, event))
    }
}

impl PartialEq for CustomProps {
    fn eq(&self, other: &CustomProps) -> bool {
        self.props == other.props && self.name() == other.name() && self.is_drawn() == other.is_drawn()
    }
}

impl fmt::Debug for CustomProps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.props)?;
        if self.is_drawn() {
            f.write_str(" (drawn)")?;
        }
        Ok(())
    }
}

/// A custom widget in the view tree: `Custom::<Rating>::new(props)`, or
/// `Rating::view(props)` through [`CustomView`].
pub struct Custom<W: CustomWidget> {
    element: Element,
    props: Value<W::Props>,
    renderer: Renderer<W>,
    force_drawn: bool,
}

impl<W: Render> Custom<W> {
    /// Props are usually reactive: `move || RatingProps { value: rating.get(), .. }`.
    pub fn new(props: impl IntoValue<W::Props>) -> Custom<W> {
        Custom {
            element: Element::new(WidgetKind::Custom(W::NAME)),
            props: props.into_value(),
            renderer: W::renderer(),
            force_drawn: false,
        }
    }
}

impl<W: CustomWidget> Custom<W> {
    /// Uses the drawn render even where a native one exists.
    pub fn drawn(mut self) -> Custom<W>
    where
        W: Drawn,
    {
        self.renderer.drawn = Some(DrawnFns::of::<W>());
        self.force_drawn = true;
        self
    }

    /// Called with each event the widget emits.
    pub fn on_event(mut self, handler: impl Fn(&W::Event) + 'static) -> Custom<W> {
        self.element.on(move |event| {
            if let UiEvent::Custom(value) = event
                && let Some(event) = value.downcast_ref::<W::Event>()
            {
                handler(event);
            }
        });
        self
    }
}

impl<W: CustomWidget> ElementBuilder for Custom<W> {
    fn element(&mut self) -> &mut Element {
        &mut self.element
    }
}

impl<W: CustomWidget> View for Custom<W> {
    fn build(self, ui: &Ui) -> NodeId {
        let Custom { mut element, props, renderer, force_drawn } = self;
        let definition = Rc::new(Definition {
            name: W::NAME,
            a11y: |props| W::a11y(props_of::<W>(props)),
            action: |props, action| W::action(props_of::<W>(props), action).map(AnyValue::new),
            native: renderer.native,
            drawn: renderer.drawn,
            force_drawn,
        });
        element.prop(props, move |props| {
            Prop::Custom(CustomProps { props: AnyValue::new(props), definition: definition.clone() })
        });
        element.build(ui)
    }
}

/// `Rating::view(props)`: shorthand for `Custom::<Rating>::new(props)`.
pub trait CustomView: Render {
    fn view(props: impl IntoValue<Self::Props>) -> Custom<Self> {
        Custom::new(props)
    }
}

impl<W: Render> CustomView for W {}
