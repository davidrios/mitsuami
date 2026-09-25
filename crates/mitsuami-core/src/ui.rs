//! The retained node tree and the update pipeline.
//!
//! The core tree is the source of truth. Backends mirror its *native* part:
//! every node except fragments, which are spliced into their nearest native
//! ancestor. Changes queue [`Command`]s; [`Ui::commit`] resolves styles, syncs
//! native children, applies the batch, runs layout and sends the new frames.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::{Rc, Weak};

use taffy::TaffyTree;

use crate::a11y::{A11yAction, A11yNode, A11yProps, ActionError, Role};
use crate::backend::{AvailableSpace, Backend, EventSink, MeasureRequest, NativeState, PlatformMetrics};
use crate::command::{Command, EventValue, UiEvent};
use crate::geometry::{Point, Rect, Size};
use crate::style::{Display, Style, TextDirection};
use crate::units::ResolveContext;
use crate::widget::{NodeId, Prop, WidgetKind};

pub(crate) type Handler = Rc<dyn Fn(&UiEvent)>;

struct Node {
    kind: WidgetKind,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    /// Native nodes only: the flattened children last sent to the backend.
    native_children: Vec<NodeId>,
    native_parent: Option<NodeId>,
    props: Vec<Prop>,
    style: Style,
    a11y: A11yProps,
    test_id: Option<String>,
    tab_index: Option<u32>,
    handlers: Vec<Handler>,
    taffy: Option<taffy::NodeId>,
    /// Relative to the native parent.
    frame: Rect,
    /// Windows only: content size.
    window_size: Size,
}

impl Node {
    fn prop(&self, key: &Prop) -> Option<&Prop> {
        self.props.iter().find(|p| p.key() == key.key())
    }
}

struct Inner {
    backend: Box<dyn Backend>,
    events: EventSink,
    metrics: PlatformMetrics,
    nodes: BTreeMap<NodeId, Node>,
    taffy: TaffyTree<NodeId>,
    next_id: u32,
    pending: Vec<Command>,
    windows: Vec<NodeId>,
    styles_dirty: bool,
    resync: BTreeSet<NodeId>,
    /// Last focus order sent, per window.
    focus_orders: BTreeMap<NodeId, Vec<NodeId>>,
    commit_scheduler: Option<Rc<dyn Fn()>>,
    commit_scheduled: bool,
}

/// Handle to a UI instance: one backend, its windows and their node trees.
/// Cheap to clone; all clones share the same state.
#[derive(Clone)]
pub struct Ui {
    inner: Rc<RefCell<Inner>>,
}

/// Non-owning [`Ui`] handle, for closures stored inside the tree itself.
#[derive(Clone)]
pub struct WeakUi {
    inner: Weak<RefCell<Inner>>,
}

impl WeakUi {
    pub fn upgrade(&self) -> Option<Ui> {
        self.inner.upgrade().map(|inner| Ui { inner })
    }
}

/// Snapshot of one native node, for inspection tools and tests.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeInfo {
    pub id: NodeId,
    pub kind: WidgetKind,
    pub props: Vec<Prop>,
    pub test_id: Option<String>,
    /// In window coordinates.
    pub frame: Rect,
    pub children: Vec<NodeInfo>,
}

impl Ui {
    pub fn new(backend: impl Backend + 'static) -> Ui {
        let mut backend: Box<dyn Backend> = Box::new(backend);
        let events = EventSink::default();
        backend.init(events.clone());
        let metrics = backend.metrics();
        Ui {
            inner: Rc::new(RefCell::new(Inner {
                backend,
                events,
                metrics,
                nodes: BTreeMap::new(),
                taffy: TaffyTree::new(),
                next_id: 1,
                pending: Vec::new(),
                windows: Vec::new(),
                styles_dirty: true,
                resync: BTreeSet::new(),
                focus_orders: BTreeMap::new(),
                commit_scheduler: None,
                commit_scheduled: false,
            })),
        }
    }

