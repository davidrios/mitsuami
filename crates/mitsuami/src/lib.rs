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
//!
//! Escape hatches, for when the shared widgets aren't enough:
//! - [`platform!`] picks per-platform code (a whole screen, a detail) at
//!   compile time, while stores and composables stay shared.
//! - `NativeView` embeds any native view in the shared tree
//!   ([`appkit::NativeView`] on macOS).
//! - Custom widgets: one [`CustomWidget`](core::CustomWidget) definition,
//!   rendered natively per platform or drawn with the
//!   [`Canvas`](core::Canvas) API.

mod app;
mod platforms;

pub use app::App;
pub use mitsuami_core as core;
pub use mitsuami_reactive as reactive;
pub use mitsuami_widgets as widgets;

/// The AppKit backend: native renders, native views, and the `objc2`
/// bindings to write them with.
#[cfg(target_os = "macos")]
pub use mitsuami_appkit as appkit;

pub mod prelude {
    pub use crate::{App, platform};
    pub use mitsuami_core::draw::DisplayList;
    pub use mitsuami_core::services::{
        Alert, AlertStyle, FileFilter, Menu, MenuBar, MenuItem, OpenFile, SaveFile, ServiceError, Shortcut, alert,
        clipboard_text, open_file, save_file, set_clipboard_text, set_menu,
    };
    pub use mitsuami_core::task::{TaskHandle, sleep, spawn_blocking, spawn_local};
    pub use mitsuami_core::{
        A11yAction, A11yProps, Canvas, Color, Custom, CustomView, CustomWidget, Drawn, MeasureRequest, Path,
        PlatformMetrics, PointerEvent, PointerKind, Rect, Render, Renderer, Shape,
    };
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
