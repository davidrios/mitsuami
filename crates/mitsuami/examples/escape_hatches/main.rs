//! The escape hatches: `cargo run -p mitsuami --example escape_hatches`.
//!
//! One screen, the same on every platform, with three custom widgets, each
//! native on one platform and stood in for on the others:
//!
//! - `Lock` is native on Linux (`GtkLockButton`), composed from a built-in
//!   button elsewhere. It guards the rating and Submit.
//! - `Rating` is native on macOS (`NSLevelIndicator`), built ad hoc from
//!   star buttons on GTK (as GNOME Software does), drawn elsewhere.
//! - `PipsPager` is WinUI's (native once the WinUI backend exists), drawn
//!   elsewhere.
//!
//! `platform!` picks each widget's render; the labels show "(native)" only
//! where it's the platform's own control. The store (`store.rs`) is shared.

mod lock;
mod pips_pager;
mod rating;
mod screen;
mod store;

use mitsuami::prelude::*;

fn main() {
    App::new()
        .window("Escape hatches", Size::new(520.0, 360.0), || {
            provide(store::Review::new());
            Column::new().padding(Spacing::Xl).child(screen::screen())
        })
        .run();
}
