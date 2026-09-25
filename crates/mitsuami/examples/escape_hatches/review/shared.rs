//! The review screen for Windows and Linux: a plain column.

use mitsuami::prelude::*;

use crate::rating::{Rating, RatingEvent, RatingProps};
use crate::store::use_review;

pub fn review_screen() -> impl View {
    let review = use_review();
    Column::new().gap(Spacing::Md).children((
        Text::new("Rate mitsuami").text_style(TextStyle::Title),
        Row::new().child(
            Rating::view(move || RatingProps::new(review.stars.get()))
                .a11y_label("Your rating")
                .on_event(move |RatingEvent::Changed(stars)| review.rate(*stars)),
        ),
        TextInput::new().a11y_label("Comment").placeholder("What do you think?").bind(review.comment),
        Button::new("Submit")
            .variant(ButtonVariant::Primary)
            .enabled(move || review.can_submit())
            .on_click(move || review.submit()),
        Text::new(move || review.status()),
    ))
}
