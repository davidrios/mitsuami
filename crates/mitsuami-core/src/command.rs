//! The data flowing between the core and a backend.

use crate::a11y::A11yProps;
use crate::geometry::{Rect, Size};
use crate::widget::{NodeId, Prop, WidgetKind};

/// A change the backend must apply to the native widget tree.
///
/// Commands arrive in batches. Within a batch, order matters: a node is
/// always created before it is inserted, and a subtree root is removed before
/// it is destroyed.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Native views start with a zero frame; the core only sends frames
    /// that differ from the last one it sent.
    Create {
        id: NodeId,
        kind: WidgetKind,
        props: Vec<Prop>,
    },
    SetProp {
        id: NodeId,
        prop: Prop,
    },
    Insert {
        parent: NodeId,
        child: NodeId,
        index: usize,
    },
    Remove {
        parent: NodeId,
        child: NodeId,
    },
    /// Frees a node. Sent for every native node of a destroyed subtree,
    /// children first. The subtree's root has already been removed from its
    /// parent; nodes inside it may still be attached to each other.
    Destroy {
        id: NodeId,
    },
    /// Parent-relative, logical units. Never sent for windows.
    SetFrame {
        id: NodeId,
        frame: Rect,
    },
    SetA11y {
        id: NodeId,
        a11y: A11yProps,
    },
    /// Window content size requested by the app. The platform may refuse it.
    SetWindowSize {
        id: NodeId,
        size: Size,
    },
    Focus {
        id: NodeId,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum EventValue {
    Text(String),
    Bool(bool),
    Number(f64),
}

/// Something that happened in the native UI.
#[derive(Clone, Debug, PartialEq)]
pub enum UiEvent {
    Click,
    /// The user changed a control's value. The native widget already shows it.
    Changed(EventValue),
    Submit,
    FocusIn,
    FocusOut,
    WindowResized(Size),
    WindowCloseRequested,
    /// Platform metrics changed (text size, color scheme, …).
    MetricsChanged,
}
