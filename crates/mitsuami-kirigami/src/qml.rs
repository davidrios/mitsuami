//! The QML each node kind is created from. Every snippet is compiled once.
//!
//! Controls take two properties of ours: `mitsuamiTextStyle` (see
//! [`text_style`]) and the accessibility overrides `mitsuamiA11yName`,
//! `mitsuamiA11yDescription` and `mitsuamiA11yHidden`.

use mitsuami_core::TextStyle;

/// The number a text style has in `mitsuamiTextStyle`.
pub(crate) fn text_style(style: TextStyle) -> i32 {
    match style {
        TextStyle::Body => 0,
        TextStyle::LargeTitle => 1,
        TextStyle::Title => 2,
        TextStyle::Headline => 3,
        TextStyle::Callout => 4,
        TextStyle::Caption => 5,
        TextStyle::Monospace => 6,
    }
}

/// Kirigami's type scale: titles are `Kirigami.Heading` sizes (levels 1, 2
/// and 3: 1.35, 1.2 and 1.15 × the default font), captions the small font,
/// monospace the fixed-width one. KDE has no callout size. Without Plasma's
/// platform theme the small font can be the larger one; captions are then
/// 0.8 × the default, Plasma's ratio (8 and 10 pt).
const TEXT_STYLE: &str = r#"
    property int mitsuamiTextStyle: 0
    font.family: mitsuamiTextStyle === 6 ? Kirigami.Theme.fixedWidthFont.family
        : mitsuamiTextStyle === 5 ? Kirigami.Theme.smallFont.family : Kirigami.Theme.defaultFont.family
    font.pointSize: mitsuamiTextStyle === 6 ? Kirigami.Theme.fixedWidthFont.pointSize
        : mitsuamiTextStyle === 5 ? (Kirigami.Theme.smallFont.pointSize < Kirigami.Theme.defaultFont.pointSize
            ? Kirigami.Theme.smallFont.pointSize : Kirigami.Theme.defaultFont.pointSize * 0.8)
        : Kirigami.Theme.defaultFont.pointSize * [1, 1.35, 1.2, 1.15, 1, 1, 1][mitsuamiTextStyle]
"#;

fn a11y(default_name: &str) -> String {
    format!(
        r#"
    property string mitsuamiA11yName: ""
    property string mitsuamiA11yDescription: ""
    property bool mitsuamiA11yHidden: false
    Accessible.name: mitsuamiA11yName !== "" ? mitsuamiA11yName : {default_name}
    Accessible.description: mitsuamiA11yDescription
    Accessible.ignored: mitsuamiA11yHidden
"#
    )
}

pub(crate) fn window() -> String {
    // One page, with no padding: its content item is the content host.
    // The page's title goes in Kirigami's toolbar above it.
    r#"
Kirigami.ApplicationWindow {
    width: 800
    height: 600
    pageStack.initialPage: Kirigami.Page {
        objectName: "mitsuamiPage"
        padding: 0
        Item {
            objectName: "mitsuamiHost"
            anchors.fill: parent
        }
    }
}
"#
    .into()
}

pub(crate) fn container() -> String {
    format!("Item {{ {} }}", a11y("\"\""))
}

pub(crate) fn label() -> String {
    // Word wrapping: a word longer than the line overflows rather than
    // breaking, so the longest word is the min-content width.
    format!("QQC2.Label {{ wrapMode: Text.WordWrap; verticalAlignment: Text.AlignTop {TEXT_STYLE} {} }}", a11y("text"))
}

pub(crate) fn button() -> String {
    format!("QQC2.Button {{ {TEXT_STYLE} {} }}", a11y("text"))
}

pub(crate) fn text_field() -> String {
    format!("QQC2.TextField {{ {TEXT_STYLE} {} }}", a11y("placeholderText"))
}

pub(crate) fn checkbox() -> String {
    format!("QQC2.CheckBox {{ {TEXT_STYLE} {} }}", a11y("text"))
}

pub(crate) fn switch() -> String {
    // No caption: the label is the accessible name.
    format!("QQC2.Switch {{ text: \"\"; {TEXT_STYLE} {} }}", a11y("\"\""))
}

/// Our content goes in the flickable's content item; the scroll bars follow
/// `mitsuamiAxes` (1 horizontal, 2 vertical, 3 both).
pub(crate) fn scroll_view() -> String {
    format!(
        r#"
QQC2.ScrollView {{
    id: scroll
    property int mitsuamiAxes: 2
    QQC2.ScrollBar.horizontal.policy: (mitsuamiAxes & 1) ? QQC2.ScrollBar.AsNeeded : QQC2.ScrollBar.AlwaysOff
    QQC2.ScrollBar.vertical.policy: (mitsuamiAxes & 2) ? QQC2.ScrollBar.AsNeeded : QQC2.ScrollBar.AlwaysOff
    Flickable {{
        objectName: "mitsuamiFlickable"
        boundsBehavior: Flickable.StopAtBounds
        flickableDirection: scroll.mitsuamiAxes === 1 ? Flickable.HorizontalFlick
            : scroll.mitsuamiAxes === 2 ? Flickable.VerticalFlick : Flickable.HorizontalAndVerticalFlick
        clip: true
    }}
    {}
}}
"#,
        a11y("\"\"")
    )
}

/// The theme's values, read by the backend: fonts, spacing and colors.
pub(crate) fn theme() -> String {
    r#"
QtObject {
    property font defaultFont: Kirigami.Theme.defaultFont
    property font smallFont: Kirigami.Theme.smallFont
    property font fixedWidthFont: Kirigami.Theme.fixedWidthFont
    property real smallSpacing: Kirigami.Units.smallSpacing
    property real mediumSpacing: Kirigami.Units.mediumSpacing
    property real largeSpacing: Kirigami.Units.largeSpacing
    property real gridUnit: Kirigami.Units.gridUnit
    property real longDuration: Kirigami.Units.longDuration
    property color textColor: Kirigami.Theme.textColor
    property color disabledTextColor: Kirigami.Theme.disabledTextColor
    property color highlightColor: Kirigami.Theme.highlightColor
    property color backgroundColor: Kirigami.Theme.backgroundColor
    property color viewBackgroundColor: viewProbe.Kirigami.Theme.backgroundColor
    property color separatorColor: Kirigami.ColorUtils.linearInterpolation(
        Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, Kirigami.Theme.frameContrast)
    // The View color set (text fields, lists) is another item's theme.
    property Item viewProbe: Item {
        Kirigami.Theme.colorSet: Kirigami.Theme.View
        Kirigami.Theme.inherit: false
    }
}
"#
    .into()
}
