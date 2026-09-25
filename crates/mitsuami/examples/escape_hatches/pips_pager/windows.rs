//! The pager's WinUI render: `PipsPager` itself.

use mitsuami::winui::bindings::{IPipsPager, PipsPager as XamlPipsPager};
use mitsuami::winui::windows_core::{Interface, Result};
use mitsuami::winui::{NativeRender, WinUiCx};

use super::{PipsPager, PipsPagerEvent, PipsPagerProps};

fn apply(pager: &IPipsPager, props: &PipsPagerProps) -> Result<()> {
    pager.SetNumberOfPages(props.count as i32)?;
    pager.SetSelectedPageIndex(props.selected as i32)
}

impl NativeRender for PipsPager {
    type Element = XamlPipsPager;

    fn create(props: &PipsPagerProps, cx: &mut WinUiCx) -> Result<XamlPipsPager> {
        let pager = XamlPipsPager::new()?;
        let iface: IPipsPager = pager.cast()?;
        apply(&iface, props)?;
        // A click selects the pip natively; the app answers with new props.
        let emitter = cx.emitter();
        cx.observe(&pager, &XamlPipsPager::SelectedPageIndexProperty()?, move |pager| {
            if let Ok(index) = pager.cast::<IPipsPager>().and_then(|p| p.SelectedPageIndex()) {
                emitter.emit(PipsPagerEvent::Selected(index.max(0) as u8));
            }
        })?;
        Ok(pager)
    }

    fn update(pager: &XamlPipsPager, _old: &PipsPagerProps, new: &PipsPagerProps) -> Result<()> {
        apply(&pager.cast()?, new)
    }

    fn read(pager: &XamlPipsPager, props: &PipsPagerProps) -> PipsPagerProps {
        let Ok(pager) = pager.cast::<IPipsPager>() else { return props.clone() };
        PipsPagerProps {
            count: pager.NumberOfPages().map_or(props.count, |n| n.max(0) as u8),
            selected: pager.SelectedPageIndex().map_or(props.selected, |i| i.max(0) as u8),
        }
    }
}
