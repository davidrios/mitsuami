//! The [`Backend`] implementation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mitsuami_core::a11y::{A11yAction, A11yProps, ActionError};
use mitsuami_core::backend::{
    AvailableSpace, Backend, CaptureError, EventSink, FontSizes, Image, Key, MeasureRequest, NativeState,
    PlatformMetrics, SyntheticInput,
};
use mitsuami_core::units::SpacingScale;
use mitsuami_core::{ButtonVariant, Command, EventValue, NodeId, Prop, Rect, Size, TextStyle, UiEvent, WidgetKind};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, Message, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSBackingStoreType, NSBitmapFormat, NSButton, NSControl, NSControlStateValueOff,
    NSControlStateValueOn, NSFont, NSFontTextStyle, NSFontTextStyleBody, NSFontTextStyleCallout,
    NSFontTextStyleCaption1, NSFontTextStyleHeadline, NSFontTextStyleLargeTitle, NSFontTextStyleTitle1,
    NSFontWeightRegular, NSScreen, NSStandardKeyBindingResponding, NSSwitch, NSTextField, NSView, NSWindow,
    NSWindowOrderingMode, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{NSArray, NSDictionary, NSPoint, NSRange, NSRect, NSSize, NSString};

use crate::classes::{ActionTarget, HostView, WindowDelegate};

/// How the backend behaves; apps and tests want different things.
#[derive(Clone, Debug)]
pub struct BackendOptions {
    /// Order windows front once their first layout is applied. Tests keep
    /// windows offscreen: layout, events and capture all work without it.
    pub show_windows: bool,
    /// Keep a log of applied commands (for tests).
    pub record_commands: bool,
    /// Force the light appearance, so captures are comparable across
    /// machines regardless of system settings.
    pub force_light_appearance: bool,
}

impl Default for BackendOptions {
    fn default() -> Self {
        BackendOptions { show_windows: true, record_commands: false, force_light_appearance: false }
    }
}

enum Widget {
    Window { window: Retained<NSWindow>, host: Retained<HostView>, _delegate: Retained<WindowDelegate> },
    Host(Retained<HostView>),
    Label(Retained<NSTextField>),
    Field(Retained<NSTextField>),
    Button(Retained<NSButton>),
    Checkbox(Retained<NSButton>),
    Switch(Retained<NSSwitch>),
}

impl Widget {
    fn view(&self) -> &NSView {
        match self {
            Widget::Window { host, .. } => host,
            Widget::Host(v) => v,
            Widget::Label(v) | Widget::Field(v) => v,
            Widget::Button(v) | Widget::Checkbox(v) => v,
            Widget::Switch(v) => v,
        }
    }

    fn control(&self) -> Option<&NSControl> {
        match self {
            Widget::Label(v) | Widget::Field(v) => Some(v),
            Widget::Button(v) | Widget::Checkbox(v) => Some(v),
            Widget::Switch(v) => Some(v),
            Widget::Window { .. } | Widget::Host(_) => None,
        }
    }
}

struct Node {
    kind: WidgetKind,
    widget: Widget,
    /// Controls only hold weak references to their target and delegate.
    _target: Option<Retained<ActionTarget>>,
    parent: Option<NodeId>,
    /// Props AppKit can't report back faithfully.
    text_style: Option<TextStyle>,
    variant: Option<ButtonVariant>,
}

struct State {
    mtm: MainThreadMarker,
    options: BackendOptions,
    nodes: HashMap<NodeId, Node>,
    by_view: HashMap<usize, NodeId>,
    events: EventSink,
    log: Vec<Command>,
    pending_show: Vec<NodeId>,
}

pub struct AppKitBackend {
    state: Rc<RefCell<State>>,
}

/// Shared access to an [`AppKitBackend`] after it was moved into a `Ui`.
#[derive(Clone)]
pub struct AppKitHandle {
    state: Rc<RefCell<State>>,
}

fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

fn key(view: &NSView) -> usize {
    view as *const NSView as usize
}

fn font(style: TextStyle) -> Retained<NSFont> {
    let text_style: &NSFontTextStyle = unsafe {
        match style {
            TextStyle::LargeTitle => NSFontTextStyleLargeTitle,
            TextStyle::Title => NSFontTextStyleTitle1,
            TextStyle::Headline => NSFontTextStyleHeadline,
            TextStyle::Body => NSFontTextStyleBody,
            TextStyle::Callout => NSFontTextStyleCallout,
            TextStyle::Caption => NSFontTextStyleCaption1,
            TextStyle::Monospace => {
                let size = font(TextStyle::Body).pointSize();
                return NSFont::monospacedSystemFontOfSize_weight(size, NSFontWeightRegular);
            }
        }
    };
    unsafe { NSFont::preferredFontForTextStyle_options(text_style, &NSDictionary::new()) }
}

fn metrics(mtm: MainThreadMarker) -> PlatformMetrics {
    let size = |style| font(style).pointSize() as f32;
    let appearance = NSApplication::sharedApplication(mtm).effectiveAppearance();
    let candidates = unsafe { [NSAppearanceNameAqua, NSAppearanceNameDarkAqua] };
    let names = NSArray::from_slice(&candidates);
    let dark = appearance
        .bestMatchFromAppearancesWithNames(&names)
        .is_some_and(|name| &*name == unsafe { NSAppearanceNameDarkAqua });
    let workspace = NSWorkspace::sharedWorkspace();
    PlatformMetrics {
        scale_factor: NSScreen::mainScreen(mtm).map_or(1.0, |s| s.backingScaleFactor() as f32),
        // The macOS HIG favours tight spacing; 20pt is the standard window margin.
        spacing: SpacingScale { xs: 4.0, sm: 6.0, md: 8.0, lg: 12.0, xl: 20.0 },
        font_sizes: FontSizes {
            large_title: size(TextStyle::LargeTitle),
            title: size(TextStyle::Title),
            headline: size(TextStyle::Headline),
            body: size(TextStyle::Body),
            callout: size(TextStyle::Callout),
            caption: size(TextStyle::Caption),
            monospace: size(TextStyle::Monospace),
        },
        dark_mode: dark,
        high_contrast: workspace.accessibilityDisplayShouldIncreaseContrast(),
        reduced_motion: workspace.accessibilityDisplayShouldReduceMotion(),
    }
}

fn violation(command: &Command, problem: &str) -> ! {
    panic!("appkit backend: protocol violation in {command:?}: {problem}")
}

impl AppKitBackend {
    pub fn new(mtm: MainThreadMarker, options: BackendOptions) -> AppKitBackend {
        AppKitBackend {
            state: Rc::new(RefCell::new(State {
                mtm,
                options,
                nodes: HashMap::new(),
                by_view: HashMap::new(),
                events: EventSink::default(),
                log: Vec::new(),
                pending_show: Vec::new(),
            })),
        }
    }

    pub fn handle(&self) -> AppKitHandle {
        AppKitHandle { state: self.state.clone() }
    }
}

impl AppKitHandle {
    pub fn command_log(&self) -> Vec<Command> {
        self.state.borrow().log.clone()
    }

    pub fn take_command_log(&self) -> Vec<Command> {
        std::mem::take(&mut self.state.borrow_mut().log)
    }

    /// Number of live native nodes.
    pub fn node_count(&self) -> usize {
        self.state.borrow().nodes.len()
    }

    /// Resizes a window's content like the user would; the window delegate
    /// reports it back as a `WindowResized` event.
    pub fn resize_window(&self, window: NodeId, size: Size) {
        let window = self.ns_window(window);
        if let Some(window) = window {
            window.setContentSize(NSSize::new(size.width as f64, size.height as f64));
        }
    }

