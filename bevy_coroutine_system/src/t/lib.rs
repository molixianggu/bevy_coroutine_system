use proc_macro::TokenStream;
use quote::{quote, quote_spanned};
use syn::{Expr, ItemFn, parse_macro_input, parse_quote, visit_mut::VisitMut, Stmt, Block, spanned::Spanned};
use proc_macro_error::proc_macro_error;

mod parse_utils;
use parse_utils::*;

mod variable_analysis;
use variable_analysis::*;

/// 协程系统宏，将包含 yield_op! 的函数转换为状态机实现
/// 
/// # 使用方法
/// ```rust
/// #[test_coroutine_system]
/// fn my_coroutine() {
///     println!("开始执行");
///     yield_op! { async_task() };
///     println!("异步任务完成");
/// }
/// ```
#[proc_macro_attribute]
#[proc_macro_error]
pub fn test_coroutine_system(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let transformed = transform_coroutine_function(input_fn);
    TokenStream::from(quote! { #transformed })
}

// ===== 协程转换核心逻辑 =====

/// 将协程函数转换为状态机实现（新版本，支持嵌套）
fn transform_coroutine_function_nested(mut func: ItemFn) -> proc_macro2::TokenStream {
    let func_name = func.sig.ident.clone();
    let state_enum_name =
        quote::format_ident!("__{}State__", to_pascal_case(&func_name.to_string()));

    // 使用新的yield追踪器分析函数体
    let mut yield_tracker = YieldIndexTracker::new();
    let block_segments = split_block_by_yields_nested(&func.block, &mut yield_tracker);
    
    // 收集所有yield点信息
    let mut yield_collector = NestedYieldCollector::new();
    yield_collector.collect_from_segments(&block_segments);
    
    let yield_count = yield_collector.yield_points.len();
    
    if yield_count == 0 {
        // 没有 yield，直接返回原函数
        return quote! { #func };
    }
    
    // 分析带类型注解的yield点（通过分析原始AST）
    let mut analyzer = YieldAnalyzer::new();
    analyzer.visit_item_fn_mut(&mut func);
    
    // 合并类型信息到收集的yield点
    for (i, yield_point) in yield_collector.yield_points.iter_mut().enumerate() {
        if let Some(return_type) = analyzer.yield_return_types.get(i) {
            yield_point.return_type = return_type.clone();
        }
        if let Some(pattern) = analyzer.yield_assignment_patterns.get(i) {
            yield_point.assignment_pattern = pattern.clone();
        }
    }
    
    // 分析变量作用域
    let mut var_analyzer = VariableScopeAnalyzer::new();
    let cross_yield_variables = var_analyzer.analyze_statements(&func.block.stmts);
    
    // 提取yield信息用于生成状态枚举
    let yield_return_types: Vec<_> = yield_collector.yield_points.iter()
        .map(|p| p.return_type.clone())
        .collect();
    
    // 生成状态枚举（包含变量存储）
    let state_enum = generate_state_enum_with_vars(
        &state_enum_name, 
        yield_count, 
        &yield_return_types,
        &cross_yield_variables
    );

    // 生成新的状态机函数
    let state_machine = generate_nested_state_machine_function(
        &func_name, 
        &state_enum_name,
        &func.sig.inputs,
        &block_segments,
        &yield_collector.yield_points,
        &cross_yield_variables
    );
    
    quote! {
        #state_enum
        #state_machine
    }
}

/// 生成支持嵌套块的状态机函数
fn generate_nested_state_machine_function(
    original_name: &syn::Ident,
    state_enum_name: &syn::Ident,
    original_inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    block_segments: &[BlockSegment],
    yield_points: &[YieldPointInfo],
    cross_yield_variables: &[VariableScope],
) -> proc_macro2::TokenStream {
    // 构建函数参数列表：原始参数 + 状态参数
    let mut inputs = original_inputs.clone();
    let state_param: syn::FnArg = parse_quote! { 
        mut __state__: bevy::ecs::system::Local<#state_enum_name>
    };
    inputs.push(state_param);
    
    // 生成Start状态的代码
    let mut current_yield_index = 0;
    let start_code = generate_nested_state_machine_code(
        block_segments,
        state_enum_name,
        cross_yield_variables,
        &mut current_yield_index,
    );
    
    // 生成yield状态的match arms
    let match_arms = generate_nested_yield_match_arms(
        yield_points,
        state_enum_name,
        block_segments,
        cross_yield_variables,
    );
    
    quote! {
        fn #original_name(#inputs) {
            loop {
                let mut __current_state__ = std::mem::replace(&mut *__state__, #state_enum_name::Done);
                match __current_state__ {
                    #state_enum_name::Start => {
                        #start_code
                    },
                    #(#match_arms)*
                    #state_enum_name::Done => {
                        return;
                    },
                }
            }
        }
    }
}

/// 生成嵌套yield状态的match arms
fn generate_nested_yield_match_arms(
    yield_points: &[YieldPointInfo],
    state_enum_name: &syn::Ident,
    _block_segments: &[BlockSegment],
    cross_yield_variables: &[VariableScope],
) -> Vec<proc_macro2::TokenStream> {
    yield_points.iter().enumerate().map(|(i, yield_point)| {
        let variant_name = quote::format_ident!("Yield{}", i);
        
        // 收集在这个yield点保存的变量
        let saved_vars: Vec<_> = cross_yield_variables
            .iter()
            .filter(|scope| {
                scope.crossed_yields.contains(&i) && 
                !scope.is_shadowed &&
                scope.last_use_yield_index.map_or(true, |last| last >= i)
            })
            .map(|scope| {
                let var_name = &scope.variable.name;
                let is_mut = if scope.variable.is_mut { quote! { mut } } else { quote! {} };
                (var_name, is_mut)
            })
            .collect();
        
        // 生成match模式
        let match_pattern = if saved_vars.is_empty() {
            quote! {
                #state_enum_name::#variant_name { _io: mut _io }
            }
        } else {
            let var_patterns: Vec<_> = saved_vars.iter()
                .map(|(name, is_mut)| quote! { #is_mut #name })
                .collect();
            quote! {
                #state_enum_name::#variant_name { _io: mut _io, #(#var_patterns,)* }
            }
        };
        
        // 生成pending恢复代码
        let pending_restore = if saved_vars.is_empty() {
            quote! {
                *__state__ = #state_enum_name::#variant_name { _io };
                return;
            }
        } else {
            let var_names: Vec<_> = saved_vars.iter()
                .map(|(name, _)| quote! { #name })
                .collect();
            quote! {
                *__state__ = #state_enum_name::#variant_name { _io, #(#var_names,)* };
                return;
            }
        };
        
        // 生成轮询处理代码
        let poll_handling = if let Some(pattern) = &yield_point.assignment_pattern {
            if yield_point.return_type.is_some() {
                quote! {
                    let #pattern = match _io.as_mut().poll(&mut std::task::Context::from_waker(&std::task::Waker::noop())) {
                        std::task::Poll::Ready(v) => v,
                        std::task::Poll::Pending => {
                            #pending_restore
                        }
                    };
                }
            } else {
                quote! {
                    let __boxed_result__ = match _io.as_mut().poll(&mut std::task::Context::from_waker(&std::task::Waker::noop())) {
                        std::task::Poll::Ready(v) => v,
                        std::task::Poll::Pending => {
                            #pending_restore
                        }
                    };
                    let #pattern = *__boxed_result__.downcast::<_>().expect("Type mismatch in yield result");
                }
            }
        } else {
            quote! {
                match _io.as_mut().poll(&mut std::task::Context::from_waker(&std::task::Waker::noop())) {
                    std::task::Poll::Ready(_) => (),
                    std::task::Poll::Pending => {
                        #pending_restore
                    }
                }
            }
        };
        
        // TODO: 生成yield之后的代码
        // 这是复杂的部分，需要知道yield在代码中的位置
        // 暂时简化处理：完成后转到Done状态
        let next_state = if i + 1 < yield_points.len() {
            let next_variant = quote::format_ident!("Yield{}", i + 1);
            quote! {
                // TODO: 应该继续执行yield之后的代码
                *__state__ = #state_enum_name::#next_variant { 
                    _io: Box::pin(async { Box::new(()) as Box<dyn std::any::Any> }),
                };
            }
        } else {
            quote! {
                *__state__ = #state_enum_name::Done;
            }
        };
        
        quote! {
            #match_pattern => {
                #poll_handling
                #next_state
            },
        }
    }).collect()
}

/// 将协程函数转换为状态机实现
/// 
/// 转换流程：
/// 1. 分析函数体，查找所有 yield_op! 点
/// 2. 分析变量作用域和生命周期
/// 3. 生成状态枚举类型和变量存储结构
/// 4. 生成状态机函数
fn transform_coroutine_function(mut func: ItemFn) -> proc_macro2::TokenStream {
    let func_name = func.sig.ident.clone();
    let state_enum_name =
        quote::format_ident!("__{}State__", to_pascal_case(&func_name.to_string()));
    let _vars_struct_name = 
        quote::format_ident!("__{}Vars__", to_pascal_case(&func_name.to_string()));

    // 分析函数体，查找 yield 点
    let mut analyzer = YieldAnalyzer::new();
    analyzer.visit_item_fn_mut(&mut func);

    let yield_count = analyzer.yield_count;
    let yield_expressions = analyzer.yield_expressions;
    let yield_setup_statements = analyzer.yield_setup_statements;
    let yield_return_types = analyzer.yield_return_types;
    let yield_assignment_patterns = analyzer.yield_assignment_patterns;
    
    if yield_count == 0 {
        // 没有 yield，直接返回原函数
        return quote! { #func };
    }

    // 分析变量作用域
    let mut var_analyzer = VariableScopeAnalyzer::new();
    let cross_yield_variables = var_analyzer.analyze_statements(&func.block.stmts);

    // 生成状态枚举（包含变量存储）
    let state_enum = generate_state_enum_with_vars(
        &state_enum_name, 
        yield_count, 
        &yield_return_types,
        &cross_yield_variables
    );

    // 生成状态机函数
    let state_machine = generate_state_machine_function_with_inline_vars(
        &func_name, 
        &state_enum_name,
        yield_count, 
        &func.sig.inputs,
        yield_expressions,
        yield_setup_statements,
        yield_return_types,
        yield_assignment_patterns,
        &func.block,
        &cross_yield_variables
    );

    quote! {
        #state_enum
        #state_machine
    }
}

// ===== 新的嵌套块处理逻辑 =====

/// 收集所有yield点的信息（支持嵌套）
struct NestedYieldCollector {
    yield_points: Vec<YieldPointInfo>,
}

#[derive(Debug, Clone)]
struct YieldPointInfo {
    index: usize,
    expr: Expr,
    setup_stmts: Vec<Stmt>,
    return_type: Option<syn::Type>,
    assignment_pattern: Option<syn::Pat>,
}

impl NestedYieldCollector {
    fn new() -> Self {
        Self {
            yield_points: Vec::new(),
        }
    }
    
    fn collect_from_segments(&mut self, segments: &[BlockSegment]) {
        for segment in segments {
            match segment {
                BlockSegment::YieldPoint { yield_expr, setup_stmts, yield_index } => {
                    self.yield_points.push(YieldPointInfo {
                        index: *yield_index,
                        expr: yield_expr.clone(),
                        setup_stmts: setup_stmts.clone(),
                        return_type: None, // 会在后续分析中填充
                        assignment_pattern: None, // 会在后续分析中填充
                    });
                }
                BlockSegment::ComplexStatement { inner_segments, .. } => {
                    // 递归收集嵌套块中的yield点
                    self.collect_from_segments(inner_segments);
                }
                _ => {}
            }
        }
    }
}

/// 生成支持嵌套块的状态机代码
fn generate_nested_state_machine_code(
    segments: &[BlockSegment],
    state_enum_name: &syn::Ident,
    cross_yield_variables: &[VariableScope],
    current_yield_index: &mut usize,
) -> proc_macro2::TokenStream {
    let mut code = quote! {};
    
    for segment in segments {
        match segment {
            BlockSegment::Statements(stmts) => {
                // 直接添加语句
                code.extend(quote! { #(#stmts)* });
            }
            BlockSegment::YieldPoint { yield_expr, setup_stmts, yield_index } => {
                // 生成yield点的状态转换代码
                let next_state_name = quote::format_ident!("Yield{}", yield_index);
                
                // 收集需要保存的变量
                let vars_to_save: Vec<_> = cross_yield_variables
                    .iter()
                    .filter(|scope| {
                        scope.crossed_yields.contains(&yield_index) && 
                        !scope.is_shadowed
                    })
                    .map(|scope| {
                        let var_name = &scope.variable.name;
                        quote! { #var_name }
                    })
                    .collect();
                
                let state_transition = if vars_to_save.is_empty() {
                    quote! {
                        *__state__ = #state_enum_name::#next_state_name {
                            _io: Box::pin(#yield_expr),
                        };
                        return;
                    }
                } else {
                    quote! {
                        *__state__ = #state_enum_name::#next_state_name {
                            _io: Box::pin(#yield_expr),
                            #(#vars_to_save,)*
                        };
                        return;
                    }
                };
                
                code.extend(quote! {
                    #(#setup_stmts)*
                    #state_transition
                });
            }
            BlockSegment::ComplexStatement { stmt, inner_segments } => {
                // 处理包含yield的复杂语句
                let inner_code = generate_nested_state_machine_code(
                    inner_segments,
                    state_enum_name,
                    cross_yield_variables,
                    current_yield_index,
                );
                
                // 根据语句类型生成相应的代码
                match stmt {
                    Stmt::Expr(expr, semi) => {
                        // 替换表达式中的块
                        let transformed_expr = transform_expr_with_yields(
                            expr,
                            state_enum_name,
                            cross_yield_variables,
                            current_yield_index,
                            inner_segments,
                        );
                        if let Some(semi) = semi {
                            code.extend(quote! { #transformed_expr #semi });
                        } else {
                            code.extend(quote! { #transformed_expr });
                        }
                    }
                    _ => {
                        // 其他类型的语句暂时保持原样
                        code.extend(quote! { #stmt });
                    }
                }
            }
        }
    }
    
    code
}

/// 转换包含yield的表达式
fn transform_expr_with_yields(
    expr: &Expr,
    state_enum_name: &syn::Ident,
    cross_yield_variables: &[VariableScope],
    current_yield_index: &mut usize,
    segments: &[BlockSegment],
) -> proc_macro2::TokenStream {
    match expr {
        Expr::Block(_expr_block) => {
            let inner_code = generate_nested_state_machine_code(
                segments,
                state_enum_name,
                cross_yield_variables,
                current_yield_index,
            );
            quote! { { #inner_code } }
        }
        Expr::If(expr_if) => {
            // 处理if表达式
            let cond = &expr_if.cond;
            
            // 分析then分支
            let then_segments = split_block_by_yields_nested(&expr_if.then_branch, &mut YieldIndexTracker::new());
            let then_code = if then_segments.iter().any(|s| matches!(s, BlockSegment::YieldPoint { .. } | BlockSegment::ComplexStatement { .. })) {
                generate_nested_state_machine_code(
                    &then_segments,
                    state_enum_name,
                    cross_yield_variables,
                    current_yield_index,
                )
            } else {
                let then_branch = &expr_if.then_branch;
                quote! { #then_branch }
            };
            
            // 处理else分支
            let else_code = if let Some((_, else_expr)) = &expr_if.else_branch {
                match &**else_expr {
                    Expr::Block(block_expr) => {
                        let else_segments = split_block_by_yields_nested(&block_expr.block, &mut YieldIndexTracker::new());
                        if else_segments.iter().any(|s| matches!(s, BlockSegment::YieldPoint { .. } | BlockSegment::ComplexStatement { .. })) {
                            let else_code = generate_nested_state_machine_code(
                                &else_segments,
                                state_enum_name,
                                cross_yield_variables,
                                current_yield_index,
                            );
                            quote! {
                                else {
                                    #else_code
                                }
                            }
                        } else {
                            quote! { else #else_expr }
                        }
                    }
                    _ => quote! { else #else_expr }
                }
            } else {
                quote! {}
            };
            
            quote! {
                if #cond {
                    #then_code
                } #else_code
            }
        }
        Expr::Match(expr_match) => {
            // 处理match表达式
            let match_expr = &expr_match.expr;
            let mut arms = Vec::new();
            
            for arm in &expr_match.arms {
                let pat = &arm.pat;
                let guard = arm.guard.as_ref().map(|(_, guard)| quote! { if #guard });
                
                // 分析arm body
                match &*arm.body {
                    Expr::Block(block_expr) => {
                        let arm_segments = split_block_by_yields_nested(&block_expr.block, &mut YieldIndexTracker::new());
                        if arm_segments.iter().any(|s| matches!(s, BlockSegment::YieldPoint { .. } | BlockSegment::ComplexStatement { .. })) {
                            let arm_code = generate_nested_state_machine_code(
                                &arm_segments,
                                state_enum_name,
                                cross_yield_variables,
                                current_yield_index,
                            );
                            arms.push(quote! {
                                #pat #guard => { #arm_code }
                            });
                        } else {
                            let body = &arm.body;
                            arms.push(quote! {
                                #pat #guard => #body
                            });
                        }
                    }
                    _ => {
                        let body = &arm.body;
                        arms.push(quote! {
                            #pat #guard => #body
                        });
                    }
                }
            }
            
            quote! {
                match #match_expr {
                    #(#arms,)*
                }
            }
        }
        Expr::ForLoop(expr_for) => {
            // 处理for循环
            let pat = &expr_for.pat;
            let iter_expr = &expr_for.expr;
            
            let body_segments = split_block_by_yields_nested(&expr_for.body, &mut YieldIndexTracker::new());
            let body_code = if body_segments.iter().any(|s| matches!(s, BlockSegment::YieldPoint { .. } | BlockSegment::ComplexStatement { .. })) {
                generate_nested_state_machine_code(
                    &body_segments,
                    state_enum_name,
                    cross_yield_variables,
                    current_yield_index,
                )
            } else {
                let body = &expr_for.body;
                quote! { #body }
            };
            
            quote! {
                for #pat in #iter_expr {
                    #body_code
                }
            }
        }
        Expr::While(expr_while) => {
            // 处理while循环
            let cond = &expr_while.cond;
            
            let body_segments = split_block_by_yields_nested(&expr_while.body, &mut YieldIndexTracker::new());
            let body_code = if body_segments.iter().any(|s| matches!(s, BlockSegment::YieldPoint { .. } | BlockSegment::ComplexStatement { .. })) {
                generate_nested_state_machine_code(
                    &body_segments,
                    state_enum_name,
                    cross_yield_variables,
                    current_yield_index,
                )
            } else {
                let body = &expr_while.body;
                quote! { #body }
            };
            
            quote! {
                while #cond {
                    #body_code
                }
            }
        }
        _ => quote! { #expr }
    }
}

// ===== 状态机生成相关函数 =====

/// 生成状态枚举类型（带变量存储）
/// 
/// 状态包括：
/// - Start: 初始状态
/// - Yield{n}: 第n个yield点的状态，保存异步Future和需要的变量
/// - Done: 结束状态
fn generate_state_enum_with_vars(
    name: &syn::Ident, 
    yield_count: usize,
    yield_return_types: &[Option<syn::Type>],
    cross_yield_variables: &[VariableScope],
) -> proc_macro2::TokenStream {
    let mut variants = vec![quote! { Start }];

    // 为每个 yield 点生成状态
    for i in 0..yield_count {
        let variant_name = quote::format_ident!("Yield{}", i);
        
        // 根据返回类型生成对应的 Future 类型
        let future_type = if let Some(Some(return_type)) = yield_return_types.get(i) {
            quote! {
                std::pin::Pin<Box<dyn std::future::Future<Output = #return_type> + Send>>
            }
        } else {
            // 没有明确的返回类型注解，使用 Box<dyn Any> 以保持灵活性
            quote! {
                std::pin::Pin<Box<dyn std::future::Future<Output = Box<dyn std::any::Any>> + Send>>
            }
        };
        
        // 收集在这个yield点需要保存的变量
        let vars_to_save: Vec<_> = cross_yield_variables
            .iter()
            .filter(|scope| {
                // 变量需要在这个yield点保存
                scope.crossed_yields.contains(&i) && 
                !scope.is_shadowed &&
                // 确保变量在yield点之后还会被使用
                // 如果last_use_yield_index >= i，说明变量在第i个yield之后还被使用
                scope.last_use_yield_index.map_or(true, |last| last >= i)
            })
            .map(|scope| {
                let var_name = &scope.variable.name;
                let var_type = &scope.variable.ty;
                quote! { #var_name: #var_type }
            })
            .collect();
        
        // 生成带变量的状态变体
        if vars_to_save.is_empty() {
            variants.push(quote! {
                #variant_name {
                    _io: #future_type,
                }
            });
        } else {
            variants.push(quote! {
                #variant_name {
                    _io: #future_type,
                    #(#vars_to_save,)*
                }
            });
        }
    }

    variants.push(quote! { Done });

    quote! {
        enum #name {
            #(#variants,)*
        }
        
        impl Default for #name {
            fn default() -> Self {
                #name::Start
            }
        }
    }
}





/// 生成使用内联变量的状态机函数实现
/// 
/// 生成的函数会：
/// 1. 接受原始参数 + 状态Local参数
/// 2. 根据当前状态执行相应逻辑
/// 3. 变量直接保存在状态中
/// 4. 在yield点检查异步任务是否完成
fn generate_state_machine_function_with_inline_vars(
    original_name: &syn::Ident,
    state_enum_name: &syn::Ident,
    yield_count: usize,
    original_inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    yield_expressions: Vec<Expr>,
    yield_setup_statements: Vec<Vec<Stmt>>,
    yield_return_types: Vec<Option<syn::Type>>,
    yield_assignment_patterns: Vec<Option<syn::Pat>>,
    original_block: &Block,
    cross_yield_variables: &[VariableScope],
) -> proc_macro2::TokenStream {
    // 构建函数参数列表：原始参数 + 状态参数
    let mut inputs = original_inputs.clone();
    let state_param: syn::FnArg = parse_quote! { 
        mut __state__: bevy::ecs::system::Local<#state_enum_name>
    };
    inputs.push(state_param);

    // 生成Start状态的代码
    let start_code = generate_start_state_code_with_inline_vars(
        &state_enum_name, 
        original_block, 
        &yield_expressions, 
        &yield_setup_statements, 
        &yield_return_types,
        cross_yield_variables
    );
    
    // 生成yield状态的match arms
    let match_arms = generate_match_arms_with_inline_vars(
        yield_count, 
        state_enum_name, 
        &yield_expressions,
        &yield_setup_statements,
        &yield_return_types,
        &yield_assignment_patterns,
        original_block,
        cross_yield_variables
    );

    quote! {
        fn #original_name(#inputs) {
            loop {
                let mut __current_state__ = std::mem::replace(&mut *__state__, #state_enum_name::Done);
                match __current_state__ {
                    #state_enum_name::Start => {
                        #start_code
                    },
                    #(#match_arms)*
                    #state_enum_name::Done => {
                        // *__state__ = #state_enum_name::Start; // 重置到开始状态
                        return;
                    },
                }
            }
        }
    }
}






// ===== AST 分析器 =====

/// Yield分析器，用于遍历AST并提取yield点信息
struct YieldAnalyzer {
    yield_count: usize,
    yield_expressions: Vec<Expr>,
    yield_setup_statements: Vec<Vec<Stmt>>, // 每个yield前的设置语句
    yield_return_types: Vec<Option<syn::Type>>, // 每个yield点的返回类型
    yield_assignment_patterns: Vec<Option<syn::Pat>>, // 每个yield点的赋值模式 (如 let x: Type)
}

impl YieldAnalyzer {
    fn new() -> Self {
        Self {
            yield_count: 0,
            yield_expressions: Vec::new(),
            yield_setup_statements: Vec::new(),
            yield_return_types: Vec::new(),
            yield_assignment_patterns: Vec::new(),
        }
    }

}

/// 生成带内联变量的Start状态执行代码
/// 
/// Start状态执行第一个yield之前的所有代码，并在转换状态时将变量保存到状态中
fn generate_start_state_code_with_inline_vars(
    state_enum_name: &syn::Ident,
    original_block: &Block,
    yield_expressions: &[Expr],
    yield_setup_statements: &[Vec<Stmt>],
    yield_return_types: &[Option<syn::Type>],
    cross_yield_variables: &[VariableScope],
) -> proc_macro2::TokenStream {
    let segments = split_block_by_yields(original_block);
    let statements_before_yield = &segments[0];
    
    // 获取第一个yield的设置语句和表达式
    let first_yield_setup = yield_setup_statements.get(0)
        .map(|stmts| stmts.as_slice())
        .unwrap_or(&[]);
    let first_yield_expr = yield_expressions.get(0)
        .cloned()
        .unwrap_or_else(|| parse_quote! { () });
    
    // 根据是否有类型注解决定如何创建 Future
    let expr_span = first_yield_expr.span();
    let future_creation = if let Some(Some(_)) = yield_return_types.get(0) {
        quote_spanned! {expr_span=> Box::pin(#first_yield_expr) }
    } else {
        quote_spanned! {expr_span=> 
            Box::pin(async move {
                Box::new(#first_yield_expr.await) as Box<dyn std::any::Any>
            })
        }
    };
    
    // 收集第一个yield点需要保存的变量
    let vars_to_save: Vec<_> = cross_yield_variables
        .iter()
        .filter(|scope| {
            scope.crossed_yields.contains(&0) && 
            !scope.is_shadowed
        })
        .map(|scope| {
            let var_name = &scope.variable.name;
            quote! { #var_name }
        })
        .collect();
    
    let state_transition = if vars_to_save.is_empty() {
        quote! {
            *__state__ = #state_enum_name::Yield0 {
                _io: #future_creation,
            };
        }
    } else {
        quote! {
            *__state__ = #state_enum_name::Yield0 {
                _io: #future_creation,
                #(#vars_to_save,)*
            };
        }
    };
    
    quote! {
        #(#statements_before_yield)*
        #(#first_yield_setup)*
        #state_transition
    }
}


/// 生成带内联变量的yield状态匹配分支
fn generate_match_arms_with_inline_vars(
    yield_count: usize,
    state_enum_name: &syn::Ident,
    yield_expressions: &[Expr],
    yield_setup_statements: &[Vec<Stmt>],
    yield_return_types: &[Option<syn::Type>],
    yield_assignment_patterns: &[Option<syn::Pat>],
    original_block: &Block,
    cross_yield_variables: &[VariableScope],
) -> Vec<proc_macro2::TokenStream> {
    let segments = split_block_by_yields(original_block);
    
    (0..yield_count)
        .map(|i| {
            let variant_name = quote::format_ident!("Yield{}", i);
            
            // 收集在这个yield点保存的变量
            let saved_vars: Vec<_> = cross_yield_variables
                .iter()
                .filter(|scope| {
                    scope.crossed_yields.contains(&i) && 
                    !scope.is_shadowed &&
                    scope.last_use_yield_index.map_or(true, |last| last >= i)
                })
                .map(|scope| {
                    let var_name = &scope.variable.name;
                    let var_type = &scope.variable.ty;
                    let is_mut = if scope.variable.is_mut { quote! { mut } } else { quote! {} };
                    (var_name, var_type, is_mut)
                })
                .collect();
            
            // 生成match模式，直接使用变量名
            let match_pattern = if saved_vars.is_empty() {
                quote! {
                    #state_enum_name::#variant_name { _io: mut _io }
                }
            } else {
                let var_patterns: Vec<_> = saved_vars.iter()
                    .map(|(name, _, is_mut)| {
                        // 如果变量是可变的，在模式匹配时也需要mut
                        quote! { #is_mut #name }
                    })
                    .collect();
                quote! {
                    #state_enum_name::#variant_name { _io: mut _io, #(#var_patterns,)* }
                }
            };
            
            // 根据是否有赋值模式和返回类型生成不同的代码
            // 需要在pending时恢复状态
            let pending_restore = if saved_vars.is_empty() {
                quote! {
                    *__state__ = #state_enum_name::#variant_name {
                        _io,
                    };
                    return;
                }
            } else {
                let var_names: Vec<_> = saved_vars.iter()
                    .map(|(name, _, _)| quote! { #name })
                    .collect();
                quote! {
                    *__state__ = #state_enum_name::#variant_name {
                        _io,
                        #(#var_names,)*
                    };
                    return;
                }
            };
            
            let poll_handling = if let Some(Some(pattern)) = yield_assignment_patterns.get(i) {
                let pattern_span = pattern.span();
                if let Some(Some(_)) = yield_return_types.get(i) {
                    quote_spanned! {pattern_span=>
                        let #pattern = match _io.as_mut().poll(&mut std::task::Context::from_waker(&std::task::Waker::noop())) {
                            std::task::Poll::Ready(v) => v,
                            std::task::Poll::Pending => {
                                #pending_restore
                            }
                        };
                    }
                } else {
                    quote_spanned! {pattern_span=>
                        let __boxed_result__ = match _io.as_mut().poll(&mut std::task::Context::from_waker(&std::task::Waker::noop())) {
                            std::task::Poll::Ready(v) => v,
                            std::task::Poll::Pending => {
                                #pending_restore
                            }
                        };
                        let #pattern = *__boxed_result__.downcast::<_>().expect("Type mismatch in yield result");
                    }
                }
            } else {
                quote! {
                    match _io.as_mut().poll(&mut std::task::Context::from_waker(&std::task::Waker::noop())) {
                        std::task::Poll::Ready(_) => (),
                        std::task::Poll::Pending => {
                            #pending_restore
                        }
                    }
                }
            };
            
            // 不需要重新声明变量，因为我们直接使用了相同的名字
            
            // 获取该yield之后的语句（段i+1）
            let statements_after_yield = segments.get(i + 1)
                .map(|seg| seg.as_slice())
                .unwrap_or(&[]);
            
            // 准备下一个状态
            let next_state_transition = if i + 1 < yield_count {
                let next_name = quote::format_ident!("Yield{}", i + 1);
                let next_setup = yield_setup_statements.get(i + 1)
                    .map(|stmts| stmts.as_slice())
                    .unwrap_or(&[]);
                let next_expr = yield_expressions.get(i + 1)
                    .cloned()
                    .unwrap_or_else(|| parse_quote! { () });
                
                let next_expr_span = next_expr.span();
                let future_creation = if let Some(Some(_)) = yield_return_types.get(i + 1) {
                    quote_spanned! {next_expr_span=> Box::pin(#next_expr) }
                } else {
                    quote_spanned! {next_expr_span=> 
                        Box::pin(async move {
                            Box::new(#next_expr.await) as Box<dyn std::any::Any>
                        })
                    }
                };
                
                // 收集下一个yield点需要保存的变量
                let next_vars: Vec<_> = cross_yield_variables
                    .iter()
                    .filter(|scope| {
                        scope.crossed_yields.contains(&(i + 1)) && 
                        !scope.is_shadowed &&
                        scope.last_use_yield_index.map_or(true, |last| last >= (i + 1))
                    })
                    .map(|scope| {
                        let var_name = &scope.variable.name;
                        quote! { #var_name }
                    })
                    .collect();
                
                if next_vars.is_empty() {
                    quote! {
                        #(#next_setup)*
                        *__state__ = #state_enum_name::#next_name {
                            _io: #future_creation,
                        };
                    }
                } else {
                    quote! {
                        #(#next_setup)*
                        *__state__ = #state_enum_name::#next_name {
                            _io: #future_creation,
                            #(#next_vars,)*
                        };
                    }
                }
            } else {
                quote! { 
                    *__state__ = #state_enum_name::Done;
                }
            };
            
            quote! {
                #match_pattern => {
                    #poll_handling
                    
                    // 执行yield之后的语句
                    #(#statements_after_yield)*
                    
                    #next_state_transition
                },
            }
        })
        .collect()
}

impl VisitMut for YieldAnalyzer {
    fn visit_stmt_mut(&mut self, stmt: &mut Stmt) {
        // 处理 let 语句中的 yield_op
        if let Stmt::Local(local) = stmt {
            // 检查初始化表达式是否包含 yield_op
            if let Some(init) = &local.init {
                if let Expr::Macro(macro_expr) = &*init.expr {
                    if macro_expr.mac.path.is_ident("yield_op") {
                        self.yield_count += 1;

                        // 解析宏内容
                        let tokens = &macro_expr.mac.tokens;
                        let yielded_expr = parse_yield_content(tokens.clone(), &mut self.yield_setup_statements);
                        self.yield_expressions.push(yielded_expr);
                        
                        // 保存赋值模式
                        self.yield_assignment_patterns.push(Some(local.pat.clone()));
                        
                        // 提取类型注解
                        if let syn::Pat::Type(pat_type) = &local.pat {
                            // 有类型注解，如 let result: Instant = yield_op!{...}
                            self.yield_return_types.push(Some((*pat_type.ty).clone()));
                        } else {
                            // 没有类型注解，如 let result = yield_op!{...} 或其他模式
                            self.yield_return_types.push(None);
                        }
                        return;
                    }
                }
            }
        }
        
        // 处理独立的 yield_op!{...}; 语句
        if let Stmt::Macro(stmt_macro) = stmt {
            if stmt_macro.mac.path.is_ident("yield_op") {
                self.yield_count += 1;

                // 解析宏内容，支持多个语句
                let tokens = &stmt_macro.mac.tokens;
                let yielded_expr = parse_yield_content(tokens.clone(), &mut self.yield_setup_statements);
                self.yield_expressions.push(yielded_expr);
                // 独立的 yield 语句没有返回值
                self.yield_return_types.push(None);
                self.yield_assignment_patterns.push(None);
                return;
            }
        }
        
        // 继续递归访问以处理嵌套的情况
        syn::visit_mut::visit_stmt_mut(self, stmt);
    }
    
    fn visit_block_mut(&mut self, block: &mut Block) {
        // 递归访问块内的所有语句
        for stmt in &mut block.stmts {
            self.visit_stmt_mut(stmt);
        }
    }
    
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        match expr {
            // 处理块表达式
            Expr::Block(expr_block) => {
                self.visit_block_mut(&mut expr_block.block);
            }
            // 处理if表达式
            Expr::If(expr_if) => {
                self.visit_expr_mut(&mut expr_if.cond);
                self.visit_block_mut(&mut expr_if.then_branch);
                if let Some((_, else_expr)) = &mut expr_if.else_branch {
                    self.visit_expr_mut(else_expr);
                }
            }
            // 处理match表达式
            Expr::Match(expr_match) => {
                self.visit_expr_mut(&mut expr_match.expr);
                for arm in &mut expr_match.arms {
                    self.visit_expr_mut(&mut arm.body);
                }
            }
            // 处理for循环
            Expr::ForLoop(expr_for) => {
                self.visit_expr_mut(&mut expr_for.expr);
                self.visit_block_mut(&mut expr_for.body);
            }
            // 处理while循环
            Expr::While(expr_while) => {
                self.visit_expr_mut(&mut expr_while.cond);
                self.visit_block_mut(&mut expr_while.body);
            }
            // 处理loop循环
            Expr::Loop(expr_loop) => {
                self.visit_block_mut(&mut expr_loop.body);
            }
            // 处理宏表达式
            Expr::Macro(macro_expr) => {
                if macro_expr.mac.path.is_ident("yield_op") {
                    // 找到了 yield_op! 宏调用
                    self.yield_count += 1;

                    // 解析宏内容，支持多个语句
                    let tokens = &macro_expr.mac.tokens;
                    let yielded_expr = parse_yield_content(tokens.clone(), &mut self.yield_setup_statements);
                    self.yield_expressions.push(yielded_expr);
                    // 在表达式中的 yield_op 可能有返回值，但这里无法确定类型
                    // 这种情况应该通过 visit_stmt_mut 中的 let 语句处理
                    self.yield_return_types.push(None);
                    self.yield_assignment_patterns.push(None);
                    return;
                }
            }
            _ => {}
        }

        // 继续递归访问其他表达式
        syn::visit_mut::visit_expr_mut(self, expr);
    }
}

// ===== yield_op! 宏定义 =====

/// yield_op! 宏，用于在协程中标记异步操作点
/// 
/// # 使用方法
/// ```rust
/// yield_op! { async_operation() }  // 单个异步表达式
/// yield_op! {                       // 多个语句块
///     let data = prepare_data();
///     async_process(data)
/// }
/// ```
#[proc_macro]
#[proc_macro_error]
pub fn yield_op(_input: TokenStream) -> TokenStream {
    // 这个宏在实际运行时不应该被调用，它只是一个标记
    // 真正的处理在 #[test_coroutine_system] 宏中完成
    quote! {
        compile_error!("yield_op! can only be used inside a function marked with #[test_coroutine_system]")
    }
    .into()
}
