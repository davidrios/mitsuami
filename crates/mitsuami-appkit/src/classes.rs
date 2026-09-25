//! Objective-C classes bridging AppKit callbacks into mitsuami events.

use std::cell::Cell;

use mitsuami_core::{EventSink, EventValue, NodeId, Size, UiEvent, WidgetKind};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSButton, NSColor, NSControl, NSControlStateValueOn, NSControlTextEditingDelegate, NSRectFill, NSSwitch,
    NSTextField, NSTextFieldDelegate, NSTextView, NSView, NSWindow, NSWindowDelegate,
};
use objc2_foundation::{NSNotification, NSPoint, NSRect, NSSize};

pub(crate) fn zero_rect() -> NSRect {
    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0))
}

pub(crate) struct HostIvars {
    /// Paint the window background (window content views only), so
    /// offscreen captures look like the real window.
    fill: Cell<bool>,
}

define_class!(
    /// A layout host: a flipped view (origin top-left, like the core) that
    /// never lays out its children itself.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = HostIvars]
    pub(crate) struct HostView;

    impl HostView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty: NSRect) {
            if self.ivars().fill.get() {
                NSColor::windowBackgroundColor().setFill();
                NSRectFill(dirty);
            }
        }
    }
);

impl HostView {
    pub(crate) fn new(mtm: MainThreadMarker, fill: bool) -> Retained<HostView> {
        let this = HostView::alloc(mtm).set_ivars(HostIvars { fill: Cell::new(fill) });
        unsafe { msg_send![super(this), initWithFrame: zero_rect()] }
    }
}

pub(crate) struct TargetIvars {
    id: NodeId,
    kind: WidgetKind,
    events: EventSink,
}

define_class!(
    /// Target of control actions and delegate of text fields for one node.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TargetIvars]
    pub(crate) struct ActionTarget;

    impl ActionTarget {
        #[unsafe(method(fire:))]
        fn fire(&self, sender: &AnyObject) {
            let TargetIvars { id, kind, events } = self.ivars();
            let event = match kind {
                WidgetKind::Button => UiEvent::Click,
                WidgetKind::Checkbox => match sender.downcast_ref::<NSButton>() {
                    Some(b) => UiEvent::Changed(EventValue::Bool(b.state() == NSControlStateValueOn)),
                    None => return,
                },
                WidgetKind::Switch => match sender.downcast_ref::<NSSwitch>() {
                    Some(s) => UiEvent::Changed(EventValue::Bool(s.state() == NSControlStateValueOn)),
                    None => return,
                },
                _ => return,
            };
            events.emit(*id, event);
        }
    }

    unsafe impl NSObjectProtocol for ActionTarget {}

    unsafe impl NSControlTextEditingDelegate for ActionTarget {
        /// Return / Enter submits. Deliberately not the field's action: that
        /// also fires when editing ends by Tab or a click elsewhere.
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn control_text_view_do_command_by_selector(
            &self,
            _control: &NSControl,
            _text_view: &NSTextView,
            command: Sel,
        ) -> bool {
            if command == sel!(insertNewline:) {
                self.ivars().events.emit(self.ivars().id, UiEvent::Submit);
            }
            // Not handled: AppKit carries on with its default behaviour.
            false
        }

        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, notification: &NSNotification) {
            let Some(object) = notification.object() else { return };
            if let Some(field) = object.downcast_ref::<NSTextField>() {
                let text = field.stringValue().to_string();
                self.ivars().events.emit(self.ivars().id, UiEvent::Changed(EventValue::Text(text)));
            }
        }
    }

    unsafe impl NSTextFieldDelegate for ActionTarget {}
);

impl ActionTarget {
    pub(crate) fn new(
        mtm: MainThreadMarker,
        id: NodeId,
        kind: WidgetKind,
        events: EventSink,
    ) -> Retained<ActionTarget> {
        let this = ActionTarget::alloc(mtm).set_ivars(TargetIvars { id, kind, events });
        unsafe { msg_send![super(this), init] }
    }
}

pub(crate) struct WindowIvars {
    id: NodeId,
    events: EventSink,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = WindowIvars]
    pub(crate) struct WindowDelegate;

    unsafe impl NSObjectProtocol for WindowDelegate {}

    unsafe impl NSWindowDelegate for WindowDelegate {
        /// The app decides whether a window closes (e.g. to ask about
        /// unsaved changes); the core destroys it if so.
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            self.ivars().events.emit(self.ivars().id, UiEvent::WindowCloseRequested);
            false
        }

        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, notification: &NSNotification) {
            let Some(object) = notification.object() else { return };
            let Some(window) = object.downcast_ref::<NSWindow>() else { return };
            let content = window.contentRectForFrameRect(window.frame()).size;
            let size = Size::new(content.width as f32, content.height as f32);
            self.ivars().events.emit(self.ivars().id, UiEvent::WindowResized(size));
        }

        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn window_did_change_backing_properties(&self, _notification: &NSNotification) {
            self.ivars().events.emit(self.ivars().id, UiEvent::MetricsChanged);
        }
    }
);

impl WindowDelegate {
    pub(crate) fn new(mtm: MainThreadMarker, id: NodeId, events: EventSink) -> Retained<WindowDelegate> {
        let this = WindowDelegate::alloc(mtm).set_ivars(WindowIvars { id, events });
        unsafe { msg_send![super(this), init] }
    }
}
