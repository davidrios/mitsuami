//! The pager's KDE render: Qt's own `PageIndicator`.

use mitsuami::kirigami::{KirigamiCx, NativeRender, QmlObject};

use super::{PipsPager, PipsPagerEvent, PipsPagerProps};

fn apply(pager: QmlObject, props: &PipsPagerProps) {
    pager.set_int("count", props.count as i32);
    pager.set_int("currentIndex", props.selected as i32);
}

impl NativeRender for PipsPager {
    fn create(props: &PipsPagerProps, cx: &mut KirigamiCx) -> QmlObject {
        let pager = cx.load("QQC2.PageIndicator { interactive: true }");
        apply(pager, props);
        // A click selects the pip natively; the app answers with new props.
        // The backend's own updates aren't reported.
        let emitter = cx.emitter();
        pager.connect("currentIndexChanged()", move || {
            emitter.emit(PipsPagerEvent::Selected(pager.int("currentIndex").max(0) as u8))
        });
        pager
    }

    fn update(pager: QmlObject, _old: &PipsPagerProps, new: &PipsPagerProps) {
        apply(pager, new);
    }

    fn read(pager: QmlObject, _props: &PipsPagerProps) -> PipsPagerProps {
        PipsPagerProps { count: pager.int("count").max(0) as u8, selected: pager.int("currentIndex").max(0) as u8 }
    }
}
