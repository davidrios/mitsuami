//! An in-memory backend for integration tests.
//!
//! It keeps a mirror of the native tree like a real backend would, validates
//! every command against the protocol (panicking on violations), and measures
//! text with fixed, platform-independent metrics so layouts are deterministic:
//! each character is `0.5em` wide and lines are `1.25em` tall.

mod services;

pub use services::{FakeServices, FakeServicesHandle, Pending, PendingAlert, PendingOpen, PendingSave};

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use mitsuami_core::a11y::{A11yAction, A11yProps, ActionError};
use mitsuami_core::backend::{
    AvailableSpace, Backend, CaptureError, EventSink, FontSizes, Image, Key, MeasureRequest, NativeState,
    PlatformMetrics, SyntheticInput,
};
use mitsuami_core::units::SpacingScale;
use mitsuami_core::{Command, EventValue, NodeId, Point, Prop, Rect, Size, TextStyle, UiEvent, WidgetKind, find_prop};

/// Fixed metrics: 16px body text, 4/8/12/16/24 spacing, scale factor 1.
pub fn metrics() -> PlatformMetrics {
    PlatformMetrics {
        scale_factor: 1.0,
        spacing: SpacingScale { xs: 4.0, sm: 8.0, md: 12.0, lg: 16.0, xl: 24.0 },
        font_sizes: FontSizes {
            large_title: 32.0,
            title: 24.0,
            headline: 18.0,
            body: 16.0,
            callout: 15.0,
            caption: 12.0,
            monospace: 14.0,
        },
        dark_mode: false,
        high_contrast: false,
        reduced_motion: false,
    }
}

struct HeadlessNode {
    kind: WidgetKind,
    props: Vec<Prop>,
    a11y: A11yProps,
    frame: Rect,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    scroll_offset: Point,
}

struct State {
    nodes: BTreeMap<NodeId, HeadlessNode>,
    events: Option<EventSink>,
    metrics: PlatformMetrics,
    log: Vec<Command>,
    focused: Option<NodeId>,
    focus_orders: BTreeMap<NodeId, Vec<NodeId>>,
}

impl State {
    fn node(&mut self, id: NodeId, command: &Command) -> &mut HeadlessNode {
        match self.nodes.get_mut(&id) {
            Some(node) => node,
            None => violation(command, &format!("node {id} does not exist")),
        }
    }

    fn emit(&self, id: NodeId, event: UiEvent) {
        if let Some(events) = &self.events {
            events.emit(id, event);
        }
    }

    fn set_prop(&mut self, id: NodeId, prop: Prop) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.props.retain(|p| p.key() != prop.key());
            node.props.push(prop);
        }
    }

    /// The next enabled control after `id` in its window's focus order (as
    /// sent by the core), wrapping around. Every control takes focus, as with
    /// full keyboard access.
    fn next_focusable(&self, id: NodeId) -> Option<NodeId> {
        let order = self.focus_orders.values().find(|order| order.contains(&id))?;
        let start = order.iter().position(|n| *n == id)?;
        (1..order.len())
            .map(|step| order[(start + step) % order.len()])
            .find(|candidate| self.nodes.get(candidate).is_some_and(|n| find_prop!(n.props, Enabled) != Some(false)))
    }

    /// Moves a scroll view, reporting it like a platform would.
    fn scroll(&mut self, id: NodeId, offset: Point) {
        let node = self.nodes.get_mut(&id).unwrap();
        if node.scroll_offset != offset {
            node.scroll_offset = offset;
            self.emit(id, UiEvent::Scrolled(offset));
        }
    }

    fn focus(&mut self, id: NodeId) {
        if self.focused == Some(id) {
            return;
        }
        if let Some(previous) = self.focused.replace(id) {
            self.emit(previous, UiEvent::FocusOut);
        }
        self.emit(id, UiEvent::FocusIn);
    }
}

fn violation(command: &Command, problem: &str) -> ! {
    panic!("headless backend: protocol violation in {command:?}: {problem}")
}

/// The backend. Hand it to [`Ui::new`](mitsuami_core::Ui::new); keep a
/// [`HeadlessHandle`] to inspect it afterwards.
pub struct HeadlessBackend {
    state: Rc<RefCell<State>>,
}

/// Shared access to a [`HeadlessBackend`] after it was moved into a `Ui`.
#[derive(Clone)]
pub struct HeadlessHandle {
    state: Rc<RefCell<State>>,
}