    /// Orders front windows whose first layout has been applied.
    pub fn show_pending_windows(&self) {
        let pending = std::mem::take(&mut self.state.borrow_mut().pending_show);
        for id in pending {
            if let Some(window) = self.ns_window(id) {
                window.center();
                window.makeKeyAndOrderFront(None);
            }
        }
    }

    /// Escape hatch: the native window of a window node.
    pub fn ns_window(&self, id: NodeId) -> Option<Retained<NSWindow>> {
        match &self.state.borrow().nodes.get(&id)?.widget {
            Widget::Window { window, .. } => Some(window.clone()),
            _ => None,
        }
    }

    /// Escape hatch: the native view of any node (a window's content view).
    pub fn ns_view(&self, id: NodeId) -> Option<Retained<NSView>> {
        let state = self.state.borrow();
        let view = state.nodes.get(&id)?.widget.view();
        Some(view.retain())
    }
}

impl State {
    fn create(&mut self, id: NodeId, kind: WidgetKind, command: &Command) {
        let mtm = self.mtm;
        let target =
            matches!(kind, WidgetKind::Button | WidgetKind::Checkbox | WidgetKind::Switch | WidgetKind::TextInput)
                .then(|| ActionTarget::new(mtm, id, kind, self.events.clone()));
        let action = Some(sel!(fire:));
        let target_obj: Option<&AnyObject> = target.as_deref().map(|t| t.as_ref());
        let widget = match kind {
            WidgetKind::Window => {
                let style = NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable;
                let window = unsafe {
                    NSWindow::initWithContentRect_styleMask_backing_defer(
                        NSWindow::alloc(mtm),
                        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 300.0)),
                        style,
                        NSBackingStoreType::Buffered,
                        false,
                    )
                };
                unsafe { window.setReleasedWhenClosed(false) };
                // Code-built windows get no Tab order unless they ask for
                // one; this keeps it in step as views come and go.
                window.setAutorecalculatesKeyViewLoop(true);
                let host = HostView::new(mtm, true);
                window.setContentView(Some(&host));
                let delegate = WindowDelegate::new(mtm, id, self.events.clone());
                window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
                if self.options.force_light_appearance {
                    window.setAppearance(NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).as_deref());
                }
                if self.options.show_windows {
                    self.pending_show.push(id);
                }
                Widget::Window { window, host, _delegate: delegate }
            }
            WidgetKind::Container | WidgetKind::Custom(_) | WidgetKind::Native => {
                Widget::Host(HostView::new(mtm, false))
            }
            WidgetKind::Text => {
                let label = NSTextField::wrappingLabelWithString(&ns(""), mtm);
                label.setSelectable(false);
                Widget::Label(label)
            }
            WidgetKind::Button => {
                Widget::Button(unsafe { NSButton::buttonWithTitle_target_action(&ns(""), target_obj, action, mtm) })
            }
            WidgetKind::Checkbox => {
                Widget::Checkbox(unsafe { NSButton::checkboxWithTitle_target_action(&ns(""), target_obj, action, mtm) })
            }
            WidgetKind::Switch => {
                let switch = NSSwitch::new(mtm);
                unsafe {
                    switch.setTarget(target_obj);
                    switch.setAction(action);
                }
                Widget::Switch(switch)
            }
            WidgetKind::TextInput => {
                // No target-action: submit comes from the delegate (Return
                // only), edits from `controlTextDidChange:`.
                let field = NSTextField::textFieldWithString(&ns(""), mtm);
                if let Some(target) = &target {
                    // SAFETY: the node keeps the target alive as long as the field.
                    unsafe { field.setDelegate(Some(ProtocolObject::from_ref(&**target))) };
                }
                Widget::Field(field)
            }
            WidgetKind::Fragment => violation(command, "fragments are core-only"),
        };
        // The core assumes new nodes start with a zero frame and only sends
        // frames that differ; AppKit controls come with their own.
        if !matches!(widget, Widget::Window { .. }) {
            widget.view().setFrame(crate::classes::zero_rect());
        }
        self.by_view.insert(key(widget.view()), id);
        self.nodes.insert(id, Node { kind, widget, _target: target, parent: None, text_style: None, variant: None });
    }

    fn set_prop(&mut self, id: NodeId, prop: &Prop, command: &Command) {
        let Some(node) = self.nodes.get_mut(&id) else { violation(command, "node does not exist") };
        match (prop, &node.widget) {
            (Prop::Title(t), Widget::Window { window, .. }) => window.setTitle(&ns(t)),
            (Prop::Text(t), Widget::Label(l)) => l.setStringValue(&ns(t)),
            (Prop::Label(t), Widget::Button(b) | Widget::Checkbox(b)) => b.setTitle(&ns(t)),
            (Prop::Label(t), Widget::Switch(s)) => s.setAccessibilityLabel(Some(&ns(t))),
            (Prop::Value(t), Widget::Field(f)) => {
                // Don't disturb the caret when the field already shows it.
                if f.stringValue().to_string() != *t {
                    f.setStringValue(&ns(t));
                }
            }
            (Prop::Placeholder(t), Widget::Field(f)) => f.setPlaceholderString(Some(&ns(t))),
            (Prop::Checked(c), Widget::Checkbox(b)) => {
                b.setState(if *c { NSControlStateValueOn } else { NSControlStateValueOff })
            }
            (Prop::Checked(c), Widget::Switch(s)) => {
                s.setState(if *c { NSControlStateValueOn } else { NSControlStateValueOff })
            }
            (Prop::Enabled(e), w) if w.control().is_some() => w.control().unwrap().setEnabled(*e),
            (Prop::TextStyle(style), w) if w.control().is_some() => {
                w.control().unwrap().setFont(Some(&font(*style)));
                node.text_style = Some(*style);
            }
            (Prop::Variant(variant), Widget::Button(b)) => {
                b.setKeyEquivalent(&ns(if *variant == ButtonVariant::Primary { "\r" } else { "" }));
                b.setHasDestructiveAction(*variant == ButtonVariant::Destructive);
                b.setBordered(*variant != ButtonVariant::Plain);
                node.variant = Some(*variant);
            }
            _ => {}
        }
    }

    fn view(&self, id: NodeId, command: &Command) -> Retained<NSView> {
        match self.nodes.get(&id) {
            Some(node) => node.widget.view().retain(),
            None => violation(command, &format!("node {id} does not exist")),
        }
    }

    fn apply(&mut self, command: &Command) {
        match command {
            Command::Create { id, kind, props } => {
                if self.nodes.contains_key(id) {
                    violation(command, "node already exists");
                }
                self.create(*id, *kind, command);
                for prop in props {
                    self.set_prop(*id, prop, command);
                }
            }
            Command::SetProp { id, prop } => self.set_prop(*id, prop, command),
            Command::Insert { parent, child, index } => {
                let parent_view = self.view(*parent, command);
                let child_view = self.view(*child, command);
                if self.nodes[child].parent.is_some() {
                    violation(command, "child is still attached");
                }
                let siblings = parent_view.subviews();
                if *index >= siblings.len() {
                    parent_view.addSubview(&child_view);
                } else {
                    let before = siblings.objectAtIndex(*index);
                    parent_view.addSubview_positioned_relativeTo(
                        &child_view,
                        NSWindowOrderingMode::Below,
                        Some(&before),
                    );
                }
                self.nodes.get_mut(child).unwrap().parent = Some(*parent);
            }
            Command::Remove { parent, child } => {
                if self.nodes.get(child).and_then(|n| n.parent) != Some(*parent) {
                    violation(command, "not a child of this parent");
                }
                self.view(*child, command).removeFromSuperview();
                self.nodes.get_mut(child).unwrap().parent = None;
            }
            Command::Destroy { id } => {
                let Some(node) = self.nodes.remove(id) else { violation(command, "node does not exist") };
                self.by_view.remove(&key(node.widget.view()));
                self.pending_show.retain(|w| w != id);
                match &node.widget {
                    Widget::Window { window, .. } => {
                        window.setDelegate(None);
                        window.close();
                    }
                    widget => widget.view().removeFromSuperview(),
                }
            }
            Command::SetFrame { id, frame } => {
                let view = self.view(*id, command);
                view.setFrame(NSRect::new(
                    NSPoint::new(frame.x() as f64, frame.y() as f64),
                    NSSize::new(frame.width() as f64, frame.height() as f64),
                ));
            }
            Command::SetA11y { id, a11y } => {
                let view = self.view(*id, command);
                let A11yProps { label, description, hidden, .. } = a11y;
                view.setAccessibilityLabel(label.as_deref().map(ns).as_deref());
                view.setAccessibilityHelp(description.as_deref().map(ns).as_deref());
                if *hidden {
                    view.setAccessibilityElement(false);
                }
            }
            Command::SetWindowSize { id, size } => match self.nodes.get(id).map(|n| &n.widget) {
                Some(Widget::Window { window, .. }) => {
                    window.setContentSize(NSSize::new(size.width as f64, size.height as f64))
                }
                _ => violation(command, "not a window"),
            },
            Command::Focus { id } => {
                let view = self.view(*id, command);
                if let Some(window) = view.window() {
                    window.makeFirstResponder(Some(&view));
                }
            }
        }
    }
}

