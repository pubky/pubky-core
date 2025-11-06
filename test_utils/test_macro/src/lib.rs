use proc_macro::TokenStream;
use proc_macro_crate::{crate_name, FoundCrate};
use quote::quote;
use syn::{parse_macro_input, ItemFn};

/// A macro that wraps a test function and makes sure the postgres test
/// database(s) are dropped after the test completes/panics.
///
/// Usage:
/// ```no_run
/// #[tokio::test]
/// #[pubky_testnet::test]
/// async fn test_function() {
///     // test code
/// }
/// ```
///
/// Important: The test function must be async and `#[tokio::test]` must be present above the macro.
#[proc_macro_attribute]
pub fn pubky_testcase(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_block = &input_fn.block;
    let fn_vis = &input_fn.vis;
    let fn_attrs = &input_fn.attrs;
    let fn_sig = &input_fn.sig;

    /// Because this macro can be used in any crate, we need to get the crate name dynamically.
    /// We support 3 crates: pubky_test_utils, pubky-testnet, pubky_test_utils_macro.
    /// If one of them is not found, we try the next one.
    /// If all of them are not found, we panic.
    fn get_crate_name() -> FoundCrate {
        let lib_names = [
            "pubky_test_utils",
            "pubky-testnet",
            "pubky_test_utils_macro",
        ];
        for lib_name in lib_names.iter() {
            match crate_name(lib_name) {
                Ok(found) => return found,
                Err(_e) => {
                    continue;
                }
            };
        }
        panic!(
            "Failed to get crate name. Tested crates: {}",
            lib_names.join(", ").as_str()
        );
    }

    // Get the crate name
    let found = get_crate_name();
    let my_crate = match found {
        FoundCrate::Itself => quote!(crate),
        FoundCrate::Name(name) => {
            let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
            quote!(::#ident)
        }
    };

    // Check if the function is async
    let is_async = input_fn.sig.asyncness.is_some();

    let expanded = if is_async {
        // Handle async functions
        // For async functions, we use a panic hook approach since catch_unwind doesn't work with async
        quote! {
            #(#fn_attrs)*
            #fn_vis #fn_sig {
                // Set up a panic hook to ensure cleanup happens
                let original_hook = std::panic::take_hook();
                std::panic::set_hook(Box::new(move |panic_info| {
                // Execute cleanup in a blocking way since we're in a panic handler
                if let Ok(rt) = tokio::runtime::Handle::try_current() {
                    rt.block_on(#my_crate::drop_test_databases());
                } else {
                    // Fallback: create a new runtime if we're not in a tokio context
                    if let Ok(rt) = tokio::runtime::Runtime::new() {
                        rt.block_on(#my_crate::drop_test_databases());
                    }
                }
                    // Call the original panic hook
                    original_hook(panic_info);
                }));

                // Execute the test body
                #fn_block

                // Restore the original panic hook
                std::panic::set_hook(original_hook);

                // Always execute drop_test_databases() after the test completes normally
                #my_crate::drop_test_databases().await;
            }
        }
    } else {
        // Handle sync functions - use std::panic::catch_unwind to ensure cleanup
        quote! {
            #(#fn_attrs)*
            #fn_vis #fn_sig {
                // Execute the test body and catch any panics
                let result = std::panic::catch_unwind(|| {
                    #fn_block
                });

                // Always execute drop_dbs() after the test, regardless of outcome
                // Use tokio::runtime to handle the async call in sync context
                if let Ok(rt) = tokio::runtime::Handle::try_current() {
                    rt.block_on(#my_crate::drop_test_databases());
                } else {
                    // Fallback: create a new runtime if we're not in a tokio context
                    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                    rt.block_on(#my_crate::drop_test_databases());
                }

                // Re-panic if the test panicked
                if let Err(panic) = result {
                    std::panic::resume_unwind(panic);
                }
            }
        }
    };

    TokenStream::from(expanded)
}

#[cfg(test)]
mod tests {
    #[test]
    fn macro_compiles() {
        // This test just ensures the macro compiles correctly
        // The actual functionality is tested in integration tests
        assert!(true);
    }
}
