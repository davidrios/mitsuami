//! The review screen: a macOS-specific version and a shared one, both over
//! the same store.

use mitsuami::prelude::*;

use crate::rating::{self, Rating, RatingEvent, RatingProps};
use crate::store::use_review;

#[cfg(target_os = "macos")]
pub mod macos;
// Built everywhere, so it can be tested everywhere; only other platforms show it.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub mod shared;

pub fn review_screen() -> impl View {
    platform! {
        macos => macos::review_screen(),
        _ => shared::review_screen(),
    }
}

/// One rating, three ways: the platform's render, the drawn render, and
/// buttons. All bound to the same store.
pub fn renders() -> impl View {
    let review = use_review();
    let props = move || RatingProps::new(review.stars.get());
    let rate = move |RatingEvent::Changed(stars): &RatingEvent| review.rate(*stars);
    Column::new().gap(Spacing::Sm).children((
        Text::new("One rating, three renders").text_style(TextStyle::Headline),
        Grid::new()
            .columns([Track::MaxContent, Track::Size(1.fr())])
            .column_gap(Spacing::Md)
            .row_gap(Spacing::Sm)
            .align(Align::Center)
            .children((
                Text::new("Native").text_style(TextStyle::Caption),
                Row::new().child(Rating::view(props).a11y_label("Native rating").on_event(rate)),
                Text::new("Drawn").text_style(TextStyle::Caption),
                Row::new().child(Rating::view(props).drawn().a11y_label("Drawn rating").on_event(rate)),
                Text::new("Composed").text_style(TextStyle::Caption),
                rating::composed(move || review.stars.get(), move |stars| review.rate(stars)),
            )),
    ))
}
