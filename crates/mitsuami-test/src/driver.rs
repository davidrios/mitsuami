//! What a test needs from a backend beyond the `Backend` contract.

use mitsuami_core::{Command, NodeId, Size, Ui};
use mitsuami_headless::{HeadlessBackend, HeadlessHandle};

/// Which backend runs the test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Headless,
    Native,
}

pub(crate) enum Driver {
    Headless(HeadlessHandle),
    #[cfg(target_os = "macos")]
    AppKit(mitsuami_appkit::AppKitHandle),
}

impl Driver {
    pub(crate) fn create(mode: Mode) -> (Ui, Driver) {
        match mode {
            Mode::Headless => {
                let backend = HeadlessBackend::new();
                let handle = backend.handle();
                (Ui::new(backend), Driver::Headless(handle))
            }
            Mode::Native => native(),
        }
    }

    /// Short name used in snapshot and baseline paths.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Driver::Headless(_) => "headless",
            #[cfg(target_os = "macos")]
            Driver::AppKit(_) => "appkit",
        }
    }

    pub(crate) fn resize_window(&self, window: NodeId, size: Size) {
        match self {
            Driver::Headless(h) => h.resize_window(window, size),
            #[cfg(target_os = "macos")]
            Driver::AppKit(h) => h.resize_window(window, size),
        }
    }

    pub(crate) fn take_command_log(&self) -> Vec<Command> {
        match self {
            Driver::Headless(h) => h.take_command_log(),
            #[cfg(target_os = "macos")]
            Driver::AppKit(h) => h.take_command_log(),
        }
    }

    pub(crate) fn node_count(&self) -> usize {
        match self {
            Driver::Headless(h) => h.node_count(),
            #[cfg(target_os = "macos")]
            Driver::AppKit(h) => h.node_count(),
        }
    }
}

#[cfg(target_os = "macos")]
fn native() -> (Ui, Driver) {
    use mitsuami_appkit::{AppKitBackend, BackendOptions};
    let mtm = mitsuami_appkit::init_for_tests();
    let show = std::env::var("MITSUAMI_SHOW_WINDOWS").is_ok_and(|v| v == "1");
    let backend = AppKitBackend::new(
        mtm,
        BackendOptions {
            show_windows: show,
            record_commands: true,
            force_light_appearance: true,
            private_clipboard: true,
        },
    );
    let handle = backend.handle();
    let ui = Ui::new(backend);
    (ui, Driver::AppKit(handle))
}

#[cfg(not(target_os = "macos"))]
fn native() -> (Ui, Driver) {
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
