//! Procedural macro for the Bevy coroutine system
//!
//! This crate provides the `#[coroutine_system]` macro, which transforms ordinary Bevy systems into coroutine-enabled systems.
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use bevy_coroutine_system::coroutine_system;
//!
//! #[coroutine_system]
//! fn my_system(query: Query<&mut Transform>) {
//!     // System code...
//! }
//! ```

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{FnArg, ItemFn, Pat, ReturnType, parse_macro_input};

/// Procedural macro for coroutine systems
///
/// This macro transforms a regular Bevy system function into a coroutine-enabled system, allowing it to execute across multiple frames.
///
/// # How to use
///
/// Apply this macro to any function that matches Bevy's system signature:
///
/// ```rust,ignore
/// #[coroutine_system]
/// fn my_system(mut commands: Commands, query: Query<&mut Transform>) {
///     for mut transform in query.iter_mut() {
///         transform.translation.x += 1.0;
///     }
///
///     // Supports native yield syntax
///     yield sleep(Duration::from_secs(1));
///
///     for mut transform in query.iter_mut() {
///         transform.translation.y += 1.0;
///     }
/// }
/// ```
///
/// # Supported parameter types
///
/// - Any type that implements `SystemParam`
/// - Including but not limited to: `Commands`, `Query`, `Res`, `ResMut`, `Local`, etc.
///
/// # Limitations
///
/// - The function must return `()` (unit type)
/// - Requires Rust nightly and corresponding feature flags
#[proc_macro_attribute]
pub fn coroutine_system(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);

    // Parse function info
    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;
    let _fn_generics = &input_fn.sig.generics;
    let fn_block = &input_fn.block;

    // Check return type (must be unit)
    match &input_fn.sig.output {
        ReturnType::Default => {}
        _ => {
            return syn::Error::new_spanned(
                &input_fn.sig.output,
                "coroutine_system functions must not have a return type",
            )
            .to_compile_error()
            .into();
        }
    }

    // Collect SystemParam parameters
    let mut params = Vec::new();
    let mut param_names = Vec::new();
    let mut param_types: Vec<syn::Type> = Vec::new();
    let mut lifetime_req = LifetimeRequirement::none();

    for arg in &input_fn.sig.inputs {
        match arg {
            FnArg::Receiver(_) => {
                return syn::Error::new_spanned(
                    arg,
                    "coroutine_system functions cannot have self parameters",
                )
                .to_compile_error()
                .into();
            }
            FnArg::Typed(pat_type) => {
                params.push(pat_type);

                // Extract parameter name
                if let Pat::Ident(pat_ident) = &*pat_type.pat {
                    param_names.push(&pat_ident.ident);
                } else {
                    return syn::Error::new_spanned(
                        &pat_type.pat,
                        "coroutine_system only supports simple parameter patterns",
                    )
                    .to_compile_error()
                    .into();
                }

                // Analyze lifetime requirements
                lifetime_req.merge(analyze_lifetime_requirements(&pat_type.ty));

                // Extract param type and add lifetimes (if needed)
                let ty = add_lifetimes_to_type(&pat_type.ty);
                param_types.push(ty);
            }
        }
    }

    // Generate SystemParam aggregate struct name (CamelCase)
    let struct_name_str = format!("{}Params", fn_name);
    let struct_name_str = struct_name_str
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if i == 0 || struct_name_str.chars().nth(i - 1).unwrap() == '_' {
                c.to_uppercase().collect::<String>()
            } else if c == '_' {
                String::new()
            } else {
                c.to_string()
            }
        })
        .collect::<String>();
    let params_struct_name = format_ident!("{}", struct_name_str);

    // Generate struct based on lifetime requirements
    // Note: Bevy's SystemParam always needs 'w and 's, keep them even if unused
    let params_struct = if lifetime_req.needs_w && lifetime_req.needs_s {
        // Both lifetimes are used
        quote! {
            #[derive(::bevy::ecs::system::SystemParam)]
            #[allow(dead_code)]
            struct #params_struct_name<'w, 's> {
                #(#param_names: #param_types,)*
            }
        }
    } else {
        // At least one lifetime is unused; add PhantomData
        let phantom_type = if !lifetime_req.needs_w && !lifetime_req.needs_s {
            // Neither is used
            quote! { ::std::marker::PhantomData<(&'w (), &'s ())> }
        } else if !lifetime_req.needs_w {
            // Only 'w is unused
            quote! { ::std::marker::PhantomData<&'w ()> }
        } else {
            // Only 's is unused
            quote! { ::std::marker::PhantomData<&'s ()> }
        };

        quote! {
            #[derive(::bevy::ecs::system::SystemParam)]
            struct #params_struct_name<'w, 's> {
                #(#param_names: #param_types,)*
                _phantom: #phantom_type,
            }
        }
    };

    // Transform the function body and handle yield expressions
    let transformed_body = transform_function_body(fn_block, &param_names, &params_struct_name);

    // Generate wrapper function (ensure only <'w, 's> are used)
    let wrapper_fn = quote! {
        #[allow(unused_variables)]
        #fn_vis fn #fn_name<'w, 's>(
            params: #params_struct_name<'w, 's>,
            mut __task: ::bevy::prelude::Local<
                ::bevy_coroutine_system::CoroutineTask<
                    ::bevy_coroutine_system::CoroutineTaskInput<#params_struct_name<'static, 'static>>
                >
            >,
            mut __running_task: ::bevy::prelude::ResMut<::bevy_coroutine_system::RunningCoroutines>,
        ) {
            use ::std::ops::Coroutine;
            use ::std::pin::Pin;
            use ::std::ptr::NonNull;
            use ::std::task::{Context, Poll, Waker};

            // Initialize coroutine
            if __task.coroutine.is_none() {
                __task.coroutine = Some(Box::pin(
                    #[coroutine]
                    move |mut __coroutine_input: ::bevy_coroutine_system::CoroutineTaskInput<#params_struct_name<'static, 'static>>| {
                        #transformed_body
                    }
                ));

                __running_task.systems.insert(#fn_name::id(), ());
            }

            // Loop until a pending async operation is encountered or the coroutine completes
            loop {
                // Handle async result
                let mut async_result = None;

                if let Some(fut) = &mut __task.fut {
                    let waker = Waker::noop();
                    let mut cx = Context::from_waker(&waker);
                    match fut.as_mut().poll(&mut cx) {
                        Poll::Ready(v) => {
                            async_result = Some(v);
                            __task.fut = None;
                        }
                        Poll::Pending => {
                            // Async operation not finished; exit loop and wait for next frame
                            return;
                        }
                    }
                }

                // Create input
                let __coroutine_input = ::bevy_coroutine_system::CoroutineTaskInput {
                    data_ptr: Some(unsafe { NonNull::new_unchecked(&params as *const _ as *mut _) }),
                    async_result,
                };

                // Resume coroutine
                if let Some(coroutine) = &mut __task.coroutine {
                    match coroutine.as_mut().resume(__coroutine_input) {
                        ::std::ops::CoroutineState::Yielded(output) => {
                            __task.fut = Some(output);
                            // Continue loop; check whether the newly yielded future completes immediately
                        }
                        ::std::ops::CoroutineState::Complete(()) => {
                            __task.coroutine = None;
                            __task.fut = None;
                            __running_task.systems.remove(#fn_name::id());
                            return;
                        }
                    }
                } else {
                    // No coroutine; exit loop
                    break;
                }
            }
        }
    };

    // Generate module and ID function
    let fn_name_str = fn_name.to_string();
    let id_fn = quote! {
        pub mod #fn_name {
            /// Get the unique identifier for the coroutine system
            ///
            /// Returns a unique identifier of the form "module_path::function_name"
            pub fn id() -> &'static str {
                // Use a dedicated constant to avoid function name collisions
                const ID: &str = concat!(module_path!(), "::", #fn_name_str);
                ID
            }
        }
    };

    // Compose output
    let output = quote! {
        #params_struct

        #wrapper_fn

        #id_fn
    };

    output.into()
}