impl Default for HeadlessBackend {
    fn default() -> Self {
        HeadlessBackend::new()
    }
}

impl HeadlessBackend {
    pub fn new() -> HeadlessBackend {
        HeadlessBackend {
            state: Rc::new(RefCell::new(State {
                nodes: BTreeMap::new(),
                events: None,
                metrics: metrics(),
                log: Vec::new(),
                focused: None,
                focus_orders: BTreeMap::new(),
            })),
        }
    }

    pub fn handle(&self) -> HeadlessHandle {
        HeadlessHandle { state: self.state.clone() }
    }
}

impl HeadlessHandle {
    /// Every command applied so far, in order.
    pub fn command_log(&self) -> Vec<Command> {
        self.state.borrow().log.clone()
    }

    pub fn take_command_log(&self) -> Vec<Command> {
        std::mem::take(&mut self.state.borrow_mut().log)
    }

    /// Simulates the user resizing a window.
    pub fn resize_window(&self, window: NodeId, size: Size) {
        let mut state = self.state.borrow_mut();
        if let Some(node) = state.nodes.get_mut(&window) {
            node.frame.size = size;
        }
        state.emit(window, UiEvent::WindowResized(size));
    }

    /// Simulates a change of system settings (text size, dark mode, …).
    pub fn set_metrics(&self, metrics: PlatformMetrics) {
        let mut state = self.state.borrow_mut();
        state.metrics = metrics;
        let windows: Vec<NodeId> =
            state.nodes.iter().filter(|(_, n)| n.kind == WidgetKind::Window).map(|(id, _)| *id).collect();
        for window in windows {
            state.emit(window, UiEvent::MetricsChanged);
        }
    }

    pub fn focused(&self) -> Option<NodeId> {
        self.state.borrow().focused
    }

    pub fn a11y(&self, id: NodeId) -> Option<A11yProps> {
        self.state.borrow().nodes.get(&id).map(|n| n.a11y.clone())
    }

    /// Number of live native nodes: a leak detector for tests.
    pub fn node_count(&self) -> usize {
        self.state.borrow().nodes.len()
    }
}

impl mitsuami_core::TestHooks for HeadlessHandle {
    fn name(&self) -> &'static str {
        "headless"
    }

    fn resize_window(&self, window: NodeId, size: Size) {
        HeadlessHandle::resize_window(self, window, size);
    }

    fn take_command_log(&self) -> Vec<Command> {
        HeadlessHandle::take_command_log(self)
    }

    fn node_count(&self) -> usize {
        HeadlessHandle::node_count(self)
    }
}

impl Backend for HeadlessBackend {
    fn init(&mut self, events: EventSink) {
        self.state.borrow_mut().events = Some(events);
    }

    fn metrics(&self) -> PlatformMetrics {
        self.state.borrow().metrics.clone()
    }

