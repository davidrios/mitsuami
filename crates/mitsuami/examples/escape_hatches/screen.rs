//! The screen: the same on every platform. Each of its three custom
//! widgets comes from one platform and is native there; elsewhere it's built
//! ad hoc from the platform's widgets, drawn or composed. Labels say which.

use mitsuami::prelude::*;

use crate::lock::{Lock, LockEvent, LockProps};
use crate::pips_pager::{PipsPager, PipsPagerEvent, PipsPagerProps};
use crate::rating::{Rating, RatingEvent, RatingProps};
use crate::store::use_review;

/// What the pager pages through: where each widget comes from.
pub const NOTES: [&str; 3] = [
    "The lock is GTK's GtkLockButton; elsewhere it's a plain button.",
    "The rating is macOS's NSLevelIndicator; GTK builds it from star buttons, like GNOME Software; elsewhere it's drawn.",
    "The pager is WinUI's PipsPager; it's drawn until the WinUI backend can show the real one.",
];

/// "Rating (native)" where the widget is the platform's own control,
/// "Rating" where it's a stand-in.
pub fn caption<W: Render>(name: &str) -> String {
    if W::renderer().is_native() { format!("{name} (native)") } else { name.to_string() }
}

pub fn screen() -> impl View {
    let review = use_review();
    let page = signal(0u8);
    Column::new().gap(Spacing::Lg).children((
        Text::new("Rate mitsuami").text_style(TextStyle::Title),
        Grid::new()
            .columns([Track::MaxContent, Track::Size(1.fr())])
            .column_gap(Spacing::Md)
            .row_gap(Spacing::Md)
            .align(Align::Center)
            .children((
                Text::new(caption::<Lock>("Lock")),
                Row::new().child(
                    Lock::view(move || LockProps { locked: review.locked.get() }).a11y_label("Lock").on_event(
                        move |event| match event {
                            LockEvent::UnlockRequested => review.unlock(),
                            LockEvent::LockRequested => review.lock(),
                        },
                    ),
                ),
                Text::new(caption::<Rating>("Rating")),
                Row::new().child(
                    Rating::view(move || RatingProps {
                        editable: !review.locked.get(),
                        ..RatingProps::new(review.stars.get())
                    })
                    .a11y_label("Rating")
                    .on_event(move |RatingEvent::Changed(stars)| review.rate(*stars)),
                ),
                Text::new(caption::<PipsPager>("Page")),
                Row::new().child(
                    PipsPager::view(move || PipsPagerProps { count: NOTES.len() as u8, selected: page.get() })
                        .a11y_label("Page")
                        .on_event(move |PipsPagerEvent::Selected(p)| page.set(*p)),
                ),
            )),
        Text::new(move || NOTES[page.get() as usize].to_string()).text_style(TextStyle::Caption),
        Row::new().gap(Spacing::Md).align(Align::Center).children((
            Text::new(move || review.status()).grow(1.0),
            Button::new("Submit")
                .variant(ButtonVariant::Primary)
                .enabled(move || review.can_submit())
                .on_click(move || review.submit()),
        )),
    ))
}
