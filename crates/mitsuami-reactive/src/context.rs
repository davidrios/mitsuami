use std::any::TypeId;
use std::rc::Rc;

use crate::runtime::with_runtime;

/// Makes `value` available to [`inject`] calls in this owner and everything
/// it owns. Providing the same type again in the same owner replaces it.
///
/// # Panics
/// When called outside of any owner.
pub fn provide<T: 'static>(value: T) {
    with_runtime(|rt| rt.provide(Rc::new(value), TypeId::of::<T>()));
}

/// Returns the nearest value of type `T` provided by the current owner or
/// one of its ancestors.
pub fn inject<T: Clone + 'static>() -> Option<T> {
    with_runtime(|rt| rt.inject(TypeId::of::<T>())).and_then(|value| value.downcast_ref::<T>().cloned())
}
