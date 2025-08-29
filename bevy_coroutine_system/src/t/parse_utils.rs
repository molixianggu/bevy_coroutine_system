use syn::{Expr, Stmt, Block, parse_quote, visit::Visit, visit_mut::VisitMut};
use quote::quote;

/// 块分段信息，支持嵌套结构
#[derive(Debug, Clone)]
pub enum BlockSegment {
    /// 简单语句序列（不包含yield）
    Statements(Vec<Stmt>),
    /// 包含yield的语句
    YieldPoint {
        /// yield之前的设置语句
        setup_stmts: Vec<Stmt>,
        /// yield表达式
        yield_expr: Expr,
        /// yield的索引
        yield_index: usize,
    },
    /// 包含yield的复杂语句（如if、for等）
    ComplexStatement {
        /// 原始语句（保持结构）
        stmt: Stmt,
        /// 内部的分段信息
        inner_segments: Vec<BlockSegment>,
    },
}

/// 将蛇形命名法转换为大驼峰命名法
pub fn to_pascal_case(snake_str: &str) -> String {
    snake_str
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
            }
        })
        .collect()
}

/// 检查语句是否包含yield
pub struct ContainsYield {
    found: bool,
}

impl ContainsYield {
    pub fn new() -> Self {
        Self { found: false }
    }
    
    pub fn check_stmt(stmt: &Stmt) -> bool {
        let mut checker = Self::new();
        checker.visit_stmt(stmt);
        checker.found
    }
}

impl<'ast> Visit<'ast> for ContainsYield {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        // 检查独立的 yield_op! 语句
        if let Stmt::Macro(stmt_macro) = stmt {
            if stmt_macro.mac.path.is_ident("yield_op") {
                self.found = true;
                return;
            }
        }
        // 继续递归检查
        syn::visit::visit_stmt(self, stmt);
    }
    
    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let Expr::Macro(macro_expr) = expr {
            if macro_expr.mac.path.is_ident("yield_op") {
                self.found = true;
                return;
            }
        }
        syn::visit::visit_expr(self, expr);
    }
}

/// 分析块中的yield点，用于跟踪yield索引
pub struct YieldIndexTracker {
    current_index: usize,
}

impl YieldIndexTracker {
    pub fn new() -> Self {
        Self { current_index: 0 }
    }
    
    pub fn next_index(&mut self) -> usize {
        let idx = self.current_index;
        self.current_index += 1;
        idx
    }
}

/// 将函数体按yield点分段（新版本，支持嵌套）
pub fn split_block_by_yields_nested(block: &Block, yield_tracker: &mut YieldIndexTracker) -> Vec<BlockSegment> {
    let mut segments = Vec::new();
    let mut current_stmts = Vec::new();
    
    for stmt in &block.stmts {
        match analyze_statement_for_yield(stmt, yield_tracker) {
            StatementYieldInfo::NoYield => {
                // 普通语句，加入当前段
                current_stmts.push(stmt.clone());
            }
            StatementYieldInfo::SimpleYield { setup_stmts, yield_expr, yield_index } => {
                // 先保存之前的语句
                if !current_stmts.is_empty() {
                    segments.push(BlockSegment::Statements(current_stmts.clone()));
                    current_stmts.clear();
                }
                // 添加yield点
                segments.push(BlockSegment::YieldPoint {
                    setup_stmts,
                    yield_expr,
                    yield_index,
                });
            }
            StatementYieldInfo::ComplexYield { stmt: complex_stmt, inner_segments } => {
                // 先保存之前的语句
                if !current_stmts.is_empty() {
                    segments.push(BlockSegment::Statements(current_stmts.clone()));
                    current_stmts.clear();
                }
                // 添加复杂语句
                segments.push(BlockSegment::ComplexStatement {
                    stmt: complex_stmt,
                    inner_segments,
                });
            }
        }
    }
    
    // 保存最后的语句段
    if !current_stmts.is_empty() {
        segments.push(BlockSegment::Statements(current_stmts));
    }
    
    segments
}

/// 语句中yield信息的分析结果
enum StatementYieldInfo {
    /// 不包含yield
    NoYield,
    /// 简单yield（独立语句或表达式）
    SimpleYield {
        setup_stmts: Vec<Stmt>,
        yield_expr: Expr,
        yield_index: usize,
    },
    /// 复杂yield（在控制流结构中）
    ComplexYield {
        stmt: Stmt,
        inner_segments: Vec<BlockSegment>,
    },
}

