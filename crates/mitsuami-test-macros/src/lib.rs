//! `#[mitsuami_test::test]`. Use it through `mitsuami-test`, not directly.

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
