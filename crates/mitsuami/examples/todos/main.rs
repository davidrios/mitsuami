//! The showcase for components and stores:
//! `cargo run -p mitsuami --example todos`.
//!
//! A todo list written with `view!` and `#[component]`:
//!
//! - `Todos` (`store.rs`) is a `Store`: the app's one instance, which every
//!   component takes with `use_store`.
//! - The screen (`screen.rs`) is made of components with required,
//!   optional, reactive, callback and children props, and uses `Show` and
//!   keyed `For`.
//! - The quote loads with a `resource` (loading, error, refetch), and the
//!   sync button runs an `action` (pending, latest value).
//!
//! `showcase` is the same kind of tour written with the builder API.

mod screen;
mod store;

use mitsuami::prelude::*;

fn main() {
    App::new()
        .window("mitsuami todos", WindowSize::FitHeight(400.0), || {
            view! {
                <Column padding=Spacing::Xl>
                    <screen::Screen/>
                </Column>
            }
        })
        .run();
}
