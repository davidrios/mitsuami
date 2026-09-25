//! Testing for mitsuami apps: one API for headless integration tests and
//! (from M1) end-to-end tests on the native backends.
//!
//! ```ignore
//! use mitsuami_test::prelude::*;
//!
//! #[mitsuami_test::test]
//! async fn increments(app: TestApp) {
//!     app.mount(|| Counter(0));
//!     app.get_by_role(Role::Button, "Increment").click().await;
//!     app.expect(by_text("Count: 1")).to_be_visible().await;
//! }
//!
//! mitsuami_test::main!();
//! ```
//!
//! Test targets use `harness = false`: native UI must own the main thread,
//! so mitsuami runs tests itself. Controls are found through the
//! accessibility tree, and driven with accessibility actions or synthesized
//! input, the way users and assistive technology reach them.

mod app;
mod driver;
mod exec;
mod format;
mod locator;
mod query;
mod runner;
mod snapshot;
mod visual;

pub use app::TestApp;
pub use driver::Mode;
pub use locator::{Expectation, Locator};
pub use mitsuami_test_macros::test;
pub use query::{Query, by_label, by_role, by_test_id, by_text};

pub mod prelude {
    pub use crate::{Expectation, Locator, Query, TestApp, by_label, by_role, by_test_id, by_text};
    pub use mitsuami_core::{Key, Rect, Role, Size};
}

/// Defines `main` for a `harness = false` test target.
#[macro_export]
macro_rules! main {
    () => {
        fn main() {
            $crate::__private::run_main()
        }
    };
}

#[doc(hidden)]
pub mod __private {
    use std::future::Future;
    use std::pin::Pin;

    pub use inventory;

    pub use crate::runner::run_main;

    pub type TestFuture = Pin<Box<dyn Future<Output = ()>>>;

    pub struct TestCase {
        pub name: &'static str,
        pub manifest_dir: &'static str,
        /// Depends on the headless backend (fake metrics, simulated
        /// system changes): skipped with `--native`.
        pub headless_only: bool,
        pub run: fn(crate::TestApp) -> TestFuture,
    }

    inventory::collect!(TestCase);
}
