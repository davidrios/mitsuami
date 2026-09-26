//! The lock's KDE render: Qt's `DelayButton`, a button that acts only once
//! it's been held until its progress fills. It guards a change against a
//! stray click the way a padlock does.
//!
//! A completed hold asks to unlock or to lock; the app decides and answers
//! with new props. A screen reader can't hold a button, so its press goes
//! through the shared definition (`CustomWidget::action`) instead.

use mitsuami::kirigami::{KirigamiCx, NativeRender, QmlObject};

use super::{Lock, LockEvent, LockProps};

const BUTTON: &str = r#"
QQC2.DelayButton {
    id: button
    property bool locked: true
    text: locked ? "Hold to Unlock" : "Hold to Lock"
    icon.name: locked ? "object-locked" : "object-unlocked"
    delay: 600
    // Not a toggle: once it has asked, it's ready to be held again. Qt
    // checks it on the release that follows a full hold (unchecking it on
    // activation had the release check it again), so it's unchecked then.
    onActivated: button.requested()
    onReleased: button.checked = false
    signal requested()
}
"#;

impl NativeRender for Lock {
    fn create(props: &LockProps, cx: &mut KirigamiCx) -> QmlObject {
        let button = cx.load(BUTTON);
        button.set_bool("locked", props.locked);
        let emitter = cx.emitter();
        button.connect("requested()", move || emitter.emit(LockEvent::toggle(button.bool("locked"))));
        button
    }

    fn update(button: QmlObject, _old: &LockProps, new: &LockProps) {
        button.set_bool("locked", new.locked);
    }

    fn read(button: QmlObject, _props: &LockProps) -> LockProps {
        LockProps { locked: button.bool("locked") }
    }
}
