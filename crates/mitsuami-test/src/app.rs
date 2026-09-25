use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mitsuami_core::task::ManualClock;

use mitsuami_core::{A11yNode, Command, NodeId, NodeInfo, Role, Size, Ui, View};
use mitsuami_headless::{FakeServices, FakeServicesHandle, HeadlessHandle};
use mitsuami_reactive::Owner;

use crate::driver::{Driver, Mode};
use crate::locator::{Expectation, Locator};
use crate::query::{Query, by_label, by_role, by_test_id, by_text};
use crate::{format, snapshot, visual};

pub(crate) struct TestContext {
    /// `module::path::test_name`, without the crate.
    pub name: String,
    pub file_prefix: String,
    pub manifest_dir: &'static str,
}

/// The app under test: a [`Ui`] with one window, driven by the test.
pub struct TestApp {
    ui: Ui,
    clock: Rc<ManualClock>,
    services: FakeServicesHandle,
    driver: Driver,
    owner: Owner,
    window: Cell<Option<NodeId>>,
    pub(crate) context: TestContext,
}

/// How long assertions and `wait_for_tasks` wait for background work.
/// `MITSUAMI_WAIT_MS` overrides the default of 2 seconds.
pub(crate) fn wait_timeout() -> Duration {
    let ms = std::env::var("MITSUAMI_WAIT_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(2000);
    Duration::from_millis(ms)
}

/// Default window content size for tests.
pub const DEFAULT_WINDOW: Size = Size::new(800.0, 600.0);

impl TestApp {
    pub(crate) fn new(context: TestContext, mode: Mode) -> TestApp {
        let (ui, driver) = Driver::create(mode);
        // Tests own time: timers only fire when the test advances the clock.
        let clock = Rc::new(ManualClock::default());
        ui.set_clock(clock.clone());
        // Never real dialogs or the real clipboard, even on native backends.
        let (fake, services) = FakeServices::new();
        ui.set_services(Box::new(fake));
        let owner = Owner::new_root();
        owner.with(|| mitsuami_reactive::provide(ui.clone()));
        TestApp { ui, clock, services, driver, owner, window: Cell::new(None), context }
    }

    /// The scripted platform services: answer dialogs, inspect the
    /// clipboard and the menus, choose menu items.
    pub fn services(&self) -> &FakeServicesHandle {
        &self.services
    }

    /// Moves the test clock forward, firing due timers, then settles.
    pub async fn advance(&self, by: Duration) {
        self.clock.advance(by);
        self.settle().await;
    }

    /// Settles repeatedly until no tasks are left (background work included),
    /// or fails after the wait timeout.
    pub async fn wait_for_tasks(&self) {
        let deadline = Instant::now() + wait_timeout();
        loop {
            self.settle_now();
            if self.ui.pending_tasks() == 0 {
                return;
            }
            if Instant::now() > deadline {
                panic!("{} task(s) still running after {:?}", self.ui.pending_tasks(), wait_timeout());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    /// The backend this test runs on: `"headless"`, `"appkit"`, …
    pub fn backend_name(&self) -> &'static str {
        self.driver.name()
    }

    pub fn is_headless(&self) -> bool {
        matches!(self.driver, Driver::Headless(_))
    }

    /// The headless backend, for simulating system changes. Tests using it
    /// must be marked `#[mitsuami_test::test(headless)]`.
    pub fn headless(&self) -> &HeadlessHandle {
        match &self.driver {
            Driver::Headless(h) => h,
            #[allow(unreachable_patterns)]
            _ => panic!("this test uses headless-only APIs; mark it #[mitsuami_test::test(headless)]"),
        }
    }

    /// Commands the backend applied since the last call.
    pub fn take_command_log(&self) -> Vec<Command> {
        self.driver.take_command_log()
    }

    /// Number of live native widgets: a leak detector.
    pub fn native_node_count(&self) -> usize {
        self.driver.node_count()
    }

    /// Name of the running test.
    pub fn test_name(&self) -> &str {
        &self.context.name
    }

    /// Makes `value` available to [`inject`](mitsuami_reactive::inject) in
    /// everything mounted afterwards. Use it to swap in fake stores.
    pub fn provide<T: 'static>(&self, value: T) {
        self.owner.with(|| mitsuami_reactive::provide(value));
    }

    /// Mounts a view in the test window (800×600 unless resized) and settles.
    pub fn mount<V: View>(&self, view: impl FnOnce() -> V) -> NodeId {
        let window = self.window();
        let root = self.owner.with(|| view().build(&self.ui));
        self.ui.append_child(window, root);
        self.settle_now();
        root
    }

    /// The test window, created on first use.
    pub fn window(&self) -> NodeId {
        if let Some(window) = self.window.get() {
            return window;
        }
        let window = self.ui.create_window("mitsuami test", DEFAULT_WINDOW);
        self.window.set(Some(window));
        window
    }

    /// Runs until the UI is idle: events dispatched, effects flushed, native
    /// tree committed and laid out.
    pub async fn settle(&self) {
        self.settle_now();
    }

    pub(crate) fn settle_now(&self) {
        // The same loop the run loop uses: events a commit produces (a
        // resize, a scroll, focus moving) are handled before we look.
        self.ui.tick();
        self.check_mirror();
    }

    /// Simulates the user resizing the window.
    pub async fn resize(&self, width: f32, height: f32) {
        self.driver.resize_window(self.window(), Size::new(width, height));
        self.settle().await;
    }

    pub fn get(&self, query: Query) -> Locator<'_> {
        Locator::new(self, query)
    }

    pub fn get_by_role(&self, role: Role, name: impl Into<String>) -> Locator<'_> {
        self.get(by_role(role, name))
    }

    pub fn get_by_text(&self, text: impl Into<String>) -> Locator<'_> {
        self.get(by_text(text))
    }

    pub fn get_by_label(&self, label: impl Into<String>) -> Locator<'_> {
        self.get(by_label(label))
    }

    pub fn get_by_test_id(&self, id: impl Into<String>) -> Locator<'_> {
        self.get(by_test_id(id))
    }

    /// Starts an assertion about the node `query` finds.
    pub fn expect(&self, query: Query) -> Expectation<'_> {
        Expectation::new(self.get(query))
    }

    pub fn a11y_tree(&self) -> A11yNode {
        self.ui.a11y_tree(self.window()).expect("the test window exists")
    }

    pub fn inspect(&self) -> NodeInfo {
        self.ui.inspect(self.window()).expect("the test window exists")
    }

    /// Compares the native tree (kinds, text, frames in window
    /// coordinates) with `tests/snapshots/<test>@<name>.tree.txt`.
    #[track_caller]
    pub fn assert_tree_snapshot(&self, name: &str) {
        snapshot::assert(&self.context, Some(self.backend_name()), name, "tree.txt", &format::tree(&self.inspect()));
    }

    /// Compares the accessibility tree with a snapshot.
    #[track_caller]
    pub fn assert_a11y_snapshot(&self, name: &str) {
        // The accessibility tree is computed by the core: one snapshot for all backends.
        snapshot::assert(&self.context, None, name, "a11y.txt", &format::a11y(&self.a11y_tree()));
    }

    /// Compares an SVG wireframe of the layout with a snapshot.
    #[track_caller]
    pub fn assert_wireframe_snapshot(&self, name: &str) {
        snapshot::assert(
            &self.context,
            Some(self.backend_name()),
            name,
            "wireframe.svg",
            &format::wireframe(&self.inspect()),
        );
    }

    /// Compares the commands sent to the backend since the last call.
    #[track_caller]
    pub fn assert_commands_snapshot(&self, name: &str) {
        let log = self.driver.take_command_log();
        snapshot::assert(&self.context, Some(self.backend_name()), name, "commands.txt", &format::commands(&log));
    }

    /// Captures the window and compares it with the PNG baseline in
    /// `tests/visual/<backend>/`. Headless has no pixels, so it skips.
    #[track_caller]
    pub fn assert_visual_snapshot(&self, name: &str) {
        match self.ui.capture(self.window()) {
            Ok(image) => visual::assert(&self.context, self.backend_name(), name, &image),
            Err(mitsuami_core::backend::CaptureError::Unsupported) => {}
            Err(e) => panic!("cannot capture the window: {e:?}"),
        }
    }

    /// Disposes everything mounted and destroys the window.
    pub fn unmount(&self) {
        self.owner.dispose();
        if let Some(window) = self.window.take() {
            self.ui.destroy(window);
        }
        self.settle_now();
    }

    /// The native mirror must match the core tree after every settle.
    fn check_mirror(&self) {
        let Some(window) = self.window.get() else { return };
        let Some(root) = self.ui.inspect(window) else { return };
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            let native = self
                .ui
                .native_state(node.id)
                .unwrap_or_else(|| panic!("backend desync: {} {} has no native widget", node.kind.name(), node.id));
            let children: Vec<NodeId> = node.children.iter().map(|c| c.id).collect();
            assert_eq!(native.children, children, "backend desync: children of {} {}", node.kind.name(), node.id);
            if node.kind != mitsuami_core::WidgetKind::Window {
                let frame = self.ui.frame(node.id).unwrap_or_default();
                assert_eq!(native.frame, frame, "backend desync: frame of {} {}", node.kind.name(), node.id);
            }
            assert_eq!(
                native.scroll_offset,
                self.ui.scroll_offset(node.id),
                "backend desync: scroll offset of {} {}",
                node.kind.name(),
                node.id
            );
            let core_focused = self.ui.focused(window) == Some(node.id);
            assert_eq!(
                native.focused,
                core_focused,
                "backend desync: {} {} is {}focused natively but {}focused in the core (missing focus event?)",
                node.kind.name(),
                node.id,
                if native.focused { "" } else { "not " },
                if core_focused { "" } else { "not " },
            );
            for prop in &node.props {
                assert!(
                    native.props.contains(prop),
                    "backend desync: {} {} shows {:?}, core has {prop:?}",
                    node.kind.name(),
                    node.id,
                    native.props.iter().find(|p| p.key() == prop.key())
                );
            }
            stack.extend(node.children);
        }
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        if self.owner.is_alive() {
            self.owner.dispose();
        }
    }
}
