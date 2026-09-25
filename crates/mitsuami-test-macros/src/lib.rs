//! `#[mitsuami_test::test]`. Use it through `mitsuami-test`, not directly.

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, parse_macro_input};

/// Registers a function as a mitsuami test. It may be `async`, and may take
/// a `TestApp` argument. The test target needs `harness = false` and a
/// `mitsuami_test::main!();` line.
#[proc_macro_attribute]
pub fn test(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(proc_macro2::Span::call_site(), "#[mitsuami_test::test] takes no arguments")
            .to_compile_error()
            .into();
    }
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
