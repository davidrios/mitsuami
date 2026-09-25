use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::{EffectFn, Kind, NodeKey, with_runtime};

/// Handle to a running effect. Dropping it does nothing; the effect lives
/// until its owner is disposed or [`Effect::dispose`] is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    key: NodeKey,
}

/// Runs `f` now, and again whenever any reactive value it read changes.
/// Vue's `watchEffect`, but synchronous.
///
/// Reactive nodes and cleanups created inside `f` are owned by the effect and
/// disposed before each re-run.
pub fn effect(f: impl FnMut() + 'static) -> Effect {
    let f: EffectFn = Rc::new(RefCell::new(f));
    let key = with_runtime(|rt| {
        let key = rt.create(Kind::Effect(f), None);
        rt.update_if_necessary(key);
        key
    });
    Effect { key }
}

/// Calls `callback(new, old)` whenever the value returned by `source` changes.
/// Unlike [`effect`], the callback does not run for the initial value, and
/// its reads are not tracked.
pub fn watch<T: Clone + PartialEq + 'static>(
    source: impl Fn() -> T + 'static,
    mut callback: impl FnMut(&T, &T) + 'static,
) -> Effect {
    let mut previous: Option<T> = None;
    effect(move || {
        let value = source();
        crate::untrack(|| {
            if let Some(old) = &previous
                && *old != value
            {
                callback(&value, old);
            }
        });
        previous = Some(value);
    })
}

impl Effect {
    /// Stops the effect and disposes everything it owns.
    pub fn dispose(self) {
        with_runtime(|rt| rt.dispose(self.key));
    }
}