    fn apply(&mut self, batch: &[Command]) {
        let mut state = self.state.borrow_mut();
        for command in batch {
            state.log.push(command.clone());
            match command {
                Command::Create { id, kind, props } => {
                    if state.nodes.contains_key(id) {
                        violation(command, "node already exists");
                    }
                    if *kind == WidgetKind::Fragment {
                        violation(command, "fragments are core-only");
                    }
                    state.nodes.insert(
                        *id,
                        HeadlessNode {
                            kind: *kind,
                            props: props.clone(),
                            a11y: A11yProps::default(),
                            frame: Rect::ZERO,
                            parent: None,
                            children: Vec::new(),
                            scroll_offset: Point::ZERO,
                        },
                    );
                }
                Command::SetProp { id, prop } => {
                    state.node(*id, command);
                    state.set_prop(*id, prop.clone());
                }
                Command::Insert { parent, child, index } => {
                    if let Some(p) = state.node(*child, command).parent {
                        violation(command, &format!("child is still attached to {p}"));
                    }
                    let parent_node = state.node(*parent, command);
                    if parent_node.kind == WidgetKind::ScrollView && !parent_node.children.is_empty() {
                        violation(command, "a ScrollView has a single native child (its content)");
                    }
                    let siblings = &mut state.node(*parent, command).children;
                    if *index > siblings.len() {
                        violation(command, &format!("index out of bounds (len {})", siblings.len()));
                    }
                    siblings.insert(*index, *child);
                    state.node(*child, command).parent = Some(*parent);
                }
                Command::Remove { parent, child } => {
                    let siblings = &mut state.node(*parent, command).children;
                    let Some(pos) = siblings.iter().position(|c| c == child) else {
                        violation(command, "not a child of this parent");
                    };
                    siblings.remove(pos);
                    state.node(*child, command).parent = None;
                }
                Command::Destroy { id } => {
                    let node = state.nodes.remove(id).unwrap_or_else(|| violation(command, "node does not exist"));
                    if let Some(parent) = node.parent.and_then(|p| state.nodes.get_mut(&p)) {
                        parent.children.retain(|c| c != id);
                    }
                    for child in node.children {
                        if let Some(child) = state.nodes.get_mut(&child) {
                            child.parent = None;
                        }
                    }
                    if state.focused == Some(*id) {
                        state.focused = None;
                    }
                    state.focus_orders.remove(id);
                }
                Command::SetFrame { id, frame } => {
                    let node = state.node(*id, command);
                    if node.kind == WidgetKind::Window {
                        violation(command, "window frames belong to the platform");
                    }
                    node.frame = *frame;
                }
                Command::SetA11y { id, a11y } => state.node(*id, command).a11y = a11y.clone(),
                Command::SetWindowSize { id, size } => state.node(*id, command).frame.size = *size,
                Command::SetFocusOrder { window, order } => {
                    if state.node(*window, command).kind != WidgetKind::Window {
                        violation(command, "not a window");
                    }
                    for id in order {
                        state.node(*id, command);
                    }
                    state.focus_orders.insert(*window, order.clone());
                }
                Command::ScrollTo { id, offset } => {
                    if state.node(*id, command).kind != WidgetKind::ScrollView {
                        violation(command, "not a ScrollView");
                    }
                    state.scroll(*id, *offset);
                }
                Command::Focus { id } => {
                    state.node(*id, command);
                    state.focus(*id);
                }
            }
        }
    }

    fn measure(&mut self, id: NodeId, request: MeasureRequest) -> Size {
        let state = self.state.borrow();
        let Some(node) = state.nodes.get(&id) else { return Size::ZERO };
        let fonts = &state.metrics.font_sizes;
        let font = fonts.get(find_prop!(node.props, TextStyle).unwrap_or(TextStyle::Body));
        let line = (font * 1.25).round();
        let label = || find_prop!(node.props, Label).unwrap_or_default();
        let natural = match node.kind {
            WidgetKind::Text => {
                let text = find_prop!(node.props, Text).unwrap_or_default();
                let wrap = request.known_width.or(match request.available_width {
                    AvailableSpace::Definite(w) => Some(w),
                    AvailableSpace::MinContent => Some(0.0),
                    AvailableSpace::MaxContent => None,
                });
                text_size(&text, font, wrap)
            }
            WidgetKind::Button => {
                let text = text_size(&label(), font, None);
                Size::new(text.width + 24.0, (line + 8.0).max(28.0))
            }
            WidgetKind::TextInput => Size::new(200.0, line + 8.0),
            WidgetKind::Checkbox => {
                let text = text_size(&label(), font, None);
                Size::new(16.0 + 6.0 + text.width, line.max(16.0))
            }
            WidgetKind::Switch => Size::new(40.0, 24.0),
            _ => Size::ZERO,
        };
        Size::new(request.known_width.unwrap_or(natural.width), request.known_height.unwrap_or(natural.height))
    }

    fn perform(&mut self, id: NodeId, action: &A11yAction) -> Result<(), ActionError> {
        let mut state = self.state.borrow_mut();
        let node = state.nodes.get(&id).ok_or(ActionError::UnknownNode)?;
        if find_prop!(node.props, Enabled) == Some(false) {
            return Err(ActionError::Disabled);
        }
        let kind = node.kind;
        match (action, kind) {
            (A11yAction::Activate, WidgetKind::Button) => {
                state.focus(id);
                state.emit(id, UiEvent::Click);
            }
            (A11yAction::Activate, WidgetKind::Checkbox | WidgetKind::Switch) => {
                let checked = !find_prop!(state.nodes[&id].props, Checked).unwrap_or(false);
                state.set_prop(id, Prop::Checked(checked));
                state.focus(id);
                state.emit(id, UiEvent::Changed(EventValue::Bool(checked)));
            }
            (A11yAction::SetValue(text), WidgetKind::TextInput) => {
                state.set_prop(id, Prop::Value(text.clone()));
                state.emit(id, UiEvent::Changed(EventValue::Text(text.clone())));
            }
            (
                A11yAction::Focus,
                WidgetKind::Button | WidgetKind::TextInput | WidgetKind::Checkbox | WidgetKind::Switch,
            ) => state.focus(id),
            (A11yAction::ScrollIntoView, _) => {}
            _ => return Err(ActionError::Unsupported),
        }
        Ok(())
    }

