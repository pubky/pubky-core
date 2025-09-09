use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};


/// A macro that wraps a test function and executes drop_dbs() at the end,
/// regardless of whether the test panics or succeeds.
#[proc_macro_attribute]
pub fn pubky_testcase(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_block = &input_fn.block;
    let fn_vis = &input_fn.vis;
    let fn_attrs = &input_fn.attrs;
    let fn_sig = &input_fn.sig;
    
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
                    rt.block_on(pubky_test_utils::drop_dbs());
                } else {
                    // Fallback: create a new runtime if we're not in a tokio context
                    if let Ok(rt) = tokio::runtime::Runtime::new() {
                        rt.block_on(pubky_test_utils::drop_dbs());
                    }
                }
                    // Call the original panic hook
                    original_hook(panic_info);
                }));
                
                // Execute the test body
                #fn_block
                
                // Restore the original panic hook
                std::panic::set_hook(original_hook);
                
                // Always execute drop_dbs() after the test completes normally
                pubky_test_utils::drop_dbs().await;
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
                    rt.block_on(pubky_test_utils::drop_dbs());
                } else {
                    // Fallback: create a new runtime if we're not in a tokio context
                    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                    rt.block_on(pubky_test_utils::drop_dbs());
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
