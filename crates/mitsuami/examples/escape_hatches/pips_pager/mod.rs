//! A pips pager, the WinUI widget: a row of dots, one per page, the current
//! one larger. On Windows it's WinUI's own `PipsPager` (`windows.rs`), and
//! on KDE Qt's `PageIndicator` (`kde.rs`). Neither AppKit nor GTK has one,
//! so elsewhere it's drawn.

use mitsuami::prelude::*;

#[cfg(all(target_os = "linux", feature = "kde"))]
mod kde;
#[cfg(windows)]
mod windows;

pub struct PipsPager;

#[derive(Clone, Debug, PartialEq)]
pub struct PipsPagerProps {
    pub count: u8,
    /// Zero-based.
    pub selected: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PipsPagerEvent {
    Selected(u8),
}

impl CustomWidget for PipsPager {
    const NAME: &'static str = "PipsPager";
    type Props = PipsPagerProps;
    type Event = PipsPagerEvent;

    fn a11y(props: &PipsPagerProps) -> A11yProps {
        A11yProps::new(Role::Slider).value(format!("Page {} of {}", props.selected + 1, props.count))
    }

    fn action(props: &PipsPagerProps, action: &A11yAction) -> Option<PipsPagerEvent> {
        let last = props.count.saturating_sub(1);
        let page = match action {
            A11yAction::Increment => props.selected.saturating_add(1).min(last),
            A11yAction::Decrement => props.selected.saturating_sub(1),
            // Pages are read out one-based.
            A11yAction::SetValue(text) => text.trim().parse::<u8>().ok()?.clamp(1, props.count) - 1,
            _ => return None,
        };
        (page != props.selected).then_some(PipsPagerEvent::Selected(page))
    }
}

impl Drawn for PipsPager {
    fn measure(props: &PipsPagerProps, _request: &MeasureRequest, metrics: &PlatformMetrics) -> Size {
        // One square cell per pip, as tall as body text.
        let cell = metrics.font_sizes.body.round();
        Size::new(cell * props.count as f32, cell)
    }

    fn draw(props: &PipsPagerProps, canvas: &mut Canvas) {
        let cell = canvas.size().height;
        for i in 0..props.count {
            let selected = i == props.selected;
            let diameter = cell * if selected { 0.5 } else { 0.3 };
            let (cx, cy) = (i as f32 * cell + cell / 2.0, cell / 2.0);
            let pip = Shape::Ellipse(Rect::new(cx - diameter / 2.0, cy - diameter / 2.0, diameter, diameter));
            canvas.fill(pip, if selected { Color::Label } else { Color::SecondaryLabel });
        }
    }

    fn pointer(props: &PipsPagerProps, size: Size, event: &PointerEvent) -> Option<PipsPagerEvent> {
        if event.kind != PointerKind::Up || size.height <= 0.0 {
            return None;
        }
        let page = ((event.position.x / size.height).floor() as i32).clamp(0, props.count as i32 - 1) as u8;
        (page != props.selected).then_some(PipsPagerEvent::Selected(page))
    }
}

impl Render for PipsPager {
    fn renderer() -> Renderer<Self> {
        platform! {
            windows => mitsuami::winui::native::<Self>().with_drawn(),
            kde => mitsuami::kirigami::native::<Self>().with_drawn(),
            _ => Renderer::drawn(),
        }
    }
}
