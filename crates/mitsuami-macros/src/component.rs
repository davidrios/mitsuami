//! `#[component]`: a function becomes a builder with one method per prop.
//!
//! `fn Counter(initial: i32, #[prop(default)] step: i32) -> impl View`
//! becomes `struct Counter<__initial = ()> { initial: __initial, step: i32 }`.
//! Each required prop has a type parameter, `()` until it's set and `(T,)`
//! after, and only the fully set builder implements `View`, so a missing
//! prop is a compile error. Building runs the function in a scope of its
//! own.

use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{Attribute, Error, Expr, FnArg, GenericArgument, ItemFn, Pat, PathArguments, Result, Type};

enum Kind {
    /// `T`: the setter takes `T`, or `impl Into<T>` with `#[prop(into)]`.
    Plain { into: bool },
    /// `Value<T>`: the setter takes `impl IntoValue<T>`.
    Value(Type),
    /// `Callback<T>`: the setter takes `impl Fn(T)`. Optional.
    Callback(Type),
    /// `Option<T>`: the setter takes `impl Into<Option<T>>`. Optional.
    Option,
    /// `children: Slot`: set by the children between the tags. Optional.
    Children,
}

struct Prop {
    name: syn::Ident,
    ty: Type,
    kind: Kind,
    /// The default for optional props; `None` for required ones.
    default: Option<TokenStream>,
    docs: Vec<Attribute>,
}

impl Prop {
    fn required(&self) -> bool {
        self.default.is_none()
    }

    fn state(&self) -> syn::Ident {
        format_ident!("__{}", self.name)
    }
}

