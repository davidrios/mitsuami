//! Views: descriptions of UI that build nodes when mounted.

use crate::ui::Ui;
use crate::widget::{NodeId, Prop, WidgetKind};

/// Something that can build itself into the node tree.
///
/// Component functions run once and return a `View`. Building creates the
/// nodes and the effects that keep their props up to date.
pub trait View: 'static {
    fn build(self, ui: &Ui) -> NodeId;
}

/// A type-erased [`View`].
pub struct AnyView(Box<dyn FnOnce(&Ui) -> NodeId>);

impl AnyView {
    pub fn new(view: impl View) -> AnyView {
        AnyView(Box::new(move |ui| view.build(ui)))
    }
}

impl View for AnyView {
    fn build(self, ui: &Ui) -> NodeId {
        (self.0)(ui)
    }
}

impl View for &'static str {
    fn build(self, ui: &Ui) -> NodeId {
        ui.create(WidgetKind::Text, vec![Prop::Text(self.to_owned())])
    }
}

impl View for String {
    fn build(self, ui: &Ui) -> NodeId {
        ui.create(WidgetKind::Text, vec![Prop::Text(self)])
    }
}

/// Anything that can be a list of children: a view, a tuple of views, a
/// `Vec`, an `Option`, or `()`.
pub trait Children {
    fn into_views(self, out: &mut Vec<AnyView>);
}

impl<V: View> Children for V {
    fn into_views(self, out: &mut Vec<AnyView>) {
        out.push(AnyView::new(self));
    }
}

impl<V: View> Children for Vec<V> {
    fn into_views(self, out: &mut Vec<AnyView>) {
        out.extend(self.into_iter().map(AnyView::new));
    }
}

impl<V: View> Children for Option<V> {
    fn into_views(self, out: &mut Vec<AnyView>) {
        out.extend(self.map(AnyView::new));
    }
}

impl Children for () {
    fn into_views(self, _: &mut Vec<AnyView>) {}
}

macro_rules! tuple_children {
    ($($name:ident),+) => {
        impl<$($name: Children),+> Children for ($($name,)+) {
            #[allow(non_snake_case)]
            fn into_views(self, out: &mut Vec<AnyView>) {
                let ($($name,)+) = self;
                $($name.into_views(out);)+
            }
        }
    };
}

tuple_children!(A);
tuple_children!(A, B);
tuple_children!(A, B, C);
tuple_children!(A, B, C, D);
tuple_children!(A, B, C, D, E);
tuple_children!(A, B, C, D, E, F);
tuple_children!(A, B, C, D, E, F, G);
tuple_children!(A, B, C, D, E, F, G, H);
tuple_children!(A, B, C, D, E, F, G, H, I);
tuple_children!(A, B, C, D, E, F, G, H, I, J);
tuple_children!(A, B, C, D, E, F, G, H, I, J, K);
tuple_children!(A, B, C, D, E, F, G, H, I, J, K, L);
