//! AppKit (macOS) backend for mitsuami.
//!
//! - Containers are flipped `NSView`s that do no layout of their own; the
//!   core positions every child with `setFrame:`.
//! - Leaf widgets are stock AppKit controls, measured through their cells.
//! - Control actions and delegate callbacks become [`UiEvent`](mitsuami_core::UiEvent)s.
//! - [`run`] drives the app: a run-loop observer calls [`Ui::tick`](mitsuami_core::Ui::tick)
//!   before the loop sleeps, in all common modes (so live resize relayouts).

#[cfg(target_os = "macos")]
mod app;
#[cfg(target_os = "macos")]
mod backend;
#[cfg(target_os = "macos")]
mod classes;

#[cfg(target_os = "macos")]
pub use app::{init_for_tests, run};
#[cfg(target_os = "macos")]
pub use backend::{AppKitBackend, AppKitHandle, BackendOptions};
