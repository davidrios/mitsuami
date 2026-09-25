use std::cell::Cell;

use mitsuami_core::{A11yNode, NodeId, NodeInfo, Role, Size, Ui, View};
use mitsuami_headless::{HeadlessBackend, HeadlessHandle};
use mitsuami_reactive::Owner;

use crate::locator::{Expectation, Locator};
use crate::query::{Query, by_label, by_role, by_test_id, by_text};
use crate::{format, snapshot};

pub(crate) struct TestContext {
    /// `module::path::test_name`, without the crate.
    pub name: String,
    pub file_prefix: String,
    pub manifest_dir: &'static str,
}

/// The app under test: a [`Ui`] with one window, driven by the test.
pub struct TestApp {
    ui: Ui,
    headless: HeadlessHandle,
    owner: Owner,
    window: Cell<Option<NodeId>>,
    pub(crate) context: TestContext,
}

/// Default window content size for tests.
pub const DEFAULT_WINDOW: Size = Size::new(800.0, 600.0);

impl TestApp {
    pub(crate) fn new(context: TestContext) -> TestApp {
        let backend = HeadlessBackend::new();
        let headless = backend.handle();
        TestApp { ui: Ui::new(backend), headless, owner: Owner::new_root(), window: Cell::new(None), context }
    }

    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    /// The headless backend: command log, simulated system changes.
    pub fn headless(&self) -> &HeadlessHandle {
        &self.headless
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
        self.ui.process_events();
        self.ui.commit();
        self.check_mirror();
    }

    /// Simulates the user resizing the window.
    pub async fn resize(&self, width: f32, height: f32) {
        self.headless.resize_window(self.window(), Size::new(width, height));
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
        snapshot::assert(&self.context, name, "tree.txt", &format::tree(&self.inspect()));
    }

    /// Compares the accessibility tree with a snapshot.
    #[track_caller]
    pub fn assert_a11y_snapshot(&self, name: &str) {
        snapshot::assert(&self.context, name, "a11y.txt", &format::a11y(&self.a11y_tree()));
    }

    /// Compares an SVG wireframe of the layout with a snapshot.
    #[track_caller]
    pub fn assert_wireframe_snapshot(&self, name: &str) {
        snapshot::assert(&self.context, name, "wireframe.svg", &format::wireframe(&self.inspect()));
    }

    /// Compares the commands sent to the backend since the last call.
    #[track_caller]
    pub fn assert_commands_snapshot(&self, name: &str) {
        let log = self.headless.take_command_log();
        snapshot::assert(&self.context, name, "commands.txt", &format::commands(&log));
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
