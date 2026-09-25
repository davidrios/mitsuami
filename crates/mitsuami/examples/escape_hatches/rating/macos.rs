//! The rating's AppKit render: `NSLevelIndicator` in its rating style.

use mitsuami::appkit::objc2::rc::Retained;
use mitsuami::appkit::objc2_app_kit::{NSLevelIndicator, NSLevelIndicatorStyle};
use mitsuami::appkit::{AppKitCx, NativeRender};
use mitsuami::prelude::*;

use super::{Rating, RatingEvent, RatingProps};

fn apply(indicator: &NSLevelIndicator, props: &RatingProps) {
    indicator.setMaxValue(props.max as f64);
    indicator.setDoubleValue(props.value as f64);
    indicator.setEditable(props.editable);
}

impl NativeRender for Rating {
    type View = NSLevelIndicator;

    fn create(props: &RatingProps, cx: &mut AppKitCx) -> Retained<NSLevelIndicator> {
        let indicator = NSLevelIndicator::new(cx.mtm());
        indicator.setLevelIndicatorStyle(NSLevelIndicatorStyle::Rating);
        indicator.setMinValue(0.0);
        apply(&indicator, props);
        // A click sets the stars natively and sends the action; the app
        // answers with new props.
        let emitter = cx.emitter();
        cx.on_action(&*indicator, move |indicator: &NSLevelIndicator| {
            emitter.emit(RatingEvent::Changed(indicator.doubleValue().round() as u8));
        });
        indicator
    }

    fn update(indicator: &NSLevelIndicator, _old: &RatingProps, new: &RatingProps) {
        apply(indicator, new);
    }

    fn measure(indicator: &NSLevelIndicator, _props: &RatingProps, _request: &MeasureRequest) -> Option<Size> {
        let size = indicator.cell()?.cellSize();
        Some(Size::new(size.width.ceil() as f32, size.height.ceil() as f32))
    }

    fn read(indicator: &NSLevelIndicator, _props: &RatingProps) -> RatingProps {
        RatingProps {
            value: indicator.doubleValue().round() as u8,
            max: indicator.maxValue() as u8,
            editable: indicator.isEditable(),
        }
    }
}
