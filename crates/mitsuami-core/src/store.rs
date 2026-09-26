//! Stores: app-wide state and the actions on it, Pinia-style.
//!
//! A store is a plain struct of signals (and resources, actions) with
//! methods. [`use_store`] returns the app's one instance, created on first
//! use, and views share it:
//!
//! ```ignore
//! #[derive(Clone, Copy)]
//! struct Cart { items: Signal<Vec<Item>> }
//!
//! impl Store for Cart {
//!     fn create() -> Cart { Cart { items: signal(Vec::new()) } }
//! }
//!
//! let cart = use_store::<Cart>();
//! ```
//!
//! A store `provide`d in a scope takes precedence there, which is how tests
//! (and previews) swap in a store in a known state.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mitsuami_reactive::{Owner, inject, provide};

/// App-wide state, created once per app by [`use_store`].
pub trait Store: Clone + 'static {
    /// Creates the store. Runs once, in a scope that lives as long as the
    /// app, so the store's effects, resources and tasks do too.
    fn create() -> Self;
}

#[derive(Clone)]
struct Registry {
    stores: Rc<RefCell<HashMap<TypeId, Rc<dyn Any>>>>,
    /// The app scope; each store gets a child of it.
    owner: Owner,
}

/// Makes [`use_store`] work in the current scope and below: the stores
/// live as long as it. `App` and `TestApp` call it in their app scope.
///
/// # Panics
/// When called outside of any scope.
pub fn provide_stores() {
    let owner = Owner::current().expect("mitsuami: provide_stores needs a scope (Owner::with)");
    provide(Registry { stores: Rc::default(), owner });
}

/// The app's instance of store `S`, created on first use; or the one
/// `provide`d nearest to this scope, if any.
///
/// # Panics
/// Outside of an app (no [`provide_stores`] above this scope).
pub fn use_store<S: Store>() -> S {
    if let Some(store) = inject::<S>() {
        return store;
    }
    let registry = inject::<Registry>().unwrap_or_else(|| {
        panic!(
            "mitsuami: use_store::<{}>() outside of an app; mount the view with App or TestApp, \
             or call provide_stores() in an enclosing scope",
            std::any::type_name::<S>()
        )
    });
    let existing = registry.stores.borrow().get(&TypeId::of::<S>()).cloned();
    if let Some(store) = existing {
        return store.downcast_ref::<S>().expect("mitsuami: store registry holds the wrong type").clone();
    }
    // Not borrowed while creating: a store may use other stores.
    let store = registry.owner.child().with(S::create);
    registry.stores.borrow_mut().insert(TypeId::of::<S>(), Rc::new(store.clone()));
    store
}
