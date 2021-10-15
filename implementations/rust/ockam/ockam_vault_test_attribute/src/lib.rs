extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use std::str::FromStr;
use syn::{parse_macro_input, ItemFn, Stmt};

#[proc_macro_attribute]
pub fn vault_test(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let original_fn = parse_macro_input!(item as ItemFn);
    let original_fn_ident = original_fn.sig.ident;
    let import_test = TokenStream::from_str(
        format!(
            "use ockam_vault_test_suite::{};",
            original_fn_ident.to_string()
        )
        .as_str(),
    )
    .unwrap();
    let import_test: Stmt = syn::parse(import_test).expect("B");
    let run_test = TokenStream::from_str(
        format!("{}(&mut vault).await;", original_fn_ident.to_string()).as_str(),
    )
    .unwrap();
    let run_test: Stmt = syn::parse(run_test).expect("B");

    let output_function = quote! {
        #[tokio::test]
        async fn #original_fn_ident() {
            #import_test
            let mut vault = new_vault();
            #run_test
        }
    };

    TokenStream::from(output_function)
}

#[proc_macro_attribute]
pub fn vault_test_sync(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let original_fn = parse_macro_input!(item as ItemFn);
    let original_fn_ident = original_fn.sig.ident;
    let import_test = TokenStream::from_str(
        format!(
            "use ockam_vault_test_suite::{};",
            original_fn_ident.to_string()
        )
        .as_str(),
    )
    .unwrap();
    let import_test: Stmt = syn::parse(import_test).expect("B");
    let run_test = TokenStream::from_str(
        format!("{}(&mut vault).await;", original_fn_ident.to_string()).as_str(),
    )
    .unwrap();
    let run_test: Stmt = syn::parse(run_test).expect("B");

    let output_function = quote! {
        #[test]
        fn #original_fn_ident() {
            #import_test
            use crate::{Vault, VaultSync};

            let (mut ctx, mut executor) = ockam_node::start_node();
            executor
            .execute(async move {
                let vault = new_vault();
                let mut vault = VaultSync::create(&ctx, vault).await.unwrap();
                #run_test

                ctx.stop().await.unwrap()
            })
         .unwrap();
        }
    };

    TokenStream::from(output_function)
}
