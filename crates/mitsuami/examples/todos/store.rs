//! The todo list: app-wide state and the actions on it, as a store.

use std::collections::HashSet;

use mitsuami::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub struct Todo {
    pub id: u32,
    pub title: String,
}

#[derive(Clone, Copy)]
pub struct Todos {
    pub items: Signal<Vec<Todo>>,
    done: Signal<HashSet<u32>>,
    pub hide_done: Signal<bool>,
    next_id: Signal<u32>,
}

impl Store for Todos {
    fn create() -> Todos {
        let todos = Todos {
            items: signal(Vec::new()),
            done: signal(HashSet::new()),
            hide_done: signal(false),
            next_id: signal(1),
        };
        for title in ["Try view!", "Write a component", "Share a store"] {
            todos.add(title);
        }
        todos
    }
}

impl Todos {
    /// Adds a todo; blank titles are ignored.
    pub fn add(&self, title: &str) {
        let title = title.trim();
        if title.is_empty() {
            return;
        }
        let id = self.next_id.get_untracked();
        self.next_id.set(id + 1);
        self.items.update(|items| items.push(Todo { id, title: title.to_string() }));
    }

    pub fn remove(&self, id: u32) {
        batch(|| {
            self.items.update(|items| items.retain(|t| t.id != id));
            self.done.update(|done| {
                done.remove(&id);
            });
        });
    }

    pub fn is_done(&self, id: u32) -> bool {
        self.done.with(|done| done.contains(&id))
    }

    pub fn set_done(&self, id: u32, done: bool) {
        self.done.update(|set| {
            if done {
                set.insert(id);
            } else {
                set.remove(&id);
            }
        });
    }

    /// The todos to list: all of them, or the open ones.
    pub fn visible(&self) -> Vec<Todo> {
        let hide_done = self.hide_done.get();
        self.items.with(|items| items.iter().filter(|t| !(hide_done && self.is_done(t.id))).cloned().collect())
    }

    pub fn left(&self) -> usize {
        self.items.with(|items| items.iter().filter(|t| !self.is_done(t.id)).count())
    }
}