fn focused(widget: &Widget) -> bool {
    let view = widget.view();
    let Some(responder) = view.window().and_then(|w| w.firstResponder()) else { return false };
    match widget {
        // While editing, the window's field editor is first responder.
        Widget::Field(field) => field.currentEditor().is_some(),
        _ => std::ptr::eq(&*responder as *const _ as *const NSView, view as *const NSView),
    }
}

fn ceil_size(size: NSSize) -> Size {
    Size::new(size.width.ceil() as f32, size.height.ceil() as f32)
}

impl Backend for AppKitBackend {
    fn init(&mut self, events: EventSink) {
        self.state.borrow_mut().events = events;
    }

    fn metrics(&self) -> PlatformMetrics {
        metrics(self.state.borrow().mtm)
    }

    fn apply(&mut self, batch: &[Command]) {
        let mut state = self.state.borrow_mut();
        for command in batch {
            if state.options.record_commands {
                state.log.push(command.clone());
            }
            state.apply(command);
        }
    }

    fn measure(&mut self, id: NodeId, request: MeasureRequest) -> Size {
        let state = self.state.borrow();
        let Some(node) = state.nodes.get(&id) else { return Size::ZERO };
        let natural = match &node.widget {
            Widget::Label(label) => {
                // TODO: min-content (longest word) — until then text never
                // shrinks below its single-line width in flex rows.
                let width = request.known_width.map(f64::from).or(match request.available_width {
                    AvailableSpace::Definite(w) => Some(w as f64),
                    AvailableSpace::MinContent | AvailableSpace::MaxContent => None,
                });
                let bounds = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width.unwrap_or(1.0e7), 1.0e7));
                match label.cell() {
                    Some(cell) => ceil_size(cell.cellSizeForBounds(bounds)),
                    None => Size::ZERO,
                }
            }
            Widget::Field(field) => {
                let intrinsic = field.intrinsicContentSize();
                Size::new(
                    if intrinsic.width > 0.0 { intrinsic.width.ceil() as f32 } else { 200.0 },
                    intrinsic.height.ceil() as f32,
                )
            }
            Widget::Button(v) | Widget::Checkbox(v) => ceil_size(v.intrinsicContentSize()),
            Widget::Switch(v) => ceil_size(v.intrinsicContentSize()),
            Widget::Window { .. } | Widget::Host(_) => Size::ZERO,
        };
        Size::new(request.known_width.unwrap_or(natural.width), request.known_height.unwrap_or(natural.height))
    }

    fn perform(&mut self, id: NodeId, action: &A11yAction) -> Result<(), ActionError> {
        let (widget_view, control_enabled, kind, events) = {
            let state = self.state.borrow();
            let node = state.nodes.get(&id).ok_or(ActionError::UnknownNode)?;
            (node.widget.view().retain(), node.widget.control().map(|c| c.isEnabled()), node.kind, state.events.clone())
        };
        if control_enabled == Some(false) {
            return Err(ActionError::Disabled);
        }
        // No state borrow below: AppKit calls back into our targets.
        match (action, kind) {
            (A11yAction::Activate, WidgetKind::Button | WidgetKind::Checkbox | WidgetKind::Switch) => {
                // The press action assistive technology uses. Its return
                // value is unreliable for windows that aren't on screen (it
                // reports NO after pressing), so it is ignored.
                let _: bool = unsafe { msg_send![&*widget_view, accessibilityPerformPress] };
            }
            (A11yAction::SetValue(text), WidgetKind::TextInput) => {
                let field: &NSTextField = widget_view.downcast_ref().ok_or(ActionError::Unsupported)?;
                field.setStringValue(&ns(text));
                // Programmatic edits don't notify the delegate; assistive
                // technology edits are user edits, so report one.
                events.emit(id, UiEvent::Changed(EventValue::Text(text.clone())));
            }
            (A11yAction::Focus, _) => {
                let window = widget_view.window().ok_or(ActionError::Unsupported)?;
                if !window.makeFirstResponder(Some(&widget_view)) {
                    return Err(ActionError::Unsupported);
                }
                events.emit(id, UiEvent::FocusIn);
            }
            (A11yAction::ScrollIntoView, _) => {}
            _ => return Err(ActionError::Unsupported),
        }
        Ok(())
    }

    fn synthesize(&mut self, id: NodeId, input: &SyntheticInput) -> Result<(), ActionError> {
        let SyntheticInput::Key(key) = input;
        let (view, kind) = {
            let state = self.state.borrow();
            let node = state.nodes.get(&id).ok_or(ActionError::UnknownNode)?;
            if node.widget.control().is_some_and(|c| !c.isEnabled()) {
                return Err(ActionError::Disabled);
            }
            (node.widget.view().retain(), node.kind)
        };
        match (kind, key) {
            (WidgetKind::TextInput, Key::Char(_) | Key::Backspace | Key::Enter | Key::Tab) => {
                // Drive the field editor, the object that receives real
                // keystrokes: text goes through `insertText:`, and editing
                // keys through `doCommandBySelector:`, which is what
                // `interpretKeyEvents:` does, delegate hooks included.
                let window = view.window().ok_or(ActionError::Unsupported)?;
                let field: &NSTextField = view.downcast_ref().ok_or(ActionError::Unsupported)?;
                let just_focused = field.currentEditor().is_none();
                if just_focused {
                    window.makeFirstResponder(Some(&view));
                }
                let editor = field.currentEditor().ok_or(ActionError::Unsupported)?;
                if just_focused {
                    // Focusing selects everything; typing should append, as
                    // after clicking past the end of the text.
                    let end = editor.string().length();
                    editor.setSelectedRange(NSRange::new(end, 0));
                }
                let command = match key {
                    Key::Char(c) => {
                        let text = ns(&c.to_string());
                        let _: () = unsafe { msg_send![&*editor, insertText: &*text] };
                        return Ok(());
                    }
                    Key::Backspace => sel!(deleteBackward:),
                    Key::Enter => sel!(insertNewline:),
                    _ => sel!(insertTab:),
                };
                unsafe { editor.doCommandBySelector(command) };
                Ok(())
            }
            (WidgetKind::Button, Key::Enter | Key::Char(' '))
            | (WidgetKind::Checkbox | WidgetKind::Switch, Key::Char(' ')) => self.perform(id, &A11yAction::Activate),
            _ => Err(ActionError::Unsupported),
        }
    }

    fn native_state(&self, id: NodeId) -> Option<NativeState> {
        let state = self.state.borrow();
        let node = state.nodes.get(&id)?;
        let mut props = Vec::new();
        let checked = |s: isize| Prop::Checked(s == NSControlStateValueOn);
        match &node.widget {
            Widget::Window { window, .. } => props.push(Prop::Title(window.title().to_string())),
            Widget::Label(l) => props.push(Prop::Text(l.stringValue().to_string())),
            Widget::Field(f) => {
                props.push(Prop::Value(f.stringValue().to_string()));
                if let Some(p) = f.placeholderString() {
                    props.push(Prop::Placeholder(p.to_string()));
                }
            }
            Widget::Button(b) => props.push(Prop::Label(b.title().to_string())),
            Widget::Checkbox(b) => {
                props.push(Prop::Label(b.title().to_string()));
                props.push(checked(b.state()));
            }
            Widget::Switch(s) => {
                if let Some(label) = s.accessibilityLabel() {
                    props.push(Prop::Label(label.to_string()));
                }
                props.push(checked(s.state()));
            }
            Widget::Host(_) => {}
        }
        if let Some(control) = node.widget.control() {
            props.push(Prop::Enabled(control.isEnabled()));
        }
        props.extend(node.text_style.map(Prop::TextStyle));
        props.extend(node.variant.map(Prop::Variant));
        let view = node.widget.view();
        let f = view.frame();
        let children = view.subviews().iter().filter_map(|v| state.by_view.get(&key(&v)).copied()).collect();
        Some(NativeState {
            kind: node.kind,
            props,
            frame: Rect::new(f.origin.x as f32, f.origin.y as f32, f.size.width as f32, f.size.height as f32),
            parent: node.parent,
            children,
            focused: focused(&node.widget),
        })
    }

    fn capture(&mut self, id: NodeId) -> Result<Image, CaptureError> {
        let view = {
            let state = self.state.borrow();
            state.nodes.get(&id).ok_or(CaptureError::UnknownNode)?.widget.view().retain()
        };
        let bounds = view.bounds();
        let rep = view
            .bitmapImageRepForCachingDisplayInRect(bounds)
            .ok_or_else(|| CaptureError::Failed("no bitmap for view".into()))?;
        view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
        let (width, height) = (rep.pixelsWide() as usize, rep.pixelsHigh() as usize);
        let (samples, bits, row) = (rep.samplesPerPixel() as usize, rep.bitsPerSample(), rep.bytesPerRow() as usize);
        if bits != 8 || !(samples == 3 || samples == 4) {
            return Err(CaptureError::Failed(format!("unsupported bitmap: {samples} samples × {bits} bits")));
        }
        let alpha_first = rep.bitmapFormat().contains(NSBitmapFormat::AlphaFirst);
        let data = rep.bitmapData();
        if data.is_null() {
            return Err(CaptureError::Failed("bitmap has no data".into()));
        }
        let bytes = unsafe { std::slice::from_raw_parts(data, row * height) };
        let mut rgba = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let p = &bytes[y * row + x * samples..][..samples];
                let [r, g, b, a] = match (samples, alpha_first) {
                    (4, true) => [p[1], p[2], p[3], p[0]],
                    (4, false) => [p[0], p[1], p[2], p[3]],
                    _ => [p[0], p[1], p[2], 255],
                };
                rgba.extend_from_slice(&[r, g, b, a]);
            }
        }
        let scale = if bounds.size.width > 0.0 { width as f32 / bounds.size.width as f32 } else { 1.0 };
        Ok(Image { width: width as u32, height: height as u32, scale_factor: scale, rgba })
    }
}
