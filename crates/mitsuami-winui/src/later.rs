//! Parking values for WinRT completion handlers.
//!
//! `IAsyncOperation::when` wants a `Send` closure, but replies and XAML
//! objects aren't `Send`. XAML completes its operations on the UI thread, so
//! the handler carries a ticket and the values wait here in a thread-local.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

thread_local! {
    static PARKED: RefCell<HashMap<u64, Box<dyn Any>>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u64> = const { Cell::new(0) };
}

/// Parks `value` until [`take`] is called with the returned ticket.
pub(crate) fn park<T: 'static>(value: T) -> u64 {
    let ticket = NEXT.get();
    NEXT.set(ticket + 1);
    PARKED.with(|p| p.borrow_mut().insert(ticket, Box::new(value)));
    ticket
}

/// Takes back a parked value. `None` if it was taken already.
pub(crate) fn take<T: 'static>(ticket: u64) -> Option<T> {
    let value = PARKED.with(|p| p.borrow_mut().remove(&ticket))?;
    value.downcast().ok().map(|b| *b)
}

/// Runs `f` on the thread `queue` belongs to (the UI thread), soon. For
/// completions of non-XAML operations (file pickers, the clipboard), which
/// arrive on worker threads.
pub(crate) fn on_ui(queue: &crate::bindings::DispatcherQueue, f: impl FnOnce() + Send + 'static) {
    use windows_core::Interface;
    let f = std::sync::Mutex::new(Some(f));
    let handler = crate::bindings::DispatcherQueueHandler::new(move || {
        if let Some(f) = f.lock().ok().and_then(|mut f| f.take()) {
            f();
        }
    });
    _ = queue.cast::<crate::bindings::IDispatcherQueue>().and_then(|q| q.TryEnqueue(&handler));
}
