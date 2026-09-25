//! Control flow: conditional and list rendering.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use mitsuami_reactive::{IntoValue, Owner, Value, computed, effect, on_cleanup, untrack};

use crate::ui::Ui;
use crate::view::{AnyView, View};
use crate::widget::{NodeId, WidgetKind};

type Branch = Rc<dyn Fn() -> AnyView>;

/// Renders `then` while `when` is true, otherwise the optional fallback.
/// Vue's `v-if` / `v-else`. Switching branches disposes the old branch's
/// nodes and reactive state.
pub struct Show {
    when: Value<bool>,
    then: Branch,
    fallback: Option<Branch>,
}

impl Show {
    pub fn new<V: View>(when: impl IntoValue<bool>, then: impl Fn() -> V + 'static) -> Show {
        Show { when: when.into_value(), then: Rc::new(move || AnyView::new(then())), fallback: None }
    }

    pub fn fallback<V: View>(mut self, fallback: impl Fn() -> V + 'static) -> Show {
        self.fallback = Some(Rc::new(move || AnyView::new(fallback())));
        self
    }
}

impl View for Show {
    fn build(self, ui: &Ui) -> NodeId {
        let fragment = ui.create(WidgetKind::Fragment, Vec::new());
        let Show { when, then, fallback } = self;
        let ui = ui.clone();
        let render = move |condition: bool| {
            let branch = if condition { Some(&then) } else { fallback.as_ref() };
            if let Some(branch) = branch {
                let child = branch().build(&ui);
                ui.append_child(fragment, child);
                let ui = ui.clone();
                on_cleanup(move || ui.destroy(child));
            }
        };
        match when {
            Value::Static(condition) => render(condition),
            Value::Dynamic(f) => {
                let condition = computed(move || f());
                effect(move || {
                    let c = condition.get();
                    untrack(|| render(c));
                });
            }
        }
        fragment
    }
}

type KeyFn<T, K> = Rc<dyn Fn(&T) -> K>;
type RenderFn<T> = Rc<dyn Fn(T) -> AnyView>;

/// Keyed list rendering. Vue's `v-for` with `:key`.
///
/// Items are matched by key between updates: existing rows keep their nodes
/// and state (and are moved if needed), new keys render new rows, and rows
/// whose key disappeared are disposed. A row is rendered once per key; put
/// signals inside items for per-row updates.
pub struct For<T: 'static, K: 'static> {
    each: Value<Vec<T>>,
    key: KeyFn<T, K>,
    render: RenderFn<T>,
}

impl<T: Clone + 'static, K: Eq + Hash + 'static> For<T, K> {
    pub fn new<V: View>(
        each: impl IntoValue<Vec<T>>,
        key: impl Fn(&T) -> K + 'static,
        render: impl Fn(T) -> V + 'static,
    ) -> For<T, K> {
        For { each: each.into_value(), key: Rc::new(key), render: Rc::new(move |item| AnyView::new(render(item))) }
    }
}

impl<T: Clone + 'static, K: Eq + Hash + 'static> View for For<T, K> {
    fn build(self, ui: &Ui) -> NodeId {
        let fragment = ui.create(WidgetKind::Fragment, Vec::new());
        // Row scopes hang off a dedicated scope in the *building* owner, so
        // they survive re-runs of the list effect.
        let rows_scope = Owner::current().map(|o| o.child()).unwrap_or_else(Owner::new_root);
        let rows: Rc<RefCell<Vec<(K, NodeId, Owner)>>> = Rc::default();
        let For { each, key, render } = self;
        let ui = ui.clone();
        effect(move || {
            let items = each.get();
            untrack(|| {
                let mut old: HashMap<K, (NodeId, Owner)> =
                    rows.borrow_mut().drain(..).map(|(k, id, owner)| (k, (id, owner))).collect();
                let mut next = Vec::with_capacity(items.len());
                for item in items {
                    let k = key(&item);
                    let (id, owner) = match old.remove(&k) {
                        Some(existing) => existing,
                        None => {
                            let owner = rows_scope.child();
                            let id = owner.with(|| render(item).build(&ui));
                            (id, owner)
                        }
                    };
                    next.push((k, id, owner));
                }
                for (_, (id, owner)) in old {
                    owner.dispose();
                    ui.destroy(id);
                }
                ui.set_children(fragment, next.iter().map(|(_, id, _)| *id).collect());
                *rows.borrow_mut() = next;
            });
        });
        fragment
    }
}