    pub fn downgrade(&self) -> WeakUi {
        WeakUi { inner: Rc::downgrade(&self.inner) }
    }

    pub fn events(&self) -> EventSink {
        self.inner.borrow().events.clone()
    }

    pub fn metrics(&self) -> PlatformMetrics {
        self.inner.borrow().metrics.clone()
    }

    /// Native backends call this to learn when a commit is needed. The
    /// callback should schedule [`Ui::commit`] on the run loop (e.g. before
    /// the next frame). Without a scheduler, callers commit explicitly.
    pub fn set_commit_scheduler(&self, schedule: impl Fn() + 'static) {
        self.inner.borrow_mut().commit_scheduler = Some(Rc::new(schedule));
    }

    fn changed(&self) {
        let schedule = {
            let mut inner = self.inner.borrow_mut();
            if inner.commit_scheduled {
                return;
            }
            inner.commit_scheduled = true;
            inner.commit_scheduler.clone()
        };
        if let Some(schedule) = schedule {
            schedule();
        }
    }

    // ------------------------------------------------------------ building

    pub fn create(&self, kind: WidgetKind, props: Vec<Prop>) -> NodeId {
        let id = {
            let mut inner = self.inner.borrow_mut();
            let id = NodeId(inner.next_id);
            inner.next_id += 1;
            let taffy = if kind.is_native() {
                let t = if kind.is_container() {
                    inner.taffy.new_leaf(taffy::Style::default())
                } else {
                    inner.taffy.new_leaf_with_context(taffy::Style::default(), id)
                };
                Some(t.expect("taffy: failed to create node"))
            } else {
                None
            };
            if kind.is_native() {
                inner.pending.push(Command::Create { id, kind, props: props.clone() });
            }
            inner.nodes.insert(
                id,
                Node {
                    kind,
                    parent: None,
                    children: Vec::new(),
                    native_children: Vec::new(),
                    native_parent: None,
                    props,
                    style: Style::default(),
                    a11y: A11yProps::default(),
                    test_id: None,
                    tab_index: None,
                    handlers: Vec::new(),
                    taffy,
                    frame: Rect::ZERO,
                    window_size: Size::ZERO,
                },
            );
            inner.styles_dirty = true;
            id
        };
        self.changed();
        id
    }

    pub fn create_window(&self, title: impl Into<String>, size: Size) -> NodeId {
        let id = self.create(WidgetKind::Window, vec![Prop::Title(title.into())]);
        let mut inner = self.inner.borrow_mut();
        let node = inner.nodes.get_mut(&id).expect("just created");
        node.window_size = size;
        node.frame = Rect { origin: Point::ZERO, size };
        inner.windows.push(id);
        inner.pending.push(Command::SetWindowSize { id, size });
        id
    }

    /// Sets a prop, sending it to the backend only if it changed.
    pub fn set_prop(&self, id: NodeId, prop: Prop) {
        {
            let mut inner = self.inner.borrow_mut();
            let inner = &mut *inner;
            let Some(node) = inner.nodes.get_mut(&id) else { return };
            if node.prop(&prop) == Some(&prop) {
                return;
            }
            node.props.retain(|p| p.key() != prop.key());
            node.props.push(prop.clone());
            if prop.affects_measure()
                && let Some(t) = node.taffy
            {
                let _ = inner.taffy.mark_dirty(t);
            }
            if matches!(prop, Prop::TextStyle(_)) {
                inner.styles_dirty = true;
            }
            if node.kind.is_native() {
                inner.pending.push(Command::SetProp { id, prop });
            }
        }
        self.changed();
    }

    pub fn set_style(&self, id: NodeId, style: Style) {
        {
            let mut inner = self.inner.borrow_mut();
            let Some(node) = inner.nodes.get_mut(&id) else { return };
            if node.style == style {
                return;
            }
            node.style = style;
            inner.styles_dirty = true;
        }
        self.changed();
    }

