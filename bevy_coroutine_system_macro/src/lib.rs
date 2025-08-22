use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, FnArg, ItemFn, Pat, ReturnType};

/// 协程系统的过程宏
/// 
/// # Example
/// ```rust
/// #[coroutine_system]
/// fn my_system(mut commands: Commands, query: Query<&mut Transform>) {
///     for mut transform in query.iter_mut() {
///         transform.translation.x += 1.0;
///     }
///     
///     // 支持原生 yield 语法
///     yield sleep(Duration::from_secs(1));
///     
///     for mut transform in query.iter_mut() {
///         transform.translation.y += 1.0;
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn coroutine_system(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    
    // 解析函数信息
    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;
    let _fn_generics = &input_fn.sig.generics;
    let fn_block = &input_fn.block;
    
    // 检查返回类型（必须是unit）
    match &input_fn.sig.output {
        ReturnType::Default => {},
        _ => {
            return syn::Error::new_spanned(
                &input_fn.sig.output,
                "coroutine_system functions must not have a return type"
            )
            .to_compile_error()
            .into();
        }
    }
    
    // 收集SystemParam参数
    let mut params = Vec::new();
    let mut param_names = Vec::new();
    let mut param_types: Vec<syn::Type> = Vec::new();
    let mut lifetime_req = LifetimeRequirement::none();
    
    for arg in &input_fn.sig.inputs {
        match arg {
            FnArg::Receiver(_) => {
                return syn::Error::new_spanned(
                    arg,
                    "coroutine_system functions cannot have self parameters"
                )
                .to_compile_error()
                .into();
            }
            FnArg::Typed(pat_type) => {
                params.push(pat_type);
                
                // 提取参数名
                if let Pat::Ident(pat_ident) = &*pat_type.pat {
                    param_names.push(&pat_ident.ident);
                } else {
                    return syn::Error::new_spanned(
                        &pat_type.pat,
                        "coroutine_system only supports simple parameter patterns"
                    )
                    .to_compile_error()
                    .into();
                }
                
                // 分析生命周期需求
                lifetime_req.merge(analyze_lifetime_requirements(&pat_type.ty));
                
                // 提取参数类型并添加生命周期（如果需要）
                let ty = add_lifetimes_to_type(&pat_type.ty);
                param_types.push(ty);
            }
        }
    }
    
    // 生成SystemParam组合结构名（转换为CamelCase）
    let struct_name_str = format!("{}Params", fn_name);
    let struct_name_str = struct_name_str.chars().enumerate().map(|(i, c)| {
        if i == 0 || struct_name_str.chars().nth(i - 1).unwrap() == '_' {
            c.to_uppercase().collect::<String>()
        } else if c == '_' {
            String::new()
        } else {
            c.to_string()
        }
    }).collect::<String>();
    let params_struct_name = format_ident!("{}", struct_name_str);
    
    // 根据生命周期需求生成结构体
    // 注意：Bevy的SystemParam总是需要'w和's，即使未使用也需要保留
    let params_struct = if lifetime_req.needs_w && lifetime_req.needs_s {
        // 两个生命周期都被使用
        quote! {
            #[derive(::bevy::ecs::system::SystemParam)]
            struct #params_struct_name<'w, 's> {
                #(#param_names: #param_types,)*
            }
        }
    } else {
        // 至少有一个生命周期未被使用，需要添加PhantomData
        let phantom_type = if !lifetime_req.needs_w && !lifetime_req.needs_s {
            // 两个都未使用
            quote! { ::std::marker::PhantomData<(&'w (), &'s ())> }
        } else if !lifetime_req.needs_w {
            // 只有'w未使用
            quote! { ::std::marker::PhantomData<&'w ()> }
        } else {
            // 只有's未使用
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
    
    // 转换函数体，处理yield表达式
    let transformed_body = transform_function_body(fn_block, &param_names, &params_struct_name);
    
    // 生成包装函数（确保只使用<'w, 's>生命周期）
    let wrapper_fn = quote! {
        #[allow(unused_variables)]
        #fn_vis fn #fn_name<'w, 's>(
            params: #params_struct_name<'w, 's>,
            mut __task: ::bevy::prelude::Local<
                ::bevy_coroutine_system::Task<
                    ::bevy_coroutine_system::TaskInput<#params_struct_name<'static, 'static>>
                >
            >,
            mut __running_task: ::bevy::prelude::ResMut<::bevy_coroutine_system::RunningTask>,
        ) {
            use ::std::ops::Coroutine;
            use ::std::pin::Pin;
            use ::std::ptr::NonNull;
            use ::std::task::{Context, Poll, Waker};
            
            // 初始化协程
            if __task.coroutine.is_none() {
                __task.coroutine = Some(Box::pin(
                    #[coroutine]
                    move |mut __coroutine_input: ::bevy_coroutine_system::TaskInput<#params_struct_name<'static, 'static>>| {
                        #transformed_body
                    }
                ));
                
                __running_task.systems.insert(#fn_name::id(), ());
            }
            
            // 处理异步结果
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
                        return;
                    }
                }
            }
            
            // 创建输入
            let __coroutine_input = ::bevy_coroutine_system::TaskInput {
                data_ptr: Some(unsafe { NonNull::new_unchecked(&params as *const _ as *mut _) }),
                async_result,
            };
            
            // 恢复协程
            if let Some(coroutine) = &mut __task.coroutine {
                match coroutine.as_mut().resume(__coroutine_input) {
                    ::std::ops::CoroutineState::Yielded(output) => {
                        __task.fut = Some(output);
                    }
                    ::std::ops::CoroutineState::Complete(()) => {
                        __task.init = false;
                        __task.coroutine = None;
                        __running_task.systems.remove(#fn_name::id());
                    }
                }
            }
        }
    };
    
    // 生成模块和ID函数
    let fn_name_str = fn_name.to_string();
    let id_fn = quote! {
        pub mod #fn_name {
            /// 获取协程系统的唯一标识符
            /// 
            /// 返回格式为 "module_path::function_name" 的唯一标识符
            pub fn id() -> &'static str {
                // 使用一个独特的常量来避免函数名重复
                const ID: &str = concat!(module_path!(), "::", #fn_name_str);
                ID
            }
        }
    };
    
    // 组合输出
    let output = quote! {
        #params_struct
        
        #wrapper_fn
        
        #id_fn
    };
    
    output.into()
}