pub fn expand(attr: TokenStream, item: TokenStream) -> Result<TokenStream> {
    if !attr.is_empty() {
        return Err(Error::new(attr.span(), "#[component] takes no arguments"));
    }
    let func: ItemFn = syn::parse2(item)?;
    let sig = &func.sig;
    if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        return Err(Error::new(sig.generics.span(), "components can't be generic yet"));
    }
    if let Some(asyncness) = sig.asyncness {
        return Err(Error::new(asyncness.span(), "components can't be async; start tasks with `spawn_local`"));
    }
    let props = sig.inputs.iter().map(prop).collect::<Result<Vec<_>>>()?;

    let vis = &func.vis;
    let name = &sig.ident;
    let docs: Vec<&Attribute> = func.attrs.iter().filter(|a| a.path().is_ident("doc")).collect();
    let other_attrs: Vec<&Attribute> = func.attrs.iter().filter(|a| !a.path().is_ident("doc")).collect();
    let output = &sig.output;
    let body = &func.block;

    let required: Vec<&Prop> = props.iter().filter(|p| p.required()).collect();
    let states: Vec<syn::Ident> = required.iter().map(|p| p.state()).collect();

    // struct Counter<__initial = ()> { initial: __initial, step: i32 }
    let fields = props.iter().map(|p| {
        let (field, ty) = (&p.name, &p.ty);
        if p.required() {
            let state = p.state();
            quote!(#field: #state)
        } else {
            quote!(#field: #ty)
        }
    });
    let struct_def = quote! {
        #(#docs)*
        #[allow(non_camel_case_types)]
        #[must_use = "a component does nothing until it's built as part of a view"]
        #vis struct #name<#(#states = ()),*> {
            #(#fields,)*
        }
    };

    // Counter::new(): required props unset, optional ones at their default.
    let initial = props.iter().map(|p| {
        let field = &p.name;
        match &p.default {
            Some(default) => quote!(#field: #default),
            None => quote!(#field: ()),
        }
    });
    let constructor = quote! {
        impl #name {
            /// The builder, with every required prop still to set.
            #vis fn new() -> #name {
                #name { #(#initial,)* }
            }

            #[doc(hidden)]
            #vis fn __tag() -> #name {
                #name::new()
            }
        }
    };

    // Setters. Setting a required prop changes its state parameter; the
    // other fields move across unchanged.
    let setters = props.iter().map(|p| setter(name, vis, p, &props, &states));

    // View for the fully set builder: run the function in its own scope.
    let full: Vec<TokenStream> = required
        .iter()
        .map(|p| {
            let ty = &p.ty;
            quote!((#ty,))
        })
        .collect();
    let destructure = props.iter().map(|p| {
        let field = &p.name;
        if p.required() { quote!(#field: (#field,)) } else { quote!(#field) }
    });
    let params = props.iter().map(|p| {
        let (field, ty) = (&p.name, &p.ty);
        quote!(#field: #ty)
    });
    let args = props.iter().map(|p| &p.name);
    // The builder is a `View` once every required prop is set. That goes
    // through a trait of its own, so a missing prop is reported by name.
    let ready = format_ident!("__{}RequiredProps", name);
    let message = format!("`<{name}>` is missing a required prop");
    let label = match required.as_slice() {
        [] => String::new(),
        required => {
            let names: Vec<String> = required.iter().map(|p| format!("`{}`", p.name)).collect();
            format!("set every required prop: {}", names.join(", "))
        }
    };
    let view_impl = quote! {
        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        #[diagnostic::on_unimplemented(message = #message, label = #label)]
        #vis trait #ready {
            fn __build(self, ui: &::mitsuami::core::Ui) -> ::mitsuami::core::NodeId;
        }

        impl #ready for #name<#(#full),*> {
            fn __build(self, ui: &::mitsuami::core::Ui) -> ::mitsuami::core::NodeId {
                #(#other_attrs)*
                #[allow(non_snake_case, clippy::too_many_arguments)]
                fn #name(#(#params),*) #output #body

                let #name { #(#destructure,)* } = self;
                let scope = ::mitsuami::reactive::Owner::new_child();
                scope.with(|| ::mitsuami::core::View::build(#name(#(#args),*), ui))
            }
        }

        #[allow(non_camel_case_types)]
        impl<#(#states: 'static),*> ::mitsuami::core::View for #name<#(#states),*>
        where
            Self: #ready,
        {
            fn build(self, ui: &::mitsuami::core::Ui) -> ::mitsuami::core::NodeId {
                #ready::__build(self, ui)
            }
        }
    };

    Ok(quote! {
        #struct_def
        #constructor
        #(#setters)*
        #view_impl
    })
}

fn setter(name: &syn::Ident, vis: &syn::Visibility, prop: &Prop, props: &[Prop], states: &[syn::Ident]) -> TokenStream {
    let field = &prop.name;
    let ty = &prop.ty;
    let docs = &prop.docs;
    let span = prop.name.span();

    let (method, param, value) = match &prop.kind {
        Kind::Plain { into: false } => (field.clone(), quote!(value: #ty), quote!(value)),
        Kind::Plain { into: true } => {
            (field.clone(), quote!(value: impl ::core::convert::Into<#ty>), quote!(value.into()))
        }
        Kind::Value(inner) => (
            field.clone(),
            quote!(value: impl ::mitsuami::reactive::IntoValue<#inner>),
            quote!(::mitsuami::reactive::IntoValue::into_value(value)),
        ),
        Kind::Callback(arg) => (
            field.clone(),
            quote!(handler: impl ::core::ops::Fn(#arg) + 'static),
            quote!(::mitsuami::core::Callback::new(handler)),
        ),
        Kind::Option => (field.clone(), quote!(value: impl ::core::convert::Into<#ty>), quote!(value.into())),
        Kind::Children => (
            format_ident!("__children"),
            quote!(children: impl ::core::ops::FnOnce() -> __C),
            quote!(::mitsuami::core::Slot::new(children())),
        ),
    };
    let generics = match &prop.kind {
        Kind::Children => quote!(<__C: ::mitsuami::core::Children>),
        _ => quote!(),
    };
    let hidden = matches!(prop.kind, Kind::Children).then(|| quote!(#[doc(hidden)]));

    if prop.required() {
        // Generic over every state, so the prop can be set again.
        let state = prop.state();
        let after = states.iter().map(|s| if *s == state { quote!((#ty,)) } else { quote!(#s) });
        let moves = props.iter().map(|p| {
            let f = &p.name;
            if p.name == prop.name { quote!(#f: (#value,)) } else { quote!(#f: self.#f) }
        });
        quote_spanned! {span=>
            #[allow(non_camel_case_types)]
            impl<#(#states),*> #name<#(#states),*> {
                #(#docs)*
                #hidden
                #vis fn #method #generics(self, #param) -> #name<#(#after),*> {
                    #name { #(#moves,)* }
                }
            }
        }
    } else {
        quote_spanned! {span=>
            #[allow(non_camel_case_types)]
            impl<#(#states),*> #name<#(#states),*> {
                #(#docs)*
                #hidden
                #vis fn #method #generics(mut self, #param) -> Self {
                    self.#field = #value;
                    self
                }
            }
        }
    }
}

fn prop(arg: &FnArg) -> Result<Prop> {
    let FnArg::Typed(arg) = arg else {
        return Err(Error::new(arg.span(), "components are functions, not methods"));
    };
    let Pat::Ident(pat) = &*arg.pat else {
        return Err(Error::new(arg.pat.span(), "component props need a plain name: `name: Type`"));
    };
    if pat.by_ref.is_some() || pat.subpat.is_some() {
        return Err(Error::new(pat.span(), "component props need a plain name: `name: Type`"));
    }
    let name = pat.ident.clone();
    let ty = (*arg.ty).clone();
    let docs: Vec<Attribute> = arg.attrs.iter().filter(|a| a.path().is_ident("doc")).cloned().collect();

    let mut default = None;
    let mut into = false;
    for attr in arg.attrs.iter().filter(|a| a.path().is_ident("prop")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("default") {
                default = Some(if meta.input.peek(syn::Token![=]) {
                    let expr: Expr = meta.value()?.parse()?;
                    quote!(#expr)
                } else {
                    quote!(::core::default::Default::default())
                });
                Ok(())
            } else if meta.path.is_ident("into") {
                into = true;
                Ok(())
            } else {
                Err(meta.error("unknown prop option; expected `default`, `default = expr` or `into`"))
            }
        })?;
    }
    if let Some(attr) = arg.attrs.iter().find(|a| !a.path().is_ident("doc") && !a.path().is_ident("prop")) {
        return Err(Error::new(attr.span(), "props take only `#[prop(…)]` and doc comments"));
    }

    let kind = if name == "children" {
        if !last_segment_is(&ty, "Slot") {
            return Err(Error::new(ty.span(), "`children` is where the children go; its type is `Slot`"));
        }
        Kind::Children
    } else if let Some(inner) = single_generic(&ty, "Value") {
        Kind::Value(inner)
    } else if let Some(inner) = single_generic(&ty, "Callback") {
        Kind::Callback(inner)
    } else if single_generic(&ty, "Option").is_some() {
        Kind::Option
    } else {
        Kind::Plain { into }
    };
    if into && !matches!(kind, Kind::Plain { .. }) {
        return Err(Error::new(name.span(), "`#[prop(into)]` is for plain types; this prop converts already"));
    }
    let default = default.or_else(|| match kind {
        Kind::Callback(_) | Kind::Option | Kind::Children => Some(quote!(::core::default::Default::default())),
        _ => None,
    });
    Ok(Prop { name, ty, kind, default, docs })
}

fn last_segment_is(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(p) if p.qself.is_none() && p.path.segments.last().is_some_and(|s| s.ident == name))
}

/// `T` in `Name<T>`.
fn single_generic(ty: &Type, name: &str) -> Option<Type> {
    let Type::Path(p) = ty else { return None };
    let segment = p.path.segments.last()?;
    if p.qself.is_some() || segment.ident != name {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else { return None };
    match args.args.iter().collect::<Vec<_>>().as_slice() {
        [GenericArgument::Type(inner)] => Some(inner.clone()),
        _ => None,
    }
}