    pub fn set_a11y(&self, id: NodeId, a11y: A11yProps) {
        {
            let mut inner = self.inner.borrow_mut();
            let Some(node) = inner.nodes.get_mut(&id) else { return };
            if node.a11y == a11y {
                return;
            }
            node.a11y = a11y.clone();
            if node.kind.is_native() {
                inner.pending.push(Command::SetA11y { id, a11y });
            }
        }
        self.changed();
    }

    /// Moves a control ahead in the Tab order: controls with a tab index
    /// come first, lowest index first, then everything else in tree order.
    pub fn set_tab_index(&self, id: NodeId, index: Option<u32>) {
        if let Some(node) = self.inner.borrow_mut().nodes.get_mut(&id) {
            node.tab_index = index;
        }
        self.changed();
    }

    /// The keyboard order of a window's focusable controls, as sent to the
    /// backend.
    pub fn focus_order(&self, window: NodeId) -> Vec<NodeId> {
        self.inner.borrow().focus_order(window)
    }

    pub fn set_test_id(&self, id: NodeId, test_id: impl Into<String>) {
        if let Some(node) = self.inner.borrow_mut().nodes.get_mut(&id) {
            node.test_id = Some(test_id.into());
        }
    }

    pub fn on_event(&self, id: NodeId, handler: impl Fn(&UiEvent) + 'static) {
        if let Some(node) = self.inner.borrow_mut().nodes.get_mut(&id) {
            node.handlers.push(Rc::new(handler));
        }
    }

    pub fn focus(&self, id: NodeId) {
        self.inner.borrow_mut().pending.push(Command::Focus { id });
        self.changed();
    }

    // ------------------------------------------------------- tree structure

    /// Inserts `child` under `parent` at `index` (or at the end), detaching it
    /// from its previous parent first.
    pub fn insert_child(&self, parent: NodeId, child: NodeId, index: Option<usize>) {
        {
            let mut inner = self.inner.borrow_mut();
            if !inner.nodes.contains_key(&parent) || !inner.nodes.contains_key(&child) {
                return;
            }
            inner.detach(child);
            let siblings = &mut inner.nodes.get_mut(&parent).unwrap().children;
            let index = index.unwrap_or(siblings.len()).min(siblings.len());
            siblings.insert(index, child);
            inner.nodes.get_mut(&child).unwrap().parent = Some(parent);
            inner.mark_resync(parent);
            inner.styles_dirty = true;
        }
        self.changed();
    }

    pub fn append_child(&self, parent: NodeId, child: NodeId) {
        self.insert_child(parent, child, None);
    }

    /// Replaces the children of `parent`, keeping nodes that stay.
    pub fn set_children(&self, parent: NodeId, children: Vec<NodeId>) {
        {
            let mut inner = self.inner.borrow_mut();
            if !inner.nodes.contains_key(&parent) {
                return;
            }
            let old = inner.nodes[&parent].children.clone();
            for child in old.iter().filter(|c| !children.contains(c)) {
                inner.detach(*child);
            }
            for child in &children {
                if inner.nodes.get(child).and_then(|n| n.parent) != Some(parent) {
                    inner.detach(*child);
                }
                if let Some(node) = inner.nodes.get_mut(child) {
                    node.parent = Some(parent);
                }
            }
            inner.nodes.get_mut(&parent).unwrap().children = children;
            inner.mark_resync(parent);
            inner.styles_dirty = true;
        }
        self.changed();
    }