    fn synthesize(&mut self, id: NodeId, input: &SyntheticInput) -> Result<(), ActionError> {
        let mut state = self.state.borrow_mut();
        let node = state.nodes.get(&id).ok_or(ActionError::UnknownNode)?;
        if find_prop!(node.props, Enabled) == Some(false) {
            return Err(ActionError::Disabled);
        }
        let kind = node.kind;
        let key = match input {
            SyntheticInput::Key(key) => key,
            SyntheticInput::Scroll { dx, dy } => {
                if kind != WidgetKind::ScrollView {
                    return Err(ActionError::Unsupported);
                }
                let node = &state.nodes[&id];
                let axes = find_prop!(node.props, ScrollAxes).unwrap_or_default();
                let content = node.children.first().map_or(Size::ZERO, |c| state.nodes[c].frame.size);
                let viewport = node.frame.size;
                let clamp = |v: f32, content: f32, viewport: f32, on: bool| {
                    if on { v.clamp(0.0, (content - viewport).max(0.0)) } else { 0.0 }
                };
                let offset = Point::new(
                    clamp(node.scroll_offset.x + dx, content.width, viewport.width, axes.horizontal()),
                    clamp(node.scroll_offset.y + dy, content.height, viewport.height, axes.vertical()),
                );
                state.scroll(id, offset);
                return Ok(());
            }
        };
        match (kind, key) {
            (WidgetKind::TextInput, Key::Char(_) | Key::Backspace) => {
                state.focus(id);
                let mut text = find_prop!(state.nodes[&id].props, Value).unwrap_or_default();
                match key {
                    Key::Char(c) => text.push(*c),
                    _ => {
                        text.pop();
                    }
                }
                state.set_prop(id, Prop::Value(text.clone()));
                state.emit(id, UiEvent::Changed(EventValue::Text(text)));
            }
            (WidgetKind::TextInput, Key::Enter) => state.emit(id, UiEvent::Submit),
            // Moves focus on; the field keeps its text and does not submit.
            (WidgetKind::TextInput, Key::Tab) => {
                if let Some(next) = state.next_focusable(id) {
                    state.focus(next);
                }
            }
            (WidgetKind::Button, Key::Enter | Key::Char(' ')) => state.emit(id, UiEvent::Click),
            (WidgetKind::Checkbox | WidgetKind::Switch, Key::Char(' ')) => {
                drop(state);
                return self.perform(id, &A11yAction::Activate);
            }
            _ => return Err(ActionError::Unsupported),
        }
        Ok(())
    }

    fn native_state(&self, id: NodeId) -> Option<NativeState> {
        let state = self.state.borrow();
        let node = state.nodes.get(&id)?;
        Some(NativeState {
            kind: node.kind,
            props: node.props.clone(),
            frame: node.frame,
            parent: node.parent,
            children: node.children.clone(),
            focused: state.focused == Some(id),
            scroll_offset: (node.kind == WidgetKind::ScrollView).then_some(node.scroll_offset),
        })
    }

    fn capture(&mut self, _id: NodeId, reply: mitsuami_core::services::Reply<Result<Image, CaptureError>>) {
        reply(Err(CaptureError::Unsupported));
    }

    fn services(&self) -> Box<dyn mitsuami_core::services::Services> {
        Box::new(FakeServices::default())
    }
}

/// Greedy word wrap with fixed-width characters.
fn text_size(text: &str, font: f32, wrap_width: Option<f32>) -> Size {
    let char_width = font * 0.5;
    let line_height = (font * 1.25).round();
    let mut lines: Vec<usize> = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = 0usize;
        for word in paragraph.split_whitespace() {
            let len = word.chars().count();
            let candidate = if current == 0 { len } else { current + 1 + len };
            let fits = wrap_width.is_none_or(|w| candidate as f32 * char_width <= w + 0.01);
            if current == 0 || fits {
                current = candidate;
            } else {
                lines.push(current);
                current = len;
            }
        }
        lines.push(current);
    }
    let widest = lines.iter().copied().max().unwrap_or(0);
    Size::new(widest as f32 * char_width, lines.len().max(1) as f32 * line_height)
}
