//! The rating on KDE, built ad hoc: neither Qt Quick nor Kirigami has a
//! rating control, so this is how Discover builds its own: a row of flat
//! tool buttons with Breeze's star icons.

use mitsuami::kirigami::{KirigamiCx, NativeRender, QmlObject};

use super::{Rating, RatingEvent, RatingProps};

const ROW: &str = r#"
Row {
    id: row
    property int value: 0
    property int max: 5
    property bool editable: true
    // The star the user clicked, reported with `requested()`.
    property int clicked: 0
    signal requested()
    Repeater {
        model: row.max
        QQC2.ToolButton {
            required property int index
            icon.name: index < row.value ? "rating" : "rating-unrated"
            display: QQC2.AbstractButton.IconOnly
            enabled: row.editable
            // The rating is one control for keyboard and screen readers;
            // the buttons are only for the pointer.
            focusPolicy: Qt.NoFocus
            onClicked: { row.clicked = index + 1; row.requested() }
        }
    }
}
"#;

fn apply(row: QmlObject, props: &RatingProps) {
    row.set_int("max", props.max as i32);
    row.set_int("value", props.value as i32);
    row.set_bool("editable", props.editable);
}

impl NativeRender for Rating {
    fn create(props: &RatingProps, cx: &mut KirigamiCx) -> QmlObject {
        let row = cx.load(ROW);
        apply(row, props);
        let emitter = cx.emitter();
        // A click asks for the new value; the app answers with new props.
        row.connect("requested()", move || emitter.emit(RatingEvent::Changed(row.int("clicked") as u8)));
        row
    }

    fn update(row: QmlObject, _old: &RatingProps, new: &RatingProps) {
        apply(row, new);
    }

    fn read(row: QmlObject, _props: &RatingProps) -> RatingProps {
        RatingProps { value: row.int("value") as u8, max: row.int("max") as u8, editable: row.bool("editable") }
    }
}