    /// Removes a node and its whole subtree, natively too.
    pub fn destroy(&self, id: NodeId) {
        {
            let mut inner = self.inner.borrow_mut();
            if !inner.nodes.contains_key(&id) {
                return;
            }
            inner.detach(id);
            // Remove the subtree's native roots from their native parent now,
            // so the backend sees Remove before Destroy.
            let roots = inner.native_roots(id);
            for root in &roots {
                inner.detach_native(*root);
            }
            let mut subtree = Vec::new();
            inner.collect_subtree(id, &mut subtree);
            for node_id in subtree {
                let node = inner.nodes.remove(&node_id).expect("in subtree");
                if let Some(t) = node.taffy {
                    let _ = inner.taffy.remove(t);
                }
                inner.resync.remove(&node_id);
                inner.windows.retain(|w| *w != node_id);
                inner.focus_orders.remove(&node_id);
                if node.kind.is_native() {
                    inner.pending.push(Command::Destroy { id: node_id });
                }
            }
        }
        self.changed();
    }

    // ------------------------------------------------------------- pipeline

    /// Drains native events and dispatches them to handlers. Handlers run in
    /// one reactive batch per event.
    pub fn process_events(&self) {
        loop {
            let next = self.inner.borrow().events.pop();
            let Some((id, event)) = next else { break };
            let handlers = {
                let mut inner = self.inner.borrow_mut();
                inner.absorb(id, &event);
                inner.nodes.get(&id).map(|n| n.handlers.clone()).unwrap_or_default()
            };
            self.changed();
            mitsuami_reactive::batch(|| {
                for handler in &handlers {
                    handler(&event);
                }
            });
        }
    }

    /// Brings the native tree up to date: styles, structure, props, layout.
    pub fn commit(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.commit_scheduled = false;
        if inner.styles_dirty {
            inner.resolve_styles();
        }
        inner.resync_all();
        inner.sync_focus_orders();
        let batch = std::mem::take(&mut inner.pending);
        if !batch.is_empty() {
            inner.backend.apply(&batch);
        }
        inner.layout();
        let frames = std::mem::take(&mut inner.pending);
        if !frames.is_empty() {
            inner.backend.apply(&frames);
        }
    }

    /// One run-loop turn: dispatch events and commit, repeating while the
    /// commit itself produced new events (e.g. a window resize), until idle.
    /// Backends call this from their run loop, before it goes to sleep.
    pub fn tick(&self) {
        const MAX_TURNS: usize = 16;
        for _ in 0..MAX_TURNS {
            self.process_events();
            self.commit();
            if self.inner.borrow().events.is_empty() {
                return;
            }
        }
    }

    /// Asks the backend to perform an accessibility action, then dispatches
    /// the events it produced.
    pub fn perform(&self, id: NodeId, action: &A11yAction) -> Result<(), ActionError> {
        let result = self.inner.borrow_mut().backend.perform(id, action);
        self.process_events();
        result
    }

    /// Synthesizes raw input on a native control, then dispatches the events
    /// it produced.
    pub fn synthesize(&self, id: NodeId, input: &crate::backend::SyntheticInput) -> Result<(), ActionError> {
        let result = self.inner.borrow_mut().backend.synthesize(id, input);
        self.process_events();
        result
    }

    // ------------------------------------------------------------- queries

    pub fn windows(&self) -> Vec<NodeId> {
        self.inner.borrow().windows.clone()
    }

    pub fn exists(&self, id: NodeId) -> bool {
        self.inner.borrow().nodes.contains_key(&id)
    }

    pub fn kind(&self, id: NodeId) -> Option<WidgetKind> {
        self.inner.borrow().nodes.get(&id).map(|n| n.kind)
    }

