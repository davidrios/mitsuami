//! The rating on GTK, built ad hoc: GTK has no rating control, so this is
//! how GNOME Software builds its own: a row of flat buttons with star icons.

use mitsuami::gtk::gtk;
use mitsuami::gtk::gtk::prelude::*;
use mitsuami::gtk::{GtkCx, NativeRender};

use super::{Rating, RatingEvent, RatingProps};

const STARRED: &str = "starred-symbolic";
const NOT_STARRED: &str = "non-starred-symbolic";

fn buttons(row: &gtk::Box) -> Vec<gtk::Button> {
    let mut buttons = Vec::new();
    let mut child = row.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        buttons.extend(widget.downcast::<gtk::Button>().ok());
    }
    buttons
}

fn apply(row: &gtk::Box, props: &RatingProps) {
    for (i, button) in buttons(row).iter().enumerate() {
        button.set_icon_name(if (i as u8) < props.value { STARRED } else { NOT_STARRED });
        button.set_sensitive(props.editable);
    }
}

impl NativeRender for Rating {
    type Widget = gtk::Box;

    fn create(props: &RatingProps, cx: &mut GtkCx) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        for n in 1..=props.max {
            let button = gtk::Button::new();
            button.add_css_class("flat");
            // The rating is one control for keyboard and screen readers;
            // the buttons are only for the pointer.
            button.set_focusable(false);
            let emitter = cx.emitter();
            // A click asks for the new value; the app answers with new props.
            button.connect_clicked(move |_| emitter.emit(RatingEvent::Changed(n)));
            row.append(&button);
        }
        apply(&row, props);
        row
    }

    fn update(row: &gtk::Box, _old: &RatingProps, new: &RatingProps) {
        apply(row, new);
    }

    fn read(row: &gtk::Box, _props: &RatingProps) -> RatingProps {
        let buttons = buttons(row);
        RatingProps {
            value: buttons.iter().filter(|b| b.icon_name().as_deref() == Some(STARRED)).count() as u8,
            max: buttons.len() as u8,
            editable: buttons.first().is_none_or(|b| b.is_sensitive()),
        }
    }
}
