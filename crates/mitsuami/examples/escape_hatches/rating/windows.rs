//! The rating's WinUI render: `RatingControl` itself.

use mitsuami::winui::bindings::{IRatingControl, RatingControl};
use mitsuami::winui::windows_core::{Interface, Result};
use mitsuami::winui::{NativeRender, WinUiCx};

use super::{Rating, RatingEvent, RatingProps};

fn apply(control: &IRatingControl, props: &RatingProps) -> Result<()> {
    control.SetMaxRating(props.max as i32)?;
    // -1 is "not rated" (no stars); 0 isn't a rating it expects.
    control.SetValue(if props.value == 0 { -1.0 } else { props.value as f64 })?;
    control.SetIsReadOnly(!props.editable)
}

fn stars(value: f64) -> u8 {
    value.round().max(0.0) as u8
}

impl NativeRender for Rating {
    type Element = RatingControl;

    fn create(props: &RatingProps, cx: &mut WinUiCx) -> Result<RatingControl> {
        let control = RatingControl::new()?;
        let iface: IRatingControl = control.cast()?;
        // Clicking the current star would clear the rating; the app decides.
        iface.SetIsClearEnabled(false)?;
        apply(&iface, props)?;
        // A click (or a screen reader) sets the stars natively; the app
        // answers with new props. The property, not `ValueChanged`: that
        // event ignores values set through UI Automation.
        let emitter = cx.emitter();
        cx.observe(&control, &RatingControl::ValueProperty()?, move |control| {
            if let Ok(value) = control.cast::<IRatingControl>().and_then(|c| c.Value()) {
                emitter.emit(RatingEvent::Changed(stars(value)));
            }
        })?;
        Ok(control)
    }

    fn update(control: &RatingControl, _old: &RatingProps, new: &RatingProps) -> Result<()> {
        apply(&control.cast()?, new)
    }

    fn read(control: &RatingControl, props: &RatingProps) -> RatingProps {
        let Ok(control) = control.cast::<IRatingControl>() else { return props.clone() };
        RatingProps {
            value: control.Value().map_or(props.value, stars),
            max: control.MaxRating().map_or(props.max, |m| m.max(0) as u8),
            editable: control.IsReadOnly().map_or(props.editable, |r| !r),
        }
    }
}
