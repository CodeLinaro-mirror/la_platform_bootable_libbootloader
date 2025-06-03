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
use quote::{quote, ToTokens};
use syn::{
    parse::{Parse, ParseBuffer},
    parse_macro_input,
    token::Comma,
    Expr, ItemFn, MetaNameValue, Result,
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
        let setup = input.parse::<MetaNameValue>()?;

        if setup.path.is_ident("backend") {
            Ok(ProfileArgs { value: setup.value })
        } else {
            Err(input.error(format!("Unexpected setup var in macro: {:#?}", setup.path)))
        }
    }
}

struct InlineProfile {
    expr: Expr,
    backend: ProfileArgs,
}

impl Parse for InlineProfile {
    fn parse(input: &ParseBuffer) -> Result<Self> {
        let backend = input.parse::<ProfileArgs>()?;
        input.parse::<Comma>()?;
        let expr = input.parse::<Expr>()?;

        if !input.is_empty() {
            return Err(input.error(format!("Unexpected macro content: {}", input)));
        }

        Ok(InlineProfile { expr, backend })
    }
}

/// Add profiling machinery to the annotated function.
/// Requires a value that implements profiling::ProfileBackend in order to
/// create a timer and write back profiling data.
///
/// The initialization of this value is passed as a parameter in the invocation
/// of the `profile` macro.
///
/// E.g.
///
/// ```
/// #[profile(backend = MyProfilingBackend::(self.handle))]
/// fn expensive_func(&self){ ... }
/// ```
#[proc_macro_attribute]
pub fn profile(args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemFn);
    let ItemFn { sig, vis, block, attrs } = input;
    let statements = block.stmts;
    let funcname = sig.ident.clone();
    let backend = parse_macro_input!(args as ProfileArgs);

    quote!(
        #(#attrs)*
        #vis #sig {
            use libprofile::{Profiler, ProfileBackend};
            let backend = #backend;
            let _profiler = Profiler::new(
                backend.new_timer(),
                backend.reporter(),
                stringify!(#funcname)
            );

            #(#statements)*
        }
    )
    .into()
}

/// Add profiling machinery to the wrapped expression.
/// Requires a value that implements profiling::ProfileBackend in order to
/// create a timer and write back profiling data.
///
/// The initialization of this value is passed as a parameter in the invocation
/// of the `profile_expr` macro.
///
/// E.g.
/// ```
/// fn function(&self) {
///     self.unimportant_setup();
///     let val = profile_expr!(backend = self.backend(), expensive_func(self.val));
///     self.unimportant_teardown(v);
/// }
/// ```
///
/// It can be useful to profile individual expressions (usually function calls) if
/// a) fine grained analysis is required to profile hotspots in a large function, or
/// b) the args of the target function don't include a means of generating a backend
///    and can't be modified, or
/// c) ownership issues are preventing the entire function from being profiled
///    using the #[profile] macro.
///
/// Warning: when logging, `profile_expr` includes the full textual representation
///          of the expression under profile, e.g.
///          ```
///          let val = profile_expr!(
///            backend = ...,
///            {
///              // Enormous block
///            },
///          );
///          ```
///          Will print the entire enormous block.
///          It's a good idea to keep the expression being profiled small.
#[proc_macro]
pub fn profile_expr(item: TokenStream) -> TokenStream {
    let InlineProfile { expr, backend } = parse_macro_input!(item as InlineProfile);

    quote!(
        {
            use libprofile::{Profiler, ProfileBackend};
            let backend = #backend;
            let _profiler = Profiler::new(
                backend.new_timer(),
                backend.reporter(),
                concat!(stringify!(#expr), " @ ", line!()),
            );

            #expr
        }
    )
    .into()
}