    pub fn props(&self, id: NodeId) -> Vec<Prop> {
        self.inner.borrow().nodes.get(&id).map(|n| n.props.clone()).unwrap_or_default()
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.inner.borrow().nodes.get(&id).and_then(|n| n.parent)
    }

    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        self.inner.borrow().nodes.get(&id).map(|n| n.children.clone()).unwrap_or_default()
    }

    pub fn native_children(&self, id: NodeId) -> Vec<NodeId> {
        self.inner.borrow().nodes.get(&id).map(|n| n.native_children.clone()).unwrap_or_default()
    }

    /// Frame relative to the native parent, as last sent to the backend.
    pub fn frame(&self, id: NodeId) -> Option<Rect> {
        self.inner.borrow().nodes.get(&id).map(|n| n.frame)
    }

    /// Frame in the coordinates of the node's window.
    pub fn window_frame(&self, id: NodeId) -> Option<Rect> {
        let inner = self.inner.borrow();
        let mut frame = inner.nodes.get(&id)?.frame;
        let mut current = inner.nodes[&id].native_parent;
        while let Some(parent) = current {
            let node = &inner.nodes[&parent];
            if node.kind != WidgetKind::Window {
                frame = frame.offset(node.frame.origin);
            }
            current = node.native_parent;
        }
        Some(frame)
    }

    /// The window a node is attached to, if any.
    pub fn window_of(&self, id: NodeId) -> Option<NodeId> {
        let inner = self.inner.borrow();
        let mut current = Some(id);
        while let Some(node_id) = current {
            let node = inner.nodes.get(&node_id)?;
            if node.kind == WidgetKind::Window {
                return Some(node_id);
            }
            current = node.parent;
        }
        None
    }

    pub fn window_size(&self, window: NodeId) -> Option<Size> {
        self.inner.borrow().nodes.get(&window).map(|n| n.window_size)
    }

    pub fn native_state(&self, id: NodeId) -> Option<NativeState> {
        self.inner.borrow().backend.native_state(id)
    }

    pub fn capture(&self, id: NodeId) -> Result<crate::backend::Image, crate::backend::CaptureError> {
        self.inner.borrow_mut().backend.capture(id)
    }

    /// The native tree under `id`, with window-coordinate frames.
    pub fn inspect(&self, id: NodeId) -> Option<NodeInfo> {
        let inner = self.inner.borrow();
        let origin = inner.window_origin(id)?;
        Some(inner.inspect(id, origin))
    }

    /// The computed accessibility tree of a window.
    pub fn a11y_tree(&self, window: NodeId) -> Option<A11yNode> {
        let inner = self.inner.borrow();
        let mut nodes = inner.a11y(window, Point::ZERO);
        (nodes.len() == 1).then(|| nodes.remove(0))
    }
}

impl Inner {
    fn mark_resync(&mut self, id: NodeId) {
        if let Some(native) = self.native_ancestor_or_self(id) {
            self.resync.insert(native);
        }
    }

    fn native_ancestor_or_self(&self, id: NodeId) -> Option<NodeId> {
        let mut current = Some(id);
        while let Some(node_id) = current {
            let node = self.nodes.get(&node_id)?;
            if node.kind.is_native() {
                return Some(node_id);
            }
            current = node.parent;
        }
        None
    }

    fn detach(&mut self, child: NodeId) {
        let Some(parent) = self.nodes.get(&child).and_then(|n| n.parent) else { return };
        if let Some(node) = self.nodes.get_mut(&parent) {
            node.children.retain(|c| *c != child);
        }
        self.nodes.get_mut(&child).unwrap().parent = None;
        self.mark_resync(parent);
    }

    /// Removes a native node from its native parent, immediately.
    fn detach_native(&mut self, child: NodeId) {
        let Some(parent) = self.nodes.get(&child).and_then(|n| n.native_parent) else { return };
        self.pending.push(Command::Remove { parent, child });
        let child_taffy = self.nodes[&child].taffy;
        if let Some(node) = self.nodes.get_mut(&parent) {
            node.native_children.retain(|c| *c != child);
            if let (Some(p), Some(c)) = (node.taffy, child_taffy) {
                let _ = self.taffy.remove_child(p, c);
            }
        }
        self.nodes.get_mut(&child).unwrap().native_parent = None;
    }

    /// The native nodes a subtree contributes to its native parent.
    fn native_roots(&self, id: NodeId) -> Vec<NodeId> {
        let node = &self.nodes[&id];
        if node.kind.is_native() {
            return vec![id];
        }
        node.children.iter().flat_map(|c| self.native_roots(*c)).collect()
    }

