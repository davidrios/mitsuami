//! `#[mitsuami_test::test]` and `#[mitsuami_test::story]`. Use them through
//! `mitsuami-test`, not directly.

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, parse_macro_input};

/// Registers a function as a mitsuami test. It may be `async`, and may take
/// a `TestApp` argument. The test target needs `harness = false` and a
/// `mitsuami_test::main!();` line.
///
/// `#[mitsuami_test::test(headless)]` marks tests that depend on the
/// headless backend (its fixed metrics, simulated system changes); they are
/// skipped with `--native`.
#[proc_macro_attribute]
pub fn test(attr: TokenStream, item: TokenStream) -> TokenStream {
    let headless_only = match attr.to_string().as_str() {
        "" => false,
        "headless" => true,
        other => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("unknown option `{other}`; the only option is `headless` (skip with --native)"),
            )
            .to_compile_error()
            .into();
        }
    };
    let func = parse_macro_input!(item as ItemFn);
    let ident = &func.sig.ident;
    if func.sig.inputs.len() > 1 {
        return syn::Error::new_spanned(&func.sig.inputs, "a mitsuami test takes at most one argument: `app: TestApp`")
            .to_compile_error()
            .into();
    }
    let call = if func.sig.inputs.is_empty() {
        quote! {{ let _ = app; #ident() }}
    } else {
        quote! { #ident(app) }
    };
    let future = if func.sig.asyncness.is_some() {
        quote! { ::std::boxed::Box::pin(#call) }
    } else {
        quote! { ::std::boxed::Box::pin(async move { #call }) }
    };
    quote! {
        #func

        ::mitsuami_test::__private::inventory::submit! {
            ::mitsuami_test::__private::TestCase {
                name: ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#ident)),
                manifest_dir: ::core::env!("CARGO_MANIFEST_DIR"),
                headless_only: #headless_only,
                run: {
                    fn __mitsuami_run(app: ::mitsuami_test::TestApp) -> ::mitsuami_test::__private::TestFuture {
                        #future
                    }
                    __mitsuami_run
                },
            }
        }
    }
    .into()
}

/// Registers a story: a function returning the view to capture, in the
/// state to capture it in. It runs as one test per size and variant, which
/// mounts the view in a window of that size, plays the script if there is
/// one, then compares a capture of the window with its baseline in
/// `tests/visual/<backend>/<image>/<story>@<width>x<height>-<variant>.png`
/// (`x<height>` is `xfit` for fitted heights).
///
/// ```ignore
/// #[mitsuami_test::story(sizes = [(320, fit), (640, 480)], variants = [Light, Dark], play = type_a_name)]
/// fn signup_filled() -> impl View { signup() }
///
/// async fn type_a_name(app: &TestApp) {
///     app.get_by_label("Name").fill("Ada").await;
/// }
/// ```
///
/// - `sizes`: window content sizes, in logical units. A height of `fit`
///   fits the content, whose height differs per platform, at its first
///   layout. The default is the test window's, 800×600.
/// - `variants`: `Light`, `Dark`. The default is both.
/// - `play`: an `async fn(&TestApp)` that brings the view into the state to
///   capture (focus a field, type text) once it is mounted.
///
/// Headless has no pixels: there, stories only check that the view mounts
/// and the script plays.
#[proc_macro_attribute]
pub fn story(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut sizes: Option<Vec<(syn::Expr, syn::Expr)>> = None;
    let mut variants: Option<Vec<syn::Ident>> = None;
    let mut play: Option<syn::Path> = None;
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("sizes") {
            let syn::Expr::Array(array) = meta.value()?.parse()? else {
                return Err(meta.error("expected a list of sizes: `sizes = [(320, fit), (640, 480)]`"));
            };
            let mut list = Vec::new();
            for element in array.elems {
                match element {
                    syn::Expr::Tuple(tuple) if tuple.elems.len() == 2 => {
                        let mut elems = tuple.elems.into_iter();
                        list.push((elems.next().unwrap(), elems.next().unwrap()));
                    }
                    other => {
                        return Err(syn::Error::new_spanned(
                            other,
                            "expected a size: `(width, height)` or `(width, fit)`",
                        ));
                    }
                }
            }
            if list.is_empty() {
                return Err(meta.error("a story needs at least one size"));
            }
            sizes = Some(list);
        } else if meta.path.is_ident("variants") {
            let syn::Expr::Array(array) = meta.value()?.parse()? else {
                return Err(meta.error("expected a list of variants: `variants = [Light, Dark]`"));
            };
            let mut list = Vec::new();
            for element in array.elems {
                match element {
                    syn::Expr::Path(path) if path.path.get_ident().is_some() => {
                        list.push(path.path.get_ident().unwrap().clone());
                    }
                    other => return Err(syn::Error::new_spanned(other, "expected a variant: `Light` or `Dark`")),
                }
            }
            if list.is_empty() {
                return Err(meta.error("a story needs at least one variant"));
            }
            variants = Some(list);
        } else if meta.path.is_ident("play") {
            play = Some(meta.value()?.parse()?);
        } else {
            return Err(meta.error("unknown option; the options are `sizes`, `variants` and `play`"));
        }
        Ok(())
    });
    parse_macro_input!(attr with parser);

    let func = parse_macro_input!(item as ItemFn);
    let ident = &func.sig.ident;
    if !func.sig.inputs.is_empty() || func.sig.asyncness.is_some() {
        return syn::Error::new_spanned(&func.sig, "a story is a plain function with no arguments that returns a view")
            .to_compile_error()
            .into();
    }
    let sizes = match sizes {
        Some(sizes) => {
            let sizes = sizes.iter().map(|(w, h)| {
                let fit = matches!(h, syn::Expr::Path(p) if p.path.is_ident("fit"));
                let h = if fit {
                    quote! { ::core::option::Option::None }
                } else {
                    quote! { ::core::option::Option::Some((#h) as f32) }
                };
                quote! { ((#w) as f32, #h) }
            });
            quote! { &[#(#sizes),*] }
        }
        None => quote! { &[(800.0, ::core::option::Option::Some(600.0))] },
    };
    let variants = variants.unwrap_or_else(|| {
        vec![
            syn::Ident::new("Light", proc_macro2::Span::call_site()),
            syn::Ident::new("Dark", proc_macro2::Span::call_site()),
        ]
    });
    let play = play.map(|play| quote! { #play(app).await; });
    quote! {
        #func

        ::mitsuami_test::__private::inventory::submit! {
            ::mitsuami_test::__private::StoryCase {
                name: ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#ident)),
                manifest_dir: ::core::env!("CARGO_MANIFEST_DIR"),
                sizes: #sizes,
                variants: &[#(::mitsuami_test::Variant::#variants),*],
                run: {
                    fn __mitsuami_story(
                        app: &::mitsuami_test::TestApp,
                    ) -> ::mitsuami_test::__private::StoryFuture<'_> {
                        ::std::boxed::Box::pin(async move {
                            app.mount(#ident);
                            #play
                        })
                    }
                    __mitsuami_story
                },
            }
        }
    }
    .into()
}
