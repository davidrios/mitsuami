//! Async state as signals: [`resource`] for loading data, [`action`] for
//! running operations, each with its pending state.
//!
//! ```ignore
//! let user = resource_on(move || user_id.get(), |id| async move { api::user(id).await });
//! view! {
//!     <Show when=move || user.loading()><Text>"Loading…"</Text></Show>
//!     <Text>{move || user.with_data(|u| u.map(|u| u.name.clone()).unwrap_or_default())}</Text>
//! }
//!
//! let save = action(|draft: Draft| async move { api::save(draft).await });
//! view! { <Button enabled=move || !save.pending() @click=move || save.dispatch(draft())>"Save"</Button> }
//! ```
//!
//! Both run their futures on the UI thread, owned by the scope that created
//! them: disposing it (closing the window, hiding the component) cancels
//! what's in flight. Put blocking work in `spawn_blocking`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use mitsuami_reactive::{Owner, Signal, batch, effect, on_cleanup, signal, untrack};

use crate::task::{TaskHandle, current_ui, spawn_local};

/// Data loaded asynchronously: [`resource`] or [`resource_on`].
///
/// It fetches when created, again whenever its source changes, and on
/// [`refetch`](Resource::refetch). A new fetch cancels the one in flight.
/// While it loads, the previous data stays available; an error keeps it
/// too, so a view can show both.
pub struct Resource<T: 'static, E: 'static> {
    data: Signal<Option<T>>,
    error: Signal<Option<E>>,
    loading: Signal<bool>,
    refetches: Signal<u64>,
}

impl<T: 'static, E: 'static> Clone for Resource<T, E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static, E: 'static> Copy for Resource<T, E> {}

/// Loads `fetch()` now; again on [`Resource::refetch`].
pub fn resource<T, E, Fut>(fetch: impl Fn() -> Fut + 'static) -> Resource<T, E>
where
    Fut: Future<Output = Result<T, E>> + 'static,
{
    resource_on(|| (), move |()| fetch())
}

/// Loads `fetch(source())` now, and again whenever the signals `source`
/// reads change. Vue's `watch` plus a fetch; the future's own reads aren't
/// tracked.
pub fn resource_on<S, T, E, Fut>(source: impl Fn() -> S + 'static, fetch: impl Fn(S) -> Fut + 'static) -> Resource<T, E>
where
    S: 'static,
    Fut: Future<Output = Result<T, E>> + 'static,
{
    let resource = Resource { data: signal(None), error: signal(None), loading: signal(false), refetches: signal(0) };
    let Resource { data, error, loading, refetches } = resource;
    effect(move || {
        refetches.with(|_| ());
        let source = source();
        untrack(|| {
            if !loading.get_untracked() {
                loading.set(true);
            }
            let future = fetch(source);
            // Owned by this run of the effect: the next run cancels it.
            spawn_local(async move {
                let result = future.await;
                batch(|| {
                    match result {
                        Ok(value) => {
                            data.set(Some(value));
                            if error.with_untracked(Option::is_some) {
                                error.set(None);
                            }
                        }
                        Err(e) => error.set(Some(e)),
                    }
                    loading.set(false);
                });
            });
        });
    });
    resource
}

impl<T: 'static, E: 'static> Resource<T, E> {
    /// Whether a fetch is in flight.
    pub fn loading(&self) -> bool {
        self.loading.get()
    }

    /// The last data loaded, if any.
    pub fn data(&self) -> Option<T>
    where
        T: Clone,
    {
        self.data.get()
    }

    /// Borrows the last data loaded.
    pub fn with_data<R>(&self, f: impl FnOnce(Option<&T>) -> R) -> R {
        self.data.with(|data| f(data.as_ref()))
    }

    /// The error of the last fetch, if it failed.
    pub fn error(&self) -> Option<E>
    where
        E: Clone,
    {
        self.error.get()
    }

    /// Borrows the error of the last fetch, if it failed.
    pub fn with_error<R>(&self, f: impl FnOnce(Option<&E>) -> R) -> R {
        self.error.with(|error| f(error.as_ref()))
    }

    /// Fetches again with the current source.
    pub fn refetch(&self) {
        self.refetches.update(|n| *n += 1);
    }

    /// Replaces the data locally, e.g. optimistically after a change the
    /// server will confirm on the next fetch.
    pub fn set_data(&self, value: T) {
        self.data.set(Some(value));
    }
}

type RunFn<I, O> = Rc<dyn Fn(I) -> Pin<Box<dyn Future<Output = O>>>>;

struct ActionState<I, O> {
    run: RunFn<I, O>,
    owner: Option<Owner>,
    /// In flight, by dispatch number; cancelled with the owner.
    tasks: RefCell<HashMap<u64, TaskHandle>>,
    dispatches: Cell<u64>,
}

/// An async operation and its state: [`action`].
pub struct Action<I: 'static, O: 'static> {
    state: Signal<Rc<ActionState<I, O>>>,
    pending: Signal<usize>,
    value: Signal<Option<O>>,
}

impl<I: 'static, O: 'static> Clone for Action<I, O> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<I: 'static, O: 'static> Copy for Action<I, O> {}

/// Wraps `run` so each [`dispatch`](Action::dispatch) runs it as a task,
/// tracking whether any is [`pending`](Action::pending) and keeping the
/// [`value`](Action::value) of the latest one.
pub fn action<I, O, Fut>(run: impl Fn(I) -> Fut + 'static) -> Action<I, O>
where
    Fut: Future<Output = O> + 'static,
{
    let state = Rc::new(ActionState {
        run: Rc::new(move |input| Box::pin(run(input)) as Pin<Box<dyn Future<Output = O>>>),
        owner: Owner::current(),
        tasks: RefCell::default(),
        dispatches: Cell::new(0),
    });
    let in_flight = state.clone();
    on_cleanup(move || {
        for (_, task) in in_flight.tasks.take() {
            task.cancel();
        }
    });
    Action { state: signal(state), pending: signal(0), value: signal(None) }
}

impl<I: 'static, O: 'static> Action<I, O> {
    /// Runs the action with `input`. Dispatches may overlap; the value is
    /// the result of the latest one.
    pub fn dispatch(&self, input: I) {
        let state = self.state.get_untracked();
        let future = (state.run)(input);
        let number = state.dispatches.get() + 1;
        state.dispatches.set(number);
        let Action { pending, value, .. } = *self;
        pending.update(|n| *n += 1);
        let finished = state.clone();
        let task = current_ui().spawn_in(
            async move {
                let output = future.await;
                finished.tasks.borrow_mut().remove(&number);
                batch(|| {
                    pending.update(|n| *n -= 1);
                    if finished.dispatches.get() == number {
                        value.set(Some(output));
                    }
                });
            },
            state.owner,
        );
        state.tasks.borrow_mut().insert(number, task);
    }

    /// Whether any dispatch is still running.
    pub fn pending(&self) -> bool {
        self.pending.get() > 0
    }

    /// The result of the latest dispatch, once it finished.
    pub fn value(&self) -> Option<O>
    where
        O: Clone,
    {
        self.value.get()
    }

    /// Borrows the result of the latest dispatch.
    pub fn with_value<R>(&self, f: impl FnOnce(Option<&O>) -> R) -> R {
        self.value.with(|value| f(value.as_ref()))
    }
}