    fn flattened_children(&self, id: NodeId) -> Vec<NodeId> {
        self.nodes[&id].children.iter().flat_map(|c| self.native_roots(*c)).collect()
    }

    fn collect_subtree(&self, id: NodeId, out: &mut Vec<NodeId>) {
        for child in &self.nodes[&id].children {
            self.collect_subtree(*child, out);
        }
        out.push(id);
    }

    fn resync_all(&mut self) {
        let dirty = std::mem::take(&mut self.resync);
        for id in dirty {
            if self.nodes.contains_key(&id) {
                self.resync_node(id);
            }
        }
    }

    /// Makes the backend's children of `parent` match the core tree.
    fn resync_node(&mut self, parent: NodeId) {
        let desired = self.flattened_children(parent);
        let current = self.nodes[&parent].native_children.clone();
        for child in current.iter().filter(|c| !desired.contains(c)) {
            self.detach_native(*child);
        }
        let mut working: Vec<NodeId> = self.nodes[&parent].native_children.clone();
        for (index, child) in desired.iter().enumerate() {
            if working.get(index) == Some(child) {
                continue;
            }
            if let Some(pos) = working.iter().position(|c| c == child) {
                working.remove(pos);
                self.pending.push(Command::Remove { parent, child: *child });
            } else if let Some(other) = self.nodes[child].native_parent
                && other != parent
            {
                self.detach_native(*child);
            }
            working.insert(index, *child);
            self.pending.push(Command::Insert { parent, child: *child, index });
        }
        for child in &desired {
            self.nodes.get_mut(child).unwrap().native_parent = Some(parent);
        }
        let taffy_children: Vec<_> = desired.iter().filter_map(|c| self.nodes[c].taffy).collect();
        if let Some(t) = self.nodes[&parent].taffy {
            let _ = self.taffy.set_children(t, &taffy_children);
        }
        self.nodes.get_mut(&parent).unwrap().native_children = desired;
    }

    /// Focusable controls in reading (tree) order, explicit tab indices first.
    fn focus_order(&self, window: NodeId) -> Vec<NodeId> {
        fn walk(inner: &Inner, id: NodeId, out: &mut Vec<(Option<u32>, NodeId)>) {
            let node = &inner.nodes[&id];
            if node.style.display == Display::None {
                return;
            }
            if matches!(
                node.kind,
                WidgetKind::Button | WidgetKind::TextInput | WidgetKind::Checkbox | WidgetKind::Switch
            ) {
                out.push((node.tab_index, id));
            }
            for child in &node.native_children {
                walk(inner, *child, out);
            }
        }
        let mut entries = Vec::new();
        if self.nodes.contains_key(&window) {
            walk(self, window, &mut entries);
        }
        // Stable sort: explicit indices first (ascending), tree order otherwise.
        entries.sort_by_key(|(index, _)| index.unwrap_or(u32::MAX));
        entries.into_iter().map(|(_, id)| id).collect()
    }

    fn sync_focus_orders(&mut self) {
        for window in self.windows.clone() {
            let order = self.focus_order(window);
            if self.focus_orders.get(&window) != Some(&order) {
                self.focus_orders.insert(window, order.clone());
                self.pending.push(Command::SetFocusOrder { window, order });
            }
        }
    }

    fn resolve_styles(&mut self) {
        self.styles_dirty = false;
        let body = self.metrics.font_sizes.body;
        for window in self.windows.clone() {
            let viewport = self.nodes[&window].window_size;
            self.resolve_node(window, body, false, viewport);
        }
    }