/// Transform the function body and handle yield expressions
fn transform_function_body(
    block: &syn::Block,
    param_names: &[&syn::Ident],
    _params_struct_name: &syn::Ident,
) -> proc_macro2::TokenStream {
    // Generate parameter access code
    let get_params = quote! {
        let params = __coroutine_input.data_mut();
        #(let #param_names = &mut params.#param_names;)*
    };

    // First add the initial parameter access
    let mut new_stmts = vec![quote! { #get_params }];

    // Transform all statements
    let transformed_stmts = transform_statements(&block.stmts, &get_params);
    new_stmts.extend(transformed_stmts);

    quote! {
        #(#new_stmts)*
    }
}

/// Recursively transform a list of statements, handling all yield expressions
fn transform_statements(
    stmts: &[syn::Stmt],
    get_params: &proc_macro2::TokenStream,
) -> Vec<proc_macro2::TokenStream> {
    let mut new_stmts = Vec::new();

    for stmt in stmts {
        match stmt {
            syn::Stmt::Local(local) => {
                // Handle `let x = yield expr;`
                if let Some(init) = &local.init {
                    if let syn::Expr::Yield(yield_expr) = &*init.expr {
                        if let Some(yielded_expr) = &yield_expr.expr {
                            let pat = &local.pat;

                            // Generate new sequence of statements
                            new_stmts.push(quote! {
                                __coroutine_input = yield #yielded_expr;
                            });
                            new_stmts.push(quote! {
                                let #pat = __coroutine_input.result();
                            });
                            // Re-acquire parameters after yield
                            new_stmts.push(quote! { #get_params });
                            continue;
                        }
                    } else if let syn::Expr::Macro(mac_expr) = &*init.expr {
                        // Be compatible with the yield_async! macro
                        if is_yield_macro(&mac_expr.mac) {
                            if let Ok(inner_expr) = mac_expr.mac.parse_body::<syn::Expr>() {
                                let pat = &local.pat;

                                new_stmts.push(quote! {
                                    __coroutine_input = yield #inner_expr;
                                });
                                new_stmts.push(quote! {
                                    let #pat = __coroutine_input.result();
                                });
                                new_stmts.push(quote! { #get_params });
                                continue;
                            }
                        }
                    }
                }
                // Otherwise keep the statement as-is
                new_stmts.push(quote! { #stmt });
            }
            syn::Stmt::Expr(expr, semi) => {
                // Handle a standalone `yield expr` statement
                if let syn::Expr::Yield(yield_expr) = expr {
                    if let Some(yielded_expr) = &yield_expr.expr {
                        new_stmts.push(quote! {
                            __coroutine_input = yield #yielded_expr;
                        });
                        new_stmts.push(quote! {
                            // Discard the result without specifying a concrete type
                            let _ = __coroutine_input.async_result.take();
                        });
                        new_stmts.push(quote! { #get_params });

                        if semi.is_some() {
                            // Preserve the original semicolon
                        }
                        continue;
                    }
                } else if let syn::Expr::Macro(mac_expr) = expr {
                    // Be compatible with the yield_async! macro
                    if is_yield_macro(&mac_expr.mac) {
                        if let Ok(inner_expr) = mac_expr.mac.parse_body::<syn::Expr>() {
                            new_stmts.push(quote! {
                                __coroutine_input = yield #inner_expr;
                            });
                            new_stmts.push(quote! {
                                // Discard the result without specifying a concrete type
                                let _ = __coroutine_input.async_result.take();
                            });
                            new_stmts.push(quote! { #get_params });
                            continue;
                        }
                    }
                } else {
                    // Recursively process code blocks inside expressions
                    let transformed_expr = transform_expression(expr, get_params);
                    if semi.is_some() {
                        new_stmts.push(quote! { #transformed_expr; });
                    } else {
                        new_stmts.push(quote! { #transformed_expr });
                    }
                    continue;
                }
                // Otherwise keep as-is
                new_stmts.push(quote! { #stmt });
            }
            _ => {
                // Recursively process other kinds of statements
                let transformed_stmt = transform_statement(stmt, get_params);
                new_stmts.push(transformed_stmt);
            }
        }
    }

    new_stmts
}

/// Transform a single statement
fn transform_statement(
    stmt: &syn::Stmt,
    get_params: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    match stmt {
        syn::Stmt::Expr(expr, semi) => {
            let transformed_expr = transform_expression(expr, get_params);
            if semi.is_some() {
                quote! { #transformed_expr; }
            } else {
                quote! { #transformed_expr }
            }
        }
        _ => quote! { #stmt },
    }
}

/// Recursively transform expressions and handle nested blocks
fn transform_expression(
    expr: &syn::Expr,
    get_params: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    match expr {
        // Handle a code block
        syn::Expr::Block(block_expr) => {
            let transformed_stmts = transform_statements(&block_expr.block.stmts, get_params);
            quote! {
                {
                    #(#transformed_stmts)*
                }
            }
        }
        // Handle an if expression
        syn::Expr::If(if_expr) => {
            let cond = &if_expr.cond;
            let then_branch_stmts = transform_statements(&if_expr.then_branch.stmts, get_params);

            if let Some((_, else_branch)) = &if_expr.else_branch {
                let else_transformed = transform_expression(else_branch, get_params);
                quote! {
                    if #cond {
                        #(#then_branch_stmts)*
                    } else #else_transformed
                }
            } else {
                quote! {
                    if #cond {
                        #(#then_branch_stmts)*
                    }
                }
            }
        }
        // Handle a while loop
        syn::Expr::While(while_expr) => {
            let cond = &while_expr.cond;
            let body_stmts = transform_statements(&while_expr.body.stmts, get_params);
            quote! {
                while #cond {
                    #(#body_stmts)*
                }
            }
        }
        // Handle a loop
        syn::Expr::Loop(loop_expr) => {
            let body_stmts = transform_statements(&loop_expr.body.stmts, get_params);
            quote! {
                loop {
                    #(#body_stmts)*
                }
            }
        }
        // Handle a for loop
        syn::Expr::ForLoop(for_expr) => {
            let pat = &for_expr.pat;
            let iter = &for_expr.expr;
            let body_stmts = transform_statements(&for_expr.body.stmts, get_params);
            quote! {
                for #pat in #iter {
                    #(#body_stmts)*
                }
            }
        }
        // Handle a match expression
        syn::Expr::Match(match_expr) => {
            let matched = &match_expr.expr;
            let mut arms = Vec::new();

            for arm in &match_expr.arms {
                let pat = &arm.pat;
                let guard = arm.guard.as_ref().map(|(_, guard)| quote! { if #guard });
                let body = transform_expression(&arm.body, get_params);
                let comma = if arm.comma.is_some() {
                    quote! {,}
                } else {
                    quote! {}
                };

                arms.push(quote! {
                    #pat #guard => #body #comma
                });
            }

            quote! {
                match #matched {
                    #(#arms)*
                }
            }
        }
        // Other expressions remain unchanged
        _ => quote! { #expr },
    }
}

/// Check whether it is the yield! macro
fn is_yield_macro(mac: &syn::Macro) -> bool {
    mac.path
        .segments
        .last()
        .map(|seg| seg.ident == "yield" || seg.ident == "yield_async")
        .unwrap_or(false)
}

/// Determine which lifetimes a type requires
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LifetimeRequirement {
    needs_w: bool,
    needs_s: bool,
}

impl LifetimeRequirement {
    fn none() -> Self {
        Self {
            needs_w: false,
            needs_s: false,
        }
    }

    fn merge(&mut self, other: Self) {
        self.needs_w |= other.needs_w;
        self.needs_s |= other.needs_s;
    }
}

/// Analyze a type and return its lifetime requirements
fn analyze_lifetime_requirements(ty: &syn::Type) -> LifetimeRequirement {
    use syn::{PathArguments, Type};

    match ty {
        Type::Reference(type_ref) => {
            // Reference types inherit the lifetime requirements of their inner type
            analyze_lifetime_requirements(&type_ref.elem)
        }

        Type::Tuple(type_tuple) => {
            // Tuple types merge lifetime requirements of all elements
            let mut req = LifetimeRequirement::none();
            for elem in &type_tuple.elems {
                req.merge(analyze_lifetime_requirements(elem));
            }
            req
        }

        Type::Path(type_path) => {
            let mut req = LifetimeRequirement::none();

            // Inspect each segment in the path
            for segment in &type_path.path.segments {
                let ident_str = segment.ident.to_string();

                // Determine lifetime requirements based on the type name
                match ident_str.as_str() {
                    "Commands" => req.merge(LifetimeRequirement {
                        needs_w: true,
                        needs_s: true,
                    }),
                    "Query" => req.merge(LifetimeRequirement {
                        needs_w: true,
                        needs_s: true,
                    }),
                    "Res" | "ResMut" => req.merge(LifetimeRequirement {
                        needs_w: true,
                        needs_s: false,
                    }),
                    "Local" => req.merge(LifetimeRequirement {
                        needs_w: false,
                        needs_s: true,
                    }),
                    "EventWriter" => req.merge(LifetimeRequirement {
                        needs_w: true,
                        needs_s: false,
                    }),
                    "EventReader" => req.merge(LifetimeRequirement {
                        needs_w: true,
                        needs_s: true,
                    }),
                    _ => {}
                }

                // Recursively analyze generic arguments
                if let PathArguments::AngleBracketed(args) = &segment.arguments {
                    for arg in &args.args {
                        if let syn::GenericArgument::Type(inner_ty) = arg {
                            req.merge(analyze_lifetime_requirements(inner_ty));
                        }
                    }
                }
            }

            req
        }

        _ => LifetimeRequirement::none(),
    }
}

/// Add lifetime parameters to known Bevy types
fn add_lifetimes_to_type(ty: &syn::Type) -> syn::Type {
    use syn::parse_quote;
    use syn::{AngleBracketedGenericArguments, GenericArgument, PathArguments, Type, TypePath};

    match ty {
        // Handle reference types &T or &mut T
        Type::Reference(type_ref) => {
            let elem = add_lifetimes_to_type(&type_ref.elem);
            let lifetime = type_ref
                .lifetime
                .clone()
                .unwrap_or_else(|| parse_quote! { 'static });

            Type::Reference(syn::TypeReference {
                and_token: type_ref.and_token,
                lifetime: Some(lifetime),
                mutability: type_ref.mutability,
                elem: Box::new(elem),
            })
        }

        // Handle tuple types (A, B, C)
        Type::Tuple(type_tuple) => {
            let elems = type_tuple
                .elems
                .iter()
                .map(|elem| add_lifetimes_to_type(elem))
                .collect();

            Type::Tuple(syn::TypeTuple {
                paren_token: type_tuple.paren_token,
                elems,
            })
        }

        // Handle path types A::B::C<T>
        Type::Path(type_path) => {
            let mut path = type_path.path.clone();

            // Process each segment in the path
            for segment in &mut path.segments {
                let ident_str = segment.ident.to_string();

                // Check whether this Bevy type needs lifetimes
                let needs_lifetimes = match ident_str.as_str() {
                    "Commands" | "Query" => true,
                    "Local" => true,
                    "Res" | "ResMut" | "EventWriter" => true,
                    "EventReader" => true,
                    _ => false,
                };

                match &mut segment.arguments {
                    PathArguments::None => {
                        if needs_lifetimes {
                            // Add lifetimes for these types
                            if ident_str == "Res"
                                || ident_str == "ResMut"
                                || ident_str == "EventWriter"
                            {
                                // Res, ResMut and EventWriter need only one lifetime 'w
                                segment.arguments =
                                    PathArguments::AngleBracketed(parse_quote! { <'w> });
                            } else if ident_str == "Local" {
                                // Local needs only one lifetime 's
                                segment.arguments =
                                    PathArguments::AngleBracketed(parse_quote! { <'s> });
                            } else {
                                // Commands, Query, EventReader need two lifetimes
                                segment.arguments =
                                    PathArguments::AngleBracketed(parse_quote! { <'w, 's> });
                            }
                        }
                    }
                    PathArguments::AngleBracketed(args) => {
                        let mut new_args = args.clone();

                        // Recursively process all generic arguments
                        new_args.args = new_args
                            .args
                            .into_iter()
                            .map(|arg| match arg {
                                GenericArgument::Type(ty) => {
                                    GenericArgument::Type(add_lifetimes_to_type(&ty))
                                }
                                other => other,
                            })
                            .collect();

                        // If the type needs lifetimes, insert them at the beginning
                        if needs_lifetimes {
                            let mut final_args = syn::punctuated::Punctuated::new();

                            // Insert lifetimes
                            if ident_str == "Res"
                                || ident_str == "ResMut"
                                || ident_str == "EventWriter"
                            {
                                final_args.push(parse_quote! { 'w });
                            } else if ident_str == "Local" {
                                final_args.push(parse_quote! { 's });
                            } else if ident_str == "Query"
                                || ident_str == "Commands"
                                || ident_str == "EventReader"
                            {
                                final_args.push(parse_quote! { 'w });
                                final_args.push(parse_quote! { 's });
                            }

                            // Add the processed arguments
                            final_args.extend(new_args.args);

                            segment.arguments =
                                PathArguments::AngleBracketed(AngleBracketedGenericArguments {
                                    colon2_token: args.colon2_token,
                                    lt_token: args.lt_token,
                                    args: final_args,
                                    gt_token: args.gt_token,
                                });
                        } else {
                            segment.arguments = PathArguments::AngleBracketed(new_args);
                        }
                    }
                    _ => {}
                }
            }

            Type::Path(TypePath {
                qself: type_path.qself.clone(),
                path,
            })
        }

        // Other types remain unchanged
        _ => (*ty).clone(),
    }
}
