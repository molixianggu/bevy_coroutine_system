use std::collections::HashMap;
use syn::{
    Expr, Ident, Local, Pat, Stmt, Type,
    visit::{self, Visit},
};

/// 变量信息
#[derive(Debug, Clone)]
pub struct VariableInfo {
    /// 变量名
    pub name: Ident,
    /// 变量类型（必须有明确类型声明）
    pub ty: Type,

    /// 是否是可变的
    pub is_mut: bool,
}

/// 变量作用域信息
#[derive(Debug, Clone)]
pub struct VariableScope {
    /// 变量信息
    pub variable: VariableInfo,
    /// 跨越的yield点索引
    pub crossed_yields: Vec<usize>,
    /// 最后使用的位置（语句索引）
    pub last_use_index: Option<usize>,
    /// 最后使用时的yield索引（在第几个yield之后）
    pub last_use_yield_index: Option<usize>,
    /// 是否被遮蔽
    pub is_shadowed: bool,
    /// 遮蔽发生的位置
    pub shadow_index: Option<usize>,
}

/// 作用域上下文，用于追踪嵌套作用域
#[derive(Debug)]
struct ScopeContext {
    /// 当前作用域中的变量（变量名 -> 变量信息）
    variables: HashMap<String, VariableInfo>,
    /// 父作用域（如果有）
    parent: Option<Box<ScopeContext>>,
}

impl ScopeContext {
    fn new() -> Self {
        Self {
            variables: HashMap::new(),
            parent: None,
        }
    }

    fn with_parent(parent: Box<ScopeContext>) -> Self {
        Self {
            variables: HashMap::new(),
            parent: Some(parent),
        }
    }


    /// 添加变量到当前作用域
    fn add_variable(&mut self, name: String, info: VariableInfo) {
        self.variables.insert(name, info);
    }
}

/// 变量作用域分析器
pub struct VariableScopeAnalyzer {
    /// 所有检测到的变量作用域信息
    pub variable_scopes: Vec<VariableScope>,
    /// 当前语句索引
    current_stmt_index: usize,
    /// 当前yield点索引
    current_yield_index: usize,
    /// 作用域栈
    scope_stack: Vec<ScopeContext>,
    /// 活跃变量（变量名 -> 作用域索引）
    active_variables: HashMap<String, usize>,
}

impl VariableScopeAnalyzer {
    pub fn new() -> Self {
        Self {
            variable_scopes: Vec::new(),
            current_stmt_index: 0,
            current_yield_index: 0,
            scope_stack: vec![ScopeContext::new()],
            active_variables: HashMap::new(),
        }
    }

    /// 分析语句列表，返回需要保存的变量作用域信息
    pub fn analyze_statements(&mut self, stmts: &[Stmt]) -> Vec<VariableScope> {
        // 先分析所有语句
        for (idx, stmt) in stmts.iter().enumerate() {
            self.current_stmt_index = idx;
            self.analyze_statement(stmt);
        }
        
        // 更新变量的最后使用索引
        self.finalize_variable_usage();

        // 返回跨越yield点的变量
        self.variable_scopes
            .iter()
            .filter(|scope| !scope.crossed_yields.is_empty() && !scope.is_shadowed)
            .cloned()
            .collect()
    }
    
    /// 完成变量使用分析，确保变量在所有需要的地方都能被访问
    fn finalize_variable_usage(&mut self) {
        // 对于每个跨越yield的变量，确保它在最后一个yield点后仍然可用
        for scope in &mut self.variable_scopes {
            if !scope.crossed_yields.is_empty() && scope.last_use_yield_index.is_some() {
                // 如果变量在最后一个yield点之后还被使用，确保它在所有yield点都可用
                let max_yield = *scope.crossed_yields.iter().max().unwrap_or(&0);
                let last_use_yield = scope.last_use_yield_index.unwrap_or(0);
                if last_use_yield > max_yield {
                    // 变量在最后一个yield点之后还被使用，需要确保它在所有yield点都可恢复
                    for i in 0..=max_yield {
                        if !scope.crossed_yields.contains(&i) {
                            scope.crossed_yields.push(i);
                        }
                    }
                    scope.crossed_yields.sort();
                }
            }
        }
    }

