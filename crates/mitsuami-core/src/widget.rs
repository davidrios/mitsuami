//! What the core tells backends to create, and the properties they carry.

use std::fmt;

/// Stable identity of a node for the lifetime of a [`Ui`](crate::Ui).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub(crate) u32);

impl NodeId {
    pub fn raw(self) -> u32 {
        self.0
    }

    /// For backends and tools that need to rebuild ids, e.g. when replaying a
    /// command log.
    pub fn from_raw(raw: u32) -> NodeId {
        NodeId(raw)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WidgetKind {
    Window,
    /// A layout host: a plain native view that we position children in.
    Container,
    /// Core-only grouping used by control flow (`Show`, `For`). Never sent to
    /// backends; its children are spliced into the nearest native ancestor.
    Fragment,
    Text,
    Button,
    TextInput,
    Checkbox,
    Switch,
    /// A custom widget registered by name (see `CustomWidget`, M4).
    Custom(&'static str),
    /// A raw native view supplied by app code (see `NativeView`, M4).
    Native,
}

impl WidgetKind {
    pub fn is_native(self) -> bool {
        self != WidgetKind::Fragment
    }

    /// Containers lay out children; everything else is measured by the backend.
    pub fn is_container(self) -> bool {
        matches!(self, WidgetKind::Window | WidgetKind::Container)
    }

    pub fn name(self) -> &'static str {
        match self {
            WidgetKind::Window => "Window",
            WidgetKind::Container => "Container",
            WidgetKind::Fragment => "Fragment",
            WidgetKind::Text => "Text",
            WidgetKind::Button => "Button",
            WidgetKind::TextInput => "TextInput",
            WidgetKind::Checkbox => "Checkbox",
            WidgetKind::Switch => "Switch",
            WidgetKind::Custom(name) => name,
            WidgetKind::Native => "Native",
        }
    }
}

/// Semantic text styles, mapped to each platform's type ramp.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextStyle {
    LargeTitle,
    Title,
    Headline,
    #[default]
    Body,
    Callout,
    Caption,
    Monospace,
}

/// Semantic button variants, mapped to each platform's native styles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ButtonVariant {
    #[default]
    Default,
    Primary,
    Destructive,
    Plain,
}

/// A property of a native widget. Which ones apply depends on the kind.
#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    /// Window title.
    Title(String),
    /// Text content of a `Text`.
    Text(String),
    /// Caption of a `Button`, `Checkbox` or `Switch`.
    Label(String),
    /// Current text of a `TextInput`.
    Value(String),
    Placeholder(String),
    Checked(bool),
    Enabled(bool),
    TextStyle(TextStyle),
    Variant(ButtonVariant),
}

impl Prop {
    /// Two props with the same key replace each other.
    pub fn key(&self) -> std::mem::Discriminant<Prop> {
        std::mem::discriminant(self)
    }

    /// Whether changing this prop can change the widget's intrinsic size.
    pub fn affects_measure(&self) -> bool {
        matches!(self, Prop::Text(_) | Prop::Label(_) | Prop::Placeholder(_) | Prop::TextStyle(_) | Prop::Variant(_))
    }
}

/// Finds a prop by pattern in a prop list.
#[macro_export]
macro_rules! find_prop {
    ($props:expr, $variant:ident) => {
        $props.iter().find_map(|p| match p {
            $crate::Prop::$variant(v) => Some(v.clone()),
            _ => None,
        })
    };
}

macro_rules! static_value {
    ($($t:ty),*) => {$(
        impl mitsuami_reactive::IntoValue<$t> for $t {
            fn into_value(self) -> mitsuami_reactive::Value<$t> {
                mitsuami_reactive::Value::Static(self)
            }
        }
    )*};
}

static_value!(TextStyle, ButtonVariant);
