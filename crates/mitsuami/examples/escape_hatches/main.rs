//! The escape hatches: `cargo run -p mitsuami --example escape_hatches`.
//!
//! - `platform!` picks a macOS-specific review screen; other platforms get
//!   the shared one. Both use the same store (`store.rs`).
//! - The macOS screen embeds a raw `NSStepper` with `NativeView`.
//! - `Rating` is a custom widget with a native render (macOS), a drawn
//!   render and a composed one, shown side by side.

mod rating;
mod review;
mod store;

use mitsuami::prelude::*;

fn main() {
    App::new()
        .window("Escape hatches", Size::new(480.0, 400.0), || {
            provide(store::Review::new());
            Column::new().padding(Spacing::Xl).gap(Spacing::Xl).children((review::review_screen(), review::renders()))
        })
        .run();
}
