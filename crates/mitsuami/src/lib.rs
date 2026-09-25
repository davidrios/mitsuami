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
//! The platform backend is chosen by target OS: AppKit on macOS today, GTK
//! and WinUI 3 in M2/M3.

mod app;

pub use app::App;
pub use mitsuami_core as core;
pub use mitsuami_reactive as reactive;
pub use mitsuami_widgets as widgets;

pub mod prelude {
    pub use crate::App;
    pub use mitsuami_core::services::{
        Alert, AlertStyle, FileFilter, Menu, MenuBar, MenuItem, OpenFile, SaveFile, Shortcut, alert, clipboard_text,
        open_file, save_file, set_clipboard_text, set_menu,
    };
    pub use mitsuami_core::task::{TaskHandle, sleep, spawn_blocking, spawn_local};
    pub use mitsuami_core::{
        Align, ButtonVariant, Children, Element, ElementBuilder, FlexDirection, For, GridPlacement, Justify, Length,
        LengthExt, NodeId, Point, Role, Show, Size, Spacing, TextDirection, TextStyle, Track, Ui, View, repeat,
    };
    pub use mitsuami_reactive::{
        Computed, IntoValue, Owner, Signal, Value, batch, computed, effect, inject, on_cleanup, provide, signal,
        untrack, watch,
    };
    pub use mitsuami_widgets::{Button, Checkbox, Column, Container, Grid, Row, ScrollView, Switch, Text, TextInput};
}