    /// 分析单个语句
    fn analyze_statement(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Local(local) => self.analyze_local(local),
            Stmt::Expr(expr, _) => {
                // 检查是否包含 yield_op!
                if self.contains_yield(stmt) {
                    self.handle_yield_point();
                }
                // 分析表达式中的变量使用
                self.analyze_expr_usage(expr);
            }
            Stmt::Macro(syn::StmtMacro { .. }) => {
                // 检查是否包含 yield_op!
                if self.contains_yield(stmt) {
                    self.handle_yield_point();
                }
                // Macro语句没有直接的表达式可以分析
            }
            Stmt::Item(_) => {
                // 项定义（如函数、结构体等）不影响变量作用域
            }
        }
    }

    /// 分析局部变量声明
    fn analyze_local(&mut self, local: &Local) {
        // 先检查初始化表达式是否包含yield
        let contains_yield = if let Some(init) = &local.init {
            self.expr_contains_yield(&init.expr)
        } else {
            false
        };
        
        // 记录yield前的索引，用于判断变量是否在yield后声明
        let pre_yield_index = self.current_yield_index;
        
        // 如果初始化表达式包含yield，先处理yield
        if contains_yield {
            self.handle_yield_point();
        }
        
        // 只处理有类型注解的变量
        if let Pat::Type(pat_type) = &local.pat {
            if let Pat::Ident(pat_ident) = &*pat_type.pat {
                // 检查是否是引用类型
                if !self.is_reference_type(&pat_type.ty) {
                    let var_info = VariableInfo {
                        name: pat_ident.ident.clone(),
                        ty: (*pat_type.ty).clone(),

                        is_mut: pat_ident.mutability.is_some(),
                    };

                    let var_name = pat_ident.ident.to_string();
                    
                    // 检查是否遮蔽了现有变量
                    if let Some(scope_idx) = self.active_variables.get(&var_name).cloned() {
                        // 标记旧变量被遮蔽
                        if let Some(old_scope) = self.variable_scopes.get_mut(scope_idx) {
                            old_scope.is_shadowed = true;
                            old_scope.shadow_index = Some(self.current_stmt_index);
                        }
                    }

                    // 添加新变量
                    let mut scope = VariableScope {
                        variable: var_info.clone(),
                        crossed_yields: Vec::new(),
                        last_use_index: None,
                        last_use_yield_index: None,
                        is_shadowed: false,
                        shadow_index: None,
                    };
                    
                    // 如果变量是通过yield表达式初始化的，不应该标记为跨越该yield
                    // 但如果它在之前的yield后声明，仍然需要标记跨越之前的yield
                    if !contains_yield && pre_yield_index > 0 {
                        // 变量在某个yield之后声明，需要标记
                        for i in 0..pre_yield_index {
                            scope.crossed_yields.push(i);
                        }
                    }

                    let scope_idx = self.variable_scopes.len();
                    self.variable_scopes.push(scope);
                    self.active_variables.insert(var_name.clone(), scope_idx);

                    // 添加到当前作用域上下文
                    if let Some(current_scope) = self.scope_stack.last_mut() {
                        current_scope.add_variable(var_name, var_info);
                    }
                }
            }
        }

        // 分析初始化表达式（除了yield部分）
        if let Some(init) = &local.init {
            if !contains_yield {
                self.analyze_expr_usage(&init.expr);
            }
        }
    }

    /// 检查是否是引用类型
    fn is_reference_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::Reference(_))
    }
    
    /// 检查表达式是否包含yield
    fn expr_contains_yield(&self, expr: &Expr) -> bool {
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
                visit::visit_expr(self, expr);
            }
        }

        let mut checker = YieldChecker { found: false };
        checker.visit_expr(expr);
        checker.found
    }

    /// 检查语句是否包含yield
    fn contains_yield(&self, stmt: &Stmt) -> bool {
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
                visit::visit_expr(self, expr);
            }
        }

        let mut checker = YieldChecker { found: false };
        match stmt {
            Stmt::Expr(expr, _) => checker.visit_expr(expr),
            Stmt::Macro(stmt_macro) => {
                if stmt_macro.mac.path.is_ident("yield_op") {
                    return true;
                }
            }
            _ => {}
        }
        checker.found
    }

    /// 处理yield点
    fn handle_yield_point(&mut self) {
        // 为所有活跃变量记录跨越的yield点
        for (_, scope_idx) in &self.active_variables {
            if let Some(scope) = self.variable_scopes.get_mut(*scope_idx) {
                if !scope.is_shadowed {
                    scope.crossed_yields.push(self.current_yield_index);
                }
            }
        }
        self.current_yield_index += 1;
    }

    /// 分析表达式中的变量使用
    fn analyze_expr_usage(&mut self, expr: &Expr) {
        struct UsageVisitor<'a> {
            analyzer: &'a mut VariableScopeAnalyzer,
        }

        impl<'a, 'ast> Visit<'ast> for UsageVisitor<'a> {
            fn visit_expr(&mut self, expr: &'ast Expr) {
                match expr {
                    Expr::Path(expr_path) => {
                        if expr_path.path.segments.len() == 1 {
                            let var_name = expr_path.path.segments[0].ident.to_string();
                            if let Some(scope_idx) = self.analyzer.active_variables.get(&var_name).cloned() {
                                if let Some(scope) = self.analyzer.variable_scopes.get_mut(scope_idx) {
                                    scope.last_use_index = Some(self.analyzer.current_stmt_index);
                                    scope.last_use_yield_index = Some(self.analyzer.current_yield_index);
                                    // 如果在yield之后使用，需要标记
                                    if self.analyzer.current_yield_index > 0 && 
                                       !scope.crossed_yields.contains(&(self.analyzer.current_yield_index - 1)) {
                                        // 变量在当前yield点之后被使用，需要从前一个yield点恢复
                                        scope.crossed_yields.push(self.analyzer.current_yield_index - 1);
                                        scope.crossed_yields.sort();
                                        scope.crossed_yields.dedup();
                                    }
                                }
                            }
                        }
                    }
                    Expr::Block(expr_block) => {
                        // 进入新作用域
                        self.analyzer.enter_scope();
                        for stmt in &expr_block.block.stmts {
                            self.analyzer.analyze_statement(stmt);
                        }
                        self.analyzer.exit_scope();
                        return; // 不继续递归
                    }
                    Expr::If(expr_if) => {
                        // 条件表达式
                        visit::visit_expr(self, &expr_if.cond);
                        
                        // then分支
                        self.analyzer.enter_scope();
                        for stmt in &expr_if.then_branch.stmts {
                            self.analyzer.analyze_statement(stmt);
                        }
                        self.analyzer.exit_scope();
                        
                        // else分支
                        if let Some((_, else_branch)) = &expr_if.else_branch {
                            self.analyzer.enter_scope();
                            visit::visit_expr(self, else_branch);
                            self.analyzer.exit_scope();
                        }
                        return;
                    }
                    Expr::Match(expr_match) => {
                        // 匹配表达式
                        visit::visit_expr(self, &expr_match.expr);
                        
                        // 每个分支
                        for arm in &expr_match.arms {
                            self.analyzer.enter_scope();
                            // TODO: 处理模式绑定
                            visit::visit_expr(self, &arm.body);
                            self.analyzer.exit_scope();
                        }
                        return;
                    }
                    Expr::Loop(expr_loop) => {
                        // 循环体
                        self.analyzer.enter_scope();
                        for stmt in &expr_loop.body.stmts {
                            self.analyzer.analyze_statement(stmt);
                        }
                        self.analyzer.exit_scope();
                        return;
                    }
                    Expr::While(expr_while) => {
                        // 条件
                        visit::visit_expr(self, &expr_while.cond);
                        
                        // 循环体
                        self.analyzer.enter_scope();
                        for stmt in &expr_while.body.stmts {
                            self.analyzer.analyze_statement(stmt);
                        }
                        self.analyzer.exit_scope();
                        return;
                    }
                    Expr::ForLoop(expr_for) => {
                        // for循环
                        visit::visit_expr(self, &expr_for.expr);
                        
                        self.analyzer.enter_scope();
                        // TODO: 处理模式绑定
                        for stmt in &expr_for.body.stmts {
                            self.analyzer.analyze_statement(stmt);
                        }
                        self.analyzer.exit_scope();
                        return;
                    }
                    _ => {}
                }
                
                // 继续递归访问
                visit::visit_expr(self, expr);
            }
        }

        let mut visitor = UsageVisitor { analyzer: self };
        visitor.visit_expr(expr);
    }

    /// 进入新作用域
    fn enter_scope(&mut self) {
        let current = self.scope_stack.pop().unwrap();
        let new_scope = ScopeContext::with_parent(Box::new(current));
        self.scope_stack.push(new_scope);
    }

    /// 退出当前作用域
    fn exit_scope(&mut self) {
        if let Some(mut current) = self.scope_stack.pop() {
            // 清理当前作用域中声明的变量
            for var_name in current.variables.keys() {
                self.active_variables.remove(var_name);
            }
            
            // 恢复父作用域
            if let Some(parent) = current.parent.take() {
                self.scope_stack.push(*parent);
            } else {
                // 不应该发生，至少应该有一个根作用域
                self.scope_stack.push(ScopeContext::new());
            }
        }
    }
}
