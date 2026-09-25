//! The app logic both review screens share: a store, as plain signals and
//! actions. Screens `inject` it; tests can `provide` their own.

use mitsuami::prelude::*;

pub const MAX_STARS: u8 = 5;

#[derive(Clone, Copy)]
pub struct Review {
    pub stars: Signal<u8>,
    pub comment: Signal<String>,
    submitted: Signal<Option<String>>,
}

impl Review {
    pub fn new() -> Review {
        Review { stars: signal(0), comment: signal(String::new()), submitted: signal(None) }
    }

    pub fn rate(&self, stars: u8) {
        self.stars.set(stars.min(MAX_STARS));
    }

    pub fn can_submit(&self) -> bool {
        self.stars.get() > 0
    }

    pub fn submit(&self) {
        let comment = self.comment.get_untracked();
        let stars = self.stars.get_untracked();
        self.submitted.set(Some(match comment.trim() {
            "" => format!("Thanks for the {stars} stars!"),
            comment => format!("Thanks for the {stars} stars: “{comment}”"),
        }));
    }

    /// What the screen says under the form.
    pub fn status(&self) -> String {
        match (self.submitted.get(), self.stars.get()) {
            (Some(thanks), _) => thanks,
            (None, 0) => "Not rated yet".to_string(),
            (None, n) => format!("{n} of {MAX_STARS} stars"),
        }
    }
}

/// The store the current screen was given.
pub fn use_review() -> Review {
    inject::<Review>().expect("provide a Review before mounting a review screen")
}