    fn resolve_node(&mut self, id: NodeId, inherited_font: f32, inherited_rtl: bool, viewport: Size) {
        let node = &self.nodes[&id];
        let font_size = match crate::find_prop!(node.props, TextStyle) {
            Some(style) => self.metrics.font_sizes.get(style),
            None => inherited_font,
        };
        let rtl = match node.style.direction {
            TextDirection::Inherit => inherited_rtl,
            TextDirection::Ltr => false,
            TextDirection::Rtl => true,
        };
        if let Some(t) = node.taffy {
            let cx = ResolveContext {
                font_size,
                root_font_size: self.metrics.font_sizes.body,
                viewport,
                spacing: &self.metrics.spacing,
            };
            let mut style = node.style.to_taffy(&cx, rtl);
            if node.kind == WidgetKind::Window {
                style.size = taffy::Size {
                    width: taffy::Dimension::length(node.window_size.width),
                    height: taffy::Dimension::length(node.window_size.height),
                };
            }
            if self.taffy.style(t).ok() != Some(&style) {
                let _ = self.taffy.set_style(t, style);
            }
        }
        for child in self.nodes[&id].children.clone() {
            self.resolve_node(child, font_size, rtl, viewport);
        }
    }

    fn layout(&mut self) {
        for window in self.windows.clone() {
            let node = &self.nodes[&window];
            let Some(root) = node.taffy else { continue };
            let size = node.window_size;
            let Inner { taffy, backend, .. } = self;
            let available = taffy::Size {
                width: taffy::AvailableSpace::Definite(size.width),
                height: taffy::AvailableSpace::Definite(size.height),
            };
            let _ = taffy.compute_layout_with_measure(root, available, |input, _, context, style| {
                taffy::compute_leaf_layout(
                    input,
                    style,
                    |_, _| 0.0,
                    |known, available| match context {
                        Some(id) => {
                            let size = backend.measure(
                                *id,
                                MeasureRequest {
                                    known_width: known.width,
                                    known_height: known.height,
                                    available_width: space(available.width),
                                    available_height: space(available.height),
                                },
                            );
                            taffy::Size { width: size.width, height: size.height }
                        }
                        None => taffy::Size::ZERO,
                    },
                )
            });
            self.nodes.get_mut(&window).unwrap().frame = Rect { origin: Point::ZERO, size };
            self.collect_frames(window);
        }
    }

    fn collect_frames(&mut self, parent: NodeId) {
        for child in self.nodes[&parent].native_children.clone() {
            let node = &self.nodes[&child];
            if let Some(t) = node.taffy
                && let Ok(layout) = self.taffy.layout(t)
            {
                let frame = Rect::new(layout.location.x, layout.location.y, layout.size.width, layout.size.height);
                if frame != node.frame {
                    self.nodes.get_mut(&child).unwrap().frame = frame;
                    self.pending.push(Command::SetFrame { id: child, frame });
                }
            }
            self.collect_frames(child);
        }
    }

    /// Updates core state from events the native side already reflects, so
    /// echoing the value back is a no-op.
    fn absorb(&mut self, id: NodeId, event: &UiEvent) {
        match event {
            UiEvent::Changed(value) => {
                let Some(node) = self.nodes.get_mut(&id) else { return };
                let prop = match (node.kind, value) {
                    (WidgetKind::TextInput, EventValue::Text(text)) => Prop::Value(text.clone()),
                    (WidgetKind::Checkbox | WidgetKind::Switch, EventValue::Bool(b)) => Prop::Checked(*b),
                    _ => return,
                };
                node.props.retain(|p| p.key() != prop.key());
                node.props.push(prop);
            }
            UiEvent::WindowResized(size) => {
                if let Some(node) = self.nodes.get_mut(&id) {
                    node.window_size = *size;
                    self.styles_dirty = true;
                }
            }
            UiEvent::MetricsChanged => {
                self.metrics = self.backend.metrics();
                self.styles_dirty = true;
                for node in self.nodes.values() {
                    if let Some(t) = node.taffy {
                        let _ = self.taffy.mark_dirty(t);
                    }
                }
            }
            _ => {}
        }
    }

