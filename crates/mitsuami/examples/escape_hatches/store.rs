//! The app logic behind the screen: a store, as plain signals and actions.
//! The screen `inject`s it; tests can `provide` their own.

use mitsuami::prelude::*;

pub const MAX_STARS: u8 = 5;

#[derive(Clone, Copy)]
pub struct Review {
    pub stars: Signal<u8>,
    /// Changes are locked until the user unlocks them.
    pub locked: Signal<bool>,
    submitted: Signal<Option<String>>,
}

impl Review {
    pub fn new() -> Review {
        Review { stars: signal(0), locked: signal(true), submitted: signal(None) }
    }

    /// A real app would check here: ask for a password, a policy.
    pub fn unlock(&self) {
        self.locked.set(false);
    }

    pub fn lock(&self) {
        self.locked.set(true);
    }

    pub fn rate(&self, stars: u8) {
        if !self.locked.get_untracked() {
            self.stars.set(stars.min(MAX_STARS));
        }
    }

    pub fn can_submit(&self) -> bool {
        !self.locked.get() && self.stars.get() > 0
    }

    pub fn submit(&self) {
        self.submitted.set(Some(format!("Thanks for the {} stars!", self.stars.get_untracked())));
    }

    /// What the screen says under the form.
    pub fn status(&self) -> String {
        match (self.submitted.get(), self.locked.get(), self.stars.get()) {
            (Some(thanks), _, _) => thanks,
            (None, true, _) => "Click the lock to make changes".to_string(),
            (None, false, 0) => "Not rated yet".to_string(),
            (None, false, n) => format!("{n} of {MAX_STARS} stars"),
        }
    }
}

/// The store the screen was given.
pub fn use_review() -> Review {
    inject::<Review>().expect("provide a Review before mounting the screen")
}
