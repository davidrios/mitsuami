//! A lock that guards changes, the GTK widget: `GtkLockButton` (`linux.rs`),
//! the padlock GNOME Settings panels put in their header bars. Elsewhere
//! it's composed from a built-in button.
//!
//! It's controlled like every custom widget: a click asks to unlock or to
//! lock, and the app decides (a password prompt, a policy check) and answers
//! with new props.

use mitsuami::prelude::*;

#[cfg(target_os = "linux")]
mod linux;

pub struct Lock;

#[derive(Clone, Debug, PartialEq)]
pub struct LockProps {
    pub locked: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LockEvent {
    UnlockRequested,
    LockRequested,
}

impl LockEvent {
    fn toggle(locked: bool) -> LockEvent {
        if locked { LockEvent::UnlockRequested } else { LockEvent::LockRequested }
    }
}

impl CustomWidget for Lock {
    const NAME: &'static str = "Lock";
    type Props = LockProps;
    type Event = LockEvent;

    fn a11y(props: &LockProps) -> A11yProps {
        A11yProps::new(Role::Button).value(if props.locked { "Locked" } else { "Unlocked" })
    }

    fn action(props: &LockProps, action: &A11yAction) -> Option<LockEvent> {
        (*action == A11yAction::Activate).then(|| LockEvent::toggle(props.locked))
    }
}

/// The stand-in where the platform has no lock button: a plain button.
fn composed(widget: Composed<Lock>) -> impl View {
    let label = widget.label().unwrap_or_default();
    let text = widget.clone();
    Button::new(move || if text.props().locked { "Unlock" } else { "Lock" }.to_string())
        .a11y_label(label)
        .on_click(move || widget.emit(LockEvent::toggle(widget.props().locked)))
}

impl Render for Lock {
    fn renderer() -> Renderer<Self> {
        platform! {
            linux => mitsuami::gtk::native::<Self>().with_composed(composed),
            _ => Renderer::composed(composed),
        }
    }
}