    fn window_origin(&self, id: NodeId) -> Option<Point> {
        let mut origin = Point::ZERO;
        let mut current = self.nodes.get(&id)?.native_parent;
        while let Some(parent) = current {
            let node = &self.nodes[&parent];
            if node.kind != WidgetKind::Window {
                origin = Point::new(origin.x + node.frame.origin.x, origin.y + node.frame.origin.y);
            }
            current = node.native_parent;
        }
        Some(origin)
    }

    fn inspect(&self, id: NodeId, parent_origin: Point) -> NodeInfo {
        let node = &self.nodes[&id];
        let frame = if node.kind == WidgetKind::Window { node.frame } else { node.frame.offset(parent_origin) };
        let origin = if node.kind == WidgetKind::Window { Point::ZERO } else { frame.origin };
        NodeInfo {
            id,
            kind: node.kind,
            props: node.props.clone(),
            test_id: node.test_id.clone(),
            frame,
            children: node.native_children.iter().map(|c| self.inspect(*c, origin)).collect(),
        }
    }

    fn text_of(&self, id: NodeId) -> Option<String> {
        let node = self.nodes.get(&id)?;
        crate::find_prop!(node.props, Text).or_else(|| crate::find_prop!(node.props, Label))
    }

    fn a11y(&self, id: NodeId, parent_origin: Point) -> Vec<A11yNode> {
        let node = &self.nodes[&id];
        if node.a11y.hidden || node.style.display == Display::None {
            return Vec::new();
        }
        let frame = if node.kind == WidgetKind::Window { node.frame } else { node.frame.offset(parent_origin) };
        let origin = if node.kind == WidgetKind::Window { Point::ZERO } else { frame.origin };
        let children: Vec<A11yNode> = node.native_children.iter().flat_map(|c| self.a11y(*c, origin)).collect();

        let labelled = node.a11y.label.is_some() || node.a11y.labelled_by.is_some();
        let role = node.a11y.role.unwrap_or(match node.kind {
            WidgetKind::Window => Role::Window,
            WidgetKind::Container if labelled => Role::Group,
            WidgetKind::Container | WidgetKind::Fragment => Role::None,
            WidgetKind::Text => Role::StaticText,
            WidgetKind::Button => Role::Button,
            WidgetKind::TextInput => Role::TextField,
            WidgetKind::Checkbox => Role::Checkbox,
            WidgetKind::Switch => Role::Switch,
            WidgetKind::Custom(_) | WidgetKind::Native => Role::Group,
        });
        if role == Role::None {
            return children;
        }
        let props = &node.props;
        let name =
            node.a11y.label.clone().or_else(|| node.a11y.labelled_by.and_then(|l| self.text_of(l))).or_else(|| {
                match node.kind {
                    WidgetKind::Window => crate::find_prop!(props, Title),
                    WidgetKind::Text => crate::find_prop!(props, Text),
                    WidgetKind::Button | WidgetKind::Checkbox | WidgetKind::Switch => crate::find_prop!(props, Label),
                    WidgetKind::TextInput => crate::find_prop!(props, Placeholder),
                    _ => None,
                }
            });
        vec![A11yNode {
            id,
            role,
            name,
            description: node.a11y.description.clone(),
            value: match node.kind {
                WidgetKind::TextInput => Some(crate::find_prop!(props, Value).unwrap_or_default()),
                _ => None,
            },
            checked: match node.kind {
                WidgetKind::Checkbox | WidgetKind::Switch => Some(crate::find_prop!(props, Checked).unwrap_or(false)),
                _ => None,
            },
            enabled: crate::find_prop!(props, Enabled).unwrap_or(true),
            test_id: node.test_id.clone(),
            frame,
            children,
        }]
    }
}

fn space(s: taffy::AvailableSpace) -> AvailableSpace {
    match s {
        taffy::AvailableSpace::Definite(v) => AvailableSpace::Definite(v),
        taffy::AvailableSpace::MinContent => AvailableSpace::MinContent,
        taffy::AvailableSpace::MaxContent => AvailableSpace::MaxContent,
    }
}