/// 分析单个语句的yield情况
fn analyze_statement_for_yield(stmt: &Stmt, yield_tracker: &mut YieldIndexTracker) -> StatementYieldInfo {
    match stmt {
        // 处理独立的yield_op!宏调用
        Stmt::Macro(stmt_macro) if stmt_macro.mac.path.is_ident("yield_op") => {
            let tokens = &stmt_macro.mac.tokens;
            let mut setup_stmts_vec = Vec::new();
            let yield_expr = parse_yield_content(tokens.clone(), &mut setup_stmts_vec);
            let yield_index = yield_tracker.next_index();
            
            StatementYieldInfo::SimpleYield {
                setup_stmts: setup_stmts_vec.pop().unwrap_or_else(Vec::new),
                yield_expr,
                yield_index,
            }
        }
        
        // 处理let语句中的yield
        Stmt::Local(local) => {
            if let Some(init) = &local.init {
                if let Expr::Macro(macro_expr) = &*init.expr {
                    if macro_expr.mac.path.is_ident("yield_op") {
                        let tokens = &macro_expr.mac.tokens;
                        let mut setup_stmts_vec = Vec::new();
                        let yield_expr = parse_yield_content(tokens.clone(), &mut setup_stmts_vec);
                        let yield_index = yield_tracker.next_index();
                        
                        return StatementYieldInfo::SimpleYield {
                            setup_stmts: setup_stmts_vec.pop().unwrap_or_else(Vec::new),
                            yield_expr,
                            yield_index,
                        };
                    }
                }
            }
            
            // 检查初始化表达式中是否有嵌套的yield
            if let Some(init) = &local.init {
                if expr_contains_yield_nested(&init.expr) {
                    // 需要递归处理表达式
                    let mut transformed_stmt = stmt.clone();
                    let inner_segments = process_expr_yields(&mut transformed_stmt, yield_tracker);
                    return StatementYieldInfo::ComplexYield {
                        stmt: transformed_stmt,
                        inner_segments,
                    };
                }
            }
            
            StatementYieldInfo::NoYield
        }
        
        // 处理表达式语句
        Stmt::Expr(expr, semi) => {
            if expr_contains_yield_nested(expr) {
                let mut transformed_stmt = stmt.clone();
                let inner_segments = process_expr_yields(&mut transformed_stmt, yield_tracker);
                StatementYieldInfo::ComplexYield {
                    stmt: transformed_stmt,
                    inner_segments,
                }
            } else {
                StatementYieldInfo::NoYield
            }
        }
        
        _ => StatementYieldInfo::NoYield,
    }
}

/// 检查表达式是否包含yield（包括嵌套）
fn expr_contains_yield_nested(expr: &Expr) -> bool {
    struct YieldChecker {
        found: bool,
    }
    
    impl<'ast> Visit<'ast> for YieldChecker {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            if let Expr::Macro(macro_expr) = expr {
                if macro_expr.mac.path.is_ident("yield_op") {
                    self.found = true;
                    return;
                }
            }
            syn::visit::visit_expr(self, expr);
        }
    }
    
    let mut checker = YieldChecker { found: false };
    checker.visit_expr(expr);
    checker.found
}

/// 处理表达式中的yield点
fn process_expr_yields(stmt: &mut Stmt, yield_tracker: &mut YieldIndexTracker) -> Vec<BlockSegment> {
    struct YieldProcessor<'a> {
        segments: Vec<BlockSegment>,
        yield_tracker: &'a mut YieldIndexTracker,
    }
    
    impl<'a> VisitMut for YieldProcessor<'a> {
        fn visit_block_mut(&mut self, block: &mut Block) {
            // 递归处理块
            let inner_segments = split_block_by_yields_nested(block, self.yield_tracker);
            self.segments.extend(inner_segments);
        }
    }
    
    let mut processor = YieldProcessor {
        segments: Vec::new(),
        yield_tracker,
    };
    
    processor.visit_stmt_mut(stmt);
    processor.segments
}

