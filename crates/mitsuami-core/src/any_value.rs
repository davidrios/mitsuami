//! Type-erased values that can travel through commands and events.
//!
//! Commands stay comparable and printable (for logs, snapshots and the
//! mirror check) even when they carry app-defined data: [`AnyValue`] keeps
//! the value's own `PartialEq` and `Debug`, and [`Opaque`] is for payloads
//! that have neither, like a native view factory.

use std::any::Any;
use std::fmt;
use std::rc::Rc;

trait DynValue: Any {
    fn dyn_eq(&self, other: &dyn DynValue) -> bool;
    fn dyn_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    fn as_any(&self) -> &dyn Any;
}

impl<T: Any + PartialEq + fmt::Debug> DynValue for T {
    fn dyn_eq(&self, other: &dyn DynValue) -> bool {
        other.as_any().downcast_ref::<T>() == Some(self)
    }

    fn dyn_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Any comparable, printable value: custom widget props and events.
#[derive(Clone)]
pub struct AnyValue(Rc<dyn DynValue>);

impl AnyValue {
    pub fn new<T: Any + PartialEq + fmt::Debug>(value: T) -> AnyValue {
        AnyValue(Rc::new(value))
    }

    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.as_any().downcast_ref()
    }

    pub fn is<T: Any>(&self) -> bool {
        self.0.as_any().is::<T>()
    }
}

impl PartialEq for AnyValue {
    fn eq(&self, other: &AnyValue) -> bool {
        self.0.dyn_eq(&*other.0)
    }
}

impl fmt::Debug for AnyValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.dyn_fmt(f)
    }
}

/// A platform-specific payload the core passes through without looking
/// inside: a native render, a native view factory. Equal only to itself;
/// printed as its label.
#[derive(Clone)]
pub struct Opaque {
    label: &'static str,
    value: Rc<dyn Any>,
}

impl Opaque {
    pub fn new<T: Any>(label: &'static str, value: T) -> Opaque {
        Opaque { label, value: Rc::new(value) }
    }

    pub fn label(&self) -> &'static str {
        self.label
    }

    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.value.downcast_ref()
    }
}

impl PartialEq for Opaque {
    fn eq(&self, other: &Opaque) -> bool {
        Rc::ptr_eq(&self.value, &other.value)
    }
}

impl fmt::Debug for Opaque {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}>", self.label)
    }
}
