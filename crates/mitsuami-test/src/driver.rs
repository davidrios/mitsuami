//! What a test needs from a backend beyond the `Backend` contract.

use mitsuami_core::{Command, NodeId, Size, TestHooks, Ui};
use mitsuami_headless::{HeadlessBackend, HeadlessHandle};

/// Which backend runs the test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Headless,
    Native,
}

pub(crate) struct Driver {
    hooks: Box<dyn TestHooks>,
    headless: Option<HeadlessHandle>,
}

impl Driver {
    pub(crate) fn create(mode: Mode) -> (Ui, Driver) {
        match mode {
            Mode::Headless => {
                let backend = HeadlessBackend::new();
                let handle = backend.handle();
                let driver = Driver { hooks: Box::new(handle.clone()), headless: Some(handle) };
                (Ui::new(backend), driver)
            }
            Mode::Native => {
                let (ui, hooks) = native();
                (ui, Driver { hooks, headless: None })
            }
        }
    }

    pub(crate) fn headless(&self) -> Option<&HeadlessHandle> {
        self.headless.as_ref()
    }

    /// Short name used in snapshot and baseline paths.
    pub(crate) fn name(&self) -> &'static str {
        self.hooks.name()
    }

    pub(crate) fn resize_window(&self, window: NodeId, size: Size) {
        self.hooks.resize_window(window, size);
    }

    pub(crate) fn take_command_log(&self) -> Vec<Command> {
        self.hooks.take_command_log()
    }

    pub(crate) fn node_count(&self) -> usize {
        self.hooks.node_count()
    }
}

fn show_windows() -> bool {
    std::env::var("MITSUAMI_SHOW_WINDOWS").is_ok_and(|v| v == "1")
}

// One `native()` per platform: create the backend configured for tests
// (offscreen unless MITSUAMI_SHOW_WINDOWS=1, recording commands, a fixed
// appearance, a private clipboard) and return its test hooks.

#[cfg(target_os = "macos")]
fn native() -> (Ui, Box<dyn TestHooks>) {
    use mitsuami_appkit::{AppKitBackend, BackendOptions};
    let mtm = mitsuami_appkit::init_for_tests();
    let backend = AppKitBackend::new(
        mtm,
        BackendOptions {
            show_windows: show_windows(),
            record_commands: true,
            force_light_appearance: true,
            private_clipboard: true,
        },
    );
    let hooks = backend.handle();
    (Ui::new(backend), Box::new(hooks))
}

#[cfg(not(target_os = "macos"))]
fn native() -> (Ui, Box<dyn TestHooks>) {
    let _ = show_windows;
    panic!("mitsuami-test: no native backend for this platform yet (GTK and WinUI arrive in M2/M3)")
}

/// Whether this platform has a native backend to run `--native` tests on.
pub(crate) fn native_available() -> bool {
    cfg!(target_os = "macos")
}

/// Runs `f` inside an autorelease pool where the platform needs one, so
/// native objects created by a test are released when it ends.
pub(crate) fn with_pool<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(target_os = "macos")]
    {
        objc2::rc::autoreleasepool(|_| f())
    }
    #[cfg(not(target_os = "macos"))]
    {
        f()
    }
}
