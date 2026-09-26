//! Stories: a view in a given state, captured at each of its sizes in each
//! variant. See `#[mitsuami_test::story]`.

use mitsuami_core::Appearance;

/// What a story is captured in, besides its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Light,
    Dark,
}

impl Variant {
    pub(crate) fn appearance(self) -> Appearance {
        match self {
            Variant::Light => Appearance::Light,
            Variant::Dark => Appearance::Dark,
        }
    }

    /// Used in test names and baseline file names.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Variant::Light => "light",
            Variant::Dark => "dark",
        }
    }
}