/// 转换函数体，处理yield表达式
fn transform_function_body(
    block: &syn::Block,
    param_names: &[&syn::Ident],
    _params_struct_name: &syn::Ident,
) -> proc_macro2::TokenStream {
    // 生成参数获取代码
    let get_params = quote! {
        let params = __coroutine_input.data_mut();
        #(let #param_names = &mut params.#param_names;)*
    };
    
    // 遍历所有语句并处理 yield
    let mut new_stmts = Vec::new();
    
    // 首先添加初始的参数获取
    new_stmts.push(quote! { #get_params });
    
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Local(local) => {
                // 处理 let x = yield expr;
                if let Some(init) = &local.init {
                    if let syn::Expr::Yield(yield_expr) = &*init.expr {
                        if let Some(yielded_expr) = &yield_expr.expr {
                            let pat = &local.pat;
                            
                            // 生成新的语句序列
                            new_stmts.push(quote! {
                                __coroutine_input = yield #yielded_expr;
                            });
                            new_stmts.push(quote! {
                                let #pat = __coroutine_input.result();
                            });
                            // yield 后重新获取参数
                            new_stmts.push(quote! { #get_params });
                            continue;
                        }
                    } else if let syn::Expr::Macro(mac_expr) = &*init.expr {
                        // 兼容 yield_async! 宏
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
                // 其他情况保持原样
                new_stmts.push(quote! { #stmt });
            }
            syn::Stmt::Expr(expr, semi) => {
                // 处理独立的 yield expr 语句
                if let syn::Expr::Yield(yield_expr) = expr {
                    if let Some(yielded_expr) = &yield_expr.expr {
                        new_stmts.push(quote! {
                            __coroutine_input = yield #yielded_expr;
                        });
                        new_stmts.push(quote! {
                            // 丢弃结果，不指定具体类型
                            let _ = __coroutine_input.async_result.take();
                        });
                        new_stmts.push(quote! { #get_params });
                        
                        if semi.is_some() {
                            // 保持原有的分号
                        }
                        continue;
                    }
                } else if let syn::Expr::Macro(mac_expr) = expr {
                    // 兼容 yield_async! 宏
                    if is_yield_macro(&mac_expr.mac) {
                        if let Ok(inner_expr) = mac_expr.mac.parse_body::<syn::Expr>() {
                            new_stmts.push(quote! {
                                __coroutine_input = yield #inner_expr;
                            });
                            new_stmts.push(quote! {
                                // 丢弃结果，不指定具体类型
                                let _ = __coroutine_input.async_result.take();
                            });
                            new_stmts.push(quote! { #get_params });
                            continue;
                        }
                    }
                }
                // 其他情况保持原样
                new_stmts.push(quote! { #stmt });
            }
            _ => {
                new_stmts.push(quote! { #stmt });
            }
        }
    }
    
    quote! {
        #(#new_stmts)*
    }
}

/// 检查是否是yield!宏
fn is_yield_macro(mac: &syn::Macro) -> bool {
    mac.path.segments.last().map(|seg| {
        seg.ident == "yield" || seg.ident == "yield_async"
    }).unwrap_or(false)
}

/// 检查类型需要哪些生命周期参数
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LifetimeRequirement {
    needs_w: bool,
    needs_s: bool,
}

impl LifetimeRequirement {
    fn none() -> Self {
        Self { needs_w: false, needs_s: false }
    }
    
    fn merge(&mut self, other: Self) {
        self.needs_w |= other.needs_w;
        self.needs_s |= other.needs_s;
    }
}

/// 分析类型并返回其生命周期需求
fn analyze_lifetime_requirements(ty: &syn::Type) -> LifetimeRequirement {
    use syn::{Type, PathArguments};
    
    match ty {
        Type::Reference(type_ref) => {
            // 引用类型继承其内部类型的生命周期需求
            analyze_lifetime_requirements(&type_ref.elem)
        }
        
        Type::Tuple(type_tuple) => {
            // 元组类型合并所有元素的生命周期需求
            let mut req = LifetimeRequirement::none();
            for elem in &type_tuple.elems {
                req.merge(analyze_lifetime_requirements(elem));
            }
            req
        }
        
        Type::Path(type_path) => {
            let mut req = LifetimeRequirement::none();
            
            // 检查路径中的每个段
            for segment in &type_path.path.segments {
                let ident_str = segment.ident.to_string();
                
                // 根据类型名确定生命周期需求
                match ident_str.as_str() {
                    "Commands" => req.merge(LifetimeRequirement { needs_w: true, needs_s: true }),
                    "Query" => req.merge(LifetimeRequirement { needs_w: true, needs_s: true }),
                    "Res" | "ResMut" => req.merge(LifetimeRequirement { needs_w: true, needs_s: false }),
                    "Local" => req.merge(LifetimeRequirement { needs_w: false, needs_s: true }),
                    "EventReader" | "EventWriter" => req.merge(LifetimeRequirement { needs_w: true, needs_s: true }),
                    _ => {}
                }
                
                // 递归分析泛型参数
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

/// 为已知的Bevy类型添加生命周期参数
fn add_lifetimes_to_type(ty: &syn::Type) -> syn::Type {
    use syn::{Type, TypePath, PathArguments, GenericArgument, AngleBracketedGenericArguments};
    use syn::parse_quote;
    
    match ty {
        // 处理引用类型 &T 或 &mut T
        Type::Reference(type_ref) => {
            let elem = add_lifetimes_to_type(&type_ref.elem);
            let lifetime = type_ref.lifetime.clone()
                .unwrap_or_else(|| parse_quote! { 'static });
            
            Type::Reference(syn::TypeReference {
                and_token: type_ref.and_token,
                lifetime: Some(lifetime),
                mutability: type_ref.mutability,
                elem: Box::new(elem),
            })
        }
        
        // 处理元组类型 (A, B, C)
        Type::Tuple(type_tuple) => {
            let elems = type_tuple.elems.iter()
                .map(|elem| add_lifetimes_to_type(elem))
                .collect();
            
            Type::Tuple(syn::TypeTuple {
                paren_token: type_tuple.paren_token,
                elems,
            })
        }
        
        // 处理路径类型 A::B::C<T>
        Type::Path(type_path) => {
            let mut path = type_path.path.clone();
            
            // 处理路径中的每个段
            for segment in &mut path.segments {
                let ident_str = segment.ident.to_string();
                
                // 检查是否是需要生命周期的Bevy类型
                let needs_lifetimes = match ident_str.as_str() {
                    "Commands" | "Query" | "Res" | "ResMut" | "Local" | "EventReader" | "EventWriter" => true,
                    _ => false,
                };
                
                match &mut segment.arguments {
                    PathArguments::None => {
                        if needs_lifetimes {
                            // 为这些类型添加生命周期
                            if ident_str == "Res" || ident_str == "ResMut" {
                                // Res 和 ResMut 只需要一个生命周期
                                segment.arguments = PathArguments::AngleBracketed(
                                    parse_quote! { <'w> }
                                );
                            } else {
                                segment.arguments = PathArguments::AngleBracketed(
                                    parse_quote! { <'w, 's> }
                                );
                            }
                        }
                    }
                    PathArguments::AngleBracketed(args) => {
                        let mut new_args = args.clone();
                        
                        // 递归处理所有泛型参数
                        new_args.args = new_args.args.into_iter().map(|arg| {
                            match arg {
                                GenericArgument::Type(ty) => {
                                    GenericArgument::Type(add_lifetimes_to_type(&ty))
                                }
                                other => other,
                            }
                        }).collect();
                        
                        // 如果是需要生命周期的类型，在开头插入生命周期
                        if needs_lifetimes {
                            let mut final_args = syn::punctuated::Punctuated::new();
                            
                            // 插入生命周期
                            if ident_str == "Res" || ident_str == "ResMut" {
                                final_args.push(parse_quote! { 'w });
                            } else if ident_str == "Query" {
                                final_args.push(parse_quote! { 'w });
                                final_args.push(parse_quote! { 's });
                            }
                            
                            // 添加处理后的参数
                            final_args.extend(new_args.args);
                            
                            segment.arguments = PathArguments::AngleBracketed(
                                AngleBracketedGenericArguments {
                                    colon2_token: args.colon2_token,
                                    lt_token: args.lt_token,
                                    args: final_args,
                                    gt_token: args.gt_token,
                                }
                            );
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
        
        // 其他类型保持不变
        _ => (*ty).clone(),
    }
}
