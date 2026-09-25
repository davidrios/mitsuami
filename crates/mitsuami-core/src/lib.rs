//! mitsuami core: the retained node tree, styles and units, layout,
//! accessibility model, control flow and the backend contract.
//!
//! See `docs/ARCHITECTURE.md` for the design.

pub mod a11y;
pub mod backend;
pub mod command;
mod element;
mod flow;
pub mod geometry;
pub mod services;
pub mod style;
pub mod task;
mod ui;
pub mod units;
mod view;
mod widget;

pub use a11y::{A11yAction, A11yNode, A11yProps, ActionError, Role};
pub use backend::{Backend, EventSink, Key, NativeState, PlatformMetrics, SyntheticInput};
pub use command::{Command, EventValue, UiEvent};
pub use element::{Element, ElementBuilder};
pub use flow::{For, Show};
pub use geometry::{Point, Rect, Size};
pub use style::{Align, Display, FlexDirection, GridPlacement, Justify, Style, TextDirection, Track, repeat};
pub use ui::{NodeInfo, Ui, WeakUi};
pub use units::{Length, LengthExt, Spacing};
pub use view::{AnyView, Children, View};
pub use widget::{ButtonVariant, NodeId, Prop, ScrollAxes, TextStyle, WidgetKind};

pub use mitsuami_reactive as reactive;
