//! The review screen for macOS: a form with trailing labels, a stepper
//! next to the stars (a raw `NSStepper`), and the button at the trailing
//! edge, where Mac users look for it.

use mitsuami::appkit::NativeView;
use mitsuami::appkit::objc2_app_kit::NSStepper;
use mitsuami::prelude::*;

use crate::rating::{Rating, RatingEvent, RatingProps};
use crate::store::{MAX_STARS, use_review};

/// A native `NSStepper` bound to the number of stars.
pub fn stars_stepper() -> impl View {
    let review = use_review();
    NativeView::appkit(|cx| {
        let stepper = NSStepper::new(cx.mtm());
        stepper.setMinValue(0.0);
        stepper.setMaxValue(MAX_STARS as f64);
        stepper.setIncrement(1.0);
        let emitter = cx.emitter();
        cx.on_action(&*stepper, move |stepper: &NSStepper| emitter.emit(stepper.doubleValue().round() as u8));
        stepper
    })
    .update(review.stars, |stepper, stars| stepper.setDoubleValue(*stars as f64))
    .on_event(move |stars: &u8| review.rate(*stars))
    .a11y_label("Stars")
}

pub fn review_screen() -> impl View {
    let review = use_review();
    let label = |text: &'static str| Row::new().justify(Justify::End).child(Text::new(text));
    Column::new().gap(Spacing::Lg).children((
        Text::new("Rate mitsuami").text_style(TextStyle::Headline),
        Grid::new()
            .columns([Track::MaxContent, Track::Size(1.fr())])
            .column_gap(Spacing::Sm)
            .row_gap(Spacing::Md)
            .align(Align::Center)
            .children((
                label("Rating:"),
                Row::new().gap(Spacing::Sm).align(Align::Center).children((
                    Rating::view(move || RatingProps::new(review.stars.get()))
                        .a11y_label("Your rating")
                        .on_event(move |RatingEvent::Changed(stars)| review.rate(*stars)),
                    stars_stepper(),
                )),
                label("Comment:"),
                TextInput::new().a11y_label("Comment").placeholder("What do you think?").bind(review.comment),
            )),
        Row::new().gap(Spacing::Md).align(Align::Center).children((
            Text::new(move || review.status()).grow(1.0),
            Button::new("Submit")
                .variant(ButtonVariant::Primary)
                .enabled(move || review.can_submit())
                .on_click(move || review.submit()),
        )),
    ))
}
