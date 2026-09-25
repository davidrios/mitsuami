//! A star rating: one shared definition, three renders.
//!
//! - Native on macOS: `NSLevelIndicator` in its rating style (`macos.rs`).
//! - Drawn everywhere else, until GTK and WinUI have native renders, and
//!   wherever `.drawn()` asks for it.
//! - Composed from buttons ([`composed`]): the tier that needs no render at all.

use mitsuami::prelude::*;

#[cfg(target_os = "macos")]
mod macos;

pub struct Rating;

#[derive(Clone, Debug, PartialEq)]
pub struct RatingProps {
    pub value: u8,
    pub max: u8,
    pub editable: bool,
}

impl RatingProps {
    pub fn new(value: u8) -> RatingProps {
        RatingProps { value, max: crate::store::MAX_STARS, editable: true }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RatingEvent {
    Changed(u8),
}

impl CustomWidget for Rating {
    const NAME: &'static str = "Rating";
    type Props = RatingProps;
    type Event = RatingEvent;

    fn a11y(props: &RatingProps) -> A11yProps {
        A11yProps::new(Role::Slider).label("Rating").value(format!("{} of {}", props.value, props.max))
    }

    fn action(props: &RatingProps, action: &A11yAction) -> Option<RatingEvent> {
        if !props.editable {
            return None;
        }
        let value = match action {
            A11yAction::Increment => props.value.saturating_add(1).min(props.max),
            A11yAction::Decrement => props.value.saturating_sub(1),
            A11yAction::SetValue(text) => text.trim().parse::<u8>().ok()?.min(props.max),
            _ => return None,
        };
        (value != props.value).then_some(RatingEvent::Changed(value))
    }
}

/// Stars are as tall as the widget, with a quarter of that between them.
fn pitch(star: f32) -> f32 {
    star * 1.25
}

fn star(x: f32, size: f32) -> Path {
    let (cx, cy) = (x + size / 2.0, size / 2.0);
    let (outer, inner) = (size / 2.0, size / 2.0 * 0.4);
    Path::polygon((0..10).map(|i| {
        let radius = if i % 2 == 0 { outer } else { inner };
        let angle = std::f32::consts::PI * (i as f32 / 5.0 - 0.5);
        Point::new(cx + radius * angle.cos(), cy + radius * angle.sin())
    }))
}

impl Drawn for Rating {
    fn measure(props: &RatingProps, _request: &MeasureRequest, metrics: &PlatformMetrics) -> Size {
        // Stars as big as body text, like the platform's rating controls.
        let star = metrics.font_sizes.body.round();
        let count = props.max as f32;
        Size::new(pitch(star) * count - (pitch(star) - star), star)
    }

    fn draw(props: &RatingProps, canvas: &mut Canvas) {
        let star_size = canvas.size().height;
        for i in 0..props.max {
            let path = star(i as f32 * pitch(star_size), star_size);
            if i < props.value {
                canvas.fill(path, Color::Accent);
            } else {
                canvas.stroke(path, Color::SecondaryLabel, 1.0);
            }
        }
    }

    fn pointer(props: &RatingProps, size: Size, event: &PointerEvent) -> Option<RatingEvent> {
        if !props.editable || event.kind != PointerKind::Up || size.height <= 0.0 {
            return None;
        }
        let index = (event.position.x / pitch(size.height)).floor();
        let value = (index as i32 + 1).clamp(1, props.max as i32) as u8;
        (value != props.value).then_some(RatingEvent::Changed(value))
    }
}

impl Render for Rating {
    fn renderer() -> Renderer<Self> {
        platform! {
            macos => mitsuami::appkit::native::<Self>().with_drawn(),
            _ => Renderer::drawn(),
        }
    }
}

/// The same rating composed from plain buttons: runs everywhere, no render.
pub fn composed(value: impl Fn() -> u8 + Copy + 'static, on_change: impl Fn(u8) + Copy + 'static) -> impl View {
    Row::new().gap(Spacing::Xs).children(
        (1..=crate::store::MAX_STARS)
            .map(|n| {
                Button::new(move || if n <= value() { "★" } else { "☆" }.to_string())
                    .variant(ButtonVariant::Plain)
                    .a11y_label(if n == 1 { "1 star".to_string() } else { format!("{n} stars") })
                    .on_click(move || on_change(n))
            })
            .collect::<Vec<_>>(),
    )
}