/// 处理包含yield的语句
fn handle_yield_statement(stmt: &Stmt, segments: &mut Vec<Vec<Stmt>>, current_segment_idx: &mut usize) {
    match stmt {
        // 处理块表达式中的yield
        Stmt::Expr(Expr::Block(expr_block), _semi) => {
            // 递归处理块内的语句
            let inner_segments = split_block_by_yields(&expr_block.block);
            
            // 将第一个内部段的语句添加到当前段
            if let Some(first_inner) = inner_segments.get(0) {
                segments[*current_segment_idx].extend(first_inner.clone());
            }
            
            // 为剩余的内部段创建新段
            for inner_segment in inner_segments.iter().skip(1) {
                segments.push(inner_segment.clone());
                *current_segment_idx += 1;
            }
        }
        // 处理if表达式中的yield
        Stmt::Expr(Expr::If(_expr_if), _semi) => {
            // TODO: 需要更复杂的处理来保持if语句的结构
            segments.push(Vec::new());
            *current_segment_idx += 1;
        }
        // 处理for循环中的yield
        Stmt::Expr(Expr::ForLoop(_expr_for), _semi) => {
            // TODO: 需要更复杂的处理来保持for循环的结构
            segments.push(Vec::new());
            *current_segment_idx += 1;
        }
        // 处理while循环中的yield
        Stmt::Expr(Expr::While(_expr_while), _semi) => {
            // TODO: 需要更复杂的处理来保持while循环的结构
            segments.push(Vec::new());
            *current_segment_idx += 1;
        }
        // 处理match表达式中的yield
        Stmt::Expr(Expr::Match(_expr_match), _semi) => {
            // TODO: 需要更复杂的处理来保持match的结构
            segments.push(Vec::new());
            *current_segment_idx += 1;
        }
        // 其他包含yield的语句
        _ => {
            segments.push(Vec::new());
            *current_segment_idx += 1;
        }
    }
}

/// 将函数体按yield点分段（改进版，支持嵌套块）
pub fn split_block_by_yields(block: &Block) -> Vec<Vec<Stmt>> {
    let mut segments = vec![Vec::new()];
    let mut current_segment_idx = 0;
    
    for stmt in &block.stmts {
        if ContainsYield::check_stmt(stmt) {
            // 当前语句包含yield，需要特殊处理
            handle_yield_statement(stmt, &mut segments, &mut current_segment_idx);
        } else {
            // 不包含yield的语句加入当前段
            segments[current_segment_idx].push(stmt.clone());
        }
    }
    
    segments
}

/// 解析 yield_op! 宏的内容，支持多个语句
pub fn parse_yield_content(tokens: proc_macro2::TokenStream, yield_setup_statements: &mut Vec<Vec<Stmt>>) -> Expr {
    // 首先尝试解析为单个表达式
    if let Ok(expr) = syn::parse2::<Expr>(tokens.clone()) {
        // 单个表达式，没有设置语句
        yield_setup_statements.push(Vec::new());
        return expr;
    }
    
    // 尝试解析为语句块
    if let Ok(block) = syn::parse2::<Block>(tokens.clone()) {
        return extract_setup_and_yield_from_block(block, yield_setup_statements);
    }
    
    // 如果不是块，尝试将tokens包装成块
    let wrapped_tokens = quote! { { #tokens } };
    if let Ok(block) = syn::parse2::<Block>(wrapped_tokens) {
        return extract_setup_and_yield_from_block(block, yield_setup_statements);
    }
    
    // 最后的fallback
    yield_setup_statements.push(Vec::new());
    parse_quote! { () }
}

/// 从块中提取设置语句和yield表达式
pub fn extract_setup_and_yield_from_block(block: Block, yield_setup_statements: &mut Vec<Vec<Stmt>>) -> Expr {
    let stmts = block.stmts;
    
    if stmts.is_empty() {
        yield_setup_statements.push(Vec::new());
        return parse_quote! { () };
    }
    
    if stmts.len() == 1 {
        // 只有一个语句
        match &stmts[0] {
            Stmt::Expr(expr, None) => {
                // 单个表达式
                yield_setup_statements.push(Vec::new());
                return expr.clone();
            }
            _ => {
                // 非表达式语句，作为设置语句
                yield_setup_statements.push(vec![stmts[0].clone()]);
                return parse_quote! { () };
            }
        }
    }
    
    // 多个语句
    match stmts.last().unwrap() {
        Stmt::Expr(expr, None) => {
            // 最后是表达式，前面都是设置语句
            let setup_stmts = stmts[..stmts.len()-1].to_vec();
            yield_setup_statements.push(setup_stmts);
            expr.clone()
        }
        _ => {
            // 最后不是表达式，所有都是设置语句
            yield_setup_statements.push(stmts);
            parse_quote! { () }
        }
    }
}


