// Copyright 2025, The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Macros for profiling GBL.

use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::{quote, ToTokens};
use syn::parse::{Parse, ParseBuffer};
use syn::{
    parse_macro_input, punctuated::Punctuated, token::Comma, Expr, ExprPath, ItemFn, Meta,
    MetaNameValue, Result,
};

struct ProfileArgs {
    value: Expr,
}

impl ToTokens for ProfileArgs {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        self.value.to_tokens(tokens)
    }
}

impl Parse for ProfileArgs {
    fn parse(input: &ParseBuffer) -> Result<Self> {
        let default = ProfileArgs {
            value: ExprPath {
                attrs: vec![],
                qself: None,
                path: Ident::new("backend", Span::call_site()).into(),
            }
            .into(),
        };
        if input.is_empty() {
            return Ok(default);
        }

        for meta in Punctuated::<Meta, Comma>::parse_terminated(input)? {
            match meta {
                Meta::NameValue(MetaNameValue { path, value, .. }) => {
                    // Any new options or parameters
                    // should be added in this if-else block.
                    if path.is_ident("backend") {
                        return Ok(ProfileArgs { value });
                    } else {
                        return Err(input
                            .error(format!("Unexpected attribute: {}", path.to_token_stream())));
                    }
                }
                Meta::Path(path) => {
                    return Err(input
                        .error(format!("Unexpected path attribute: {}", path.to_token_stream())));
                }
                Meta::List(list) => {
                    return Err(input.error(format!(
                        "Unexpected list attribute: {}",
                        list.path.to_token_stream()
                    )));
                }
            }
        }

        Ok(default)
    }
}

/// Add profiling machinery to the annotated function.
/// Requires a value that implements profiling::ProfileBackend in order to
/// create a timer and write back profiling data.
///
/// The initialization of this value is passed as a parameter in the invocation
/// of the `gbl_profile` macro.
///
/// E.g.
///
/// #[gbl_profile(backend = MyProfilingBackend::(self.handle))]
/// fn expensive_func(&self){ ... }
///
#[proc_macro_attribute]
pub fn gbl_profile(args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemFn);
    let ItemFn { sig, vis, block, attrs } = input;
    let statements = block.stmts;
    let funcname = sig.ident.clone();
    let backend = parse_macro_input!(args as ProfileArgs);

    quote!(
        #(#attrs)*
        #vis #sig {
            use libgbl::profiling::{Profiler, ProfileBackend};
            let backend = #backend;
            let timer = backend.new_timer();
            let reporter = backend.reporter();
            let _profiler = Profiler::new(timer, reporter, stringify!(#funcname));

            #(#statements)*
        }
    )
    .into()
}
