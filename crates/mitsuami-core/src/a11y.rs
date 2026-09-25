//! Accessibility model. Every node carries semantics from day one. The test
//! driver uses the same roles, names and actions as assistive technology.

use crate::NodeId;
use crate::geometry::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// Not exposed; children are exposed in its place.
    None,
    Window,
    Group,
    StaticText,
    Heading,
    Button,
    TextField,
    Checkbox,
    Switch,
    Image,
    Slider,
    List,
    ListItem,
    ScrollArea,
}

/// Overrides and additions to the semantics a widget derives on its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct A11yProps {
    pub role: Option<Role>,
    pub label: Option<String>,
    pub description: Option<String>,
    pub labelled_by: Option<NodeId>,
    /// Remove this node and its subtree from the accessibility tree.
    pub hidden: bool,
}

impl A11yProps {
    pub fn is_empty(&self) -> bool {
        *self == A11yProps::default()
    }
}

/// Something assistive technology (or a test) can ask a control to do.
#[derive(Clone, Debug, PartialEq)]
pub enum A11yAction {
    /// Press / click / toggle, whatever the control's primary action is.
    Activate,
    Focus,
    SetValue(String),
    Increment,
    Decrement,
    ScrollIntoView,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionError {
    UnknownNode,
    Disabled,
    Unsupported,
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ActionError::UnknownNode => "the node does not exist",
            ActionError::Disabled => "the control is disabled",
            ActionError::Unsupported => "the control does not support this action",
        })
    }
}

/// A node of the computed accessibility tree.
#[derive(Clone, Debug, PartialEq)]
pub struct A11yNode {
    pub id: NodeId,
    pub role: Role,
    pub name: Option<String>,
    pub description: Option<String>,
    pub value: Option<String>,
    pub checked: Option<bool>,
    pub enabled: bool,
    pub test_id: Option<String>,
    /// In window coordinates.
    pub frame: Rect,
    pub children: Vec<A11yNode>,
}

impl A11yNode {
    /// Depth-first iteration over this node and all descendants.
    pub fn walk(&self) -> Vec<&A11yNode> {
        let mut out = vec![self];
        for child in &self.children {
            out.extend(child.walk());
        }
        out
    }
}
