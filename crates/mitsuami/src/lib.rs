//! mitsuami: native widgets on macOS (AppKit), Windows (WinUI 3) and Linux
//! (GTK 4), driven by one declarative, Vue-inspired layer.
//!
//! ```ignore
//! use mitsuami::prelude::*;
//!
//! fn Counter(initial: i32) -> impl View {
//!     let count = signal(initial);
//!     Column::new().gap(Spacing::Md).padding(2.em()).children((
//!         Text::new(move || format!("Count: {}", count.get())).text_style(TextStyle::Title),
//!         Button::new("Increment").on_click(move || count.update(|c| *c += 1)),
//!     ))
//! }
//! ```
//!
//! The platform backend is chosen by target OS. Backends arrive in M1–M3;
//! until then, use `mitsuami-test` to run views headlessly.

pub use mitsuami_core as core;
pub use mitsuami_reactive as reactive;
pub use mitsuami_widgets as widgets;

pub mod prelude {
    pub use mitsuami_core::{
        Align, ButtonVariant, Children, Element, ElementBuilder, FlexDirection, For, GridPlacement, Justify, Length,
        LengthExt, NodeId, Role, Show, Spacing, TextDirection, TextStyle, Track, Ui, View, repeat,
    };
    pub use mitsuami_reactive::{
        Computed, IntoValue, Owner, Signal, Value, batch, computed, effect, inject, on_cleanup, provide, signal,
        untrack, watch,
    };
    pub use mitsuami_widgets::{Button, Checkbox, Column, Container, Grid, Row, Switch, Text, TextInput};
}
