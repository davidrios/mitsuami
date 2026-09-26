//! The escape hatches: `cargo run -p mitsuami --example escape_hatches`.
//!
//! One screen, the same on every platform, with three custom widgets, each
//! native where the platform has the control and stood in for elsewhere:
//!
//! - `Lock` is native on Linux (`GtkLockButton`), composed from a built-in
//!   button elsewhere. It guards the rating and Submit.
//! - `Rating` is native on macOS (`NSLevelIndicator`) and on Windows
//!   (`RatingControl`), built ad hoc from star buttons on GTK (as GNOME
//!   Software does), drawn elsewhere.
//! - `PipsPager` is native on Windows (WinUI's `PipsPager`), drawn elsewhere.
//!
//! `platform!` picks each widget's render; the labels show "(native)" only
//! where it's the platform's own control. The store (`store.rs`) is shared.
//!
//! Written with `view!` and `#[component]`; the store is a `Store`, the
//! app's one instance.

mod lock;
mod pips_pager;
mod rating;
mod screen;
mod store;

use mitsuami::prelude::*;

fn main() {
    App::new()
        .window("Escape hatches", WindowSize::FitHeight(520.0), || {
            view! {
                <Column padding=Spacing::Xl>
                    <screen::Screen/>
                </Column>
            }
        })
        .run();
}
