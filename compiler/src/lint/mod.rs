use crate::ast::{
    self, Block, Expression, ExpressionKind, Function, ImplBlock, Item, LValue, Method, Module,
    Statement, StatementKind, TraitDeclaration, TraitImpl, TraitMethod,
};
use crate::span::Span;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LintRule {
    UnusedBinding,
    UnreachableCode,
    Shadowing,
    NarrowingCast,
}

impl LintRule {
    pub fn code(&self) -> &'static str {
        match self {
            LintRule::UnusedBinding => "unused-binding",
            LintRule::UnreachableCode => "unreachable-code",
            LintRule::Shadowing => "shadowing",
            LintRule::NarrowingCast => "narrowing-cast",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            LintRule::UnusedBinding => "unused binding",
            LintRule::UnreachableCode => "unreachable code",
            LintRule::Shadowing => "shadowed binding",
            LintRule::NarrowingCast => "narrowing numeric cast",
        }
    }

    pub fn all() -> &'static [LintRule] {
        const ALL: &[LintRule] = &[
            LintRule::UnusedBinding,
            LintRule::UnreachableCode,
            LintRule::Shadowing,
            LintRule::NarrowingCast,
        ];
        ALL
    }

    pub fn from_code(value: &str) -> Option<Self> {
        match value {
            "unused-binding" | "unused_binding" => Some(LintRule::UnusedBinding),
            "unreachable-code" | "unreachable_code" => Some(LintRule::UnreachableCode),
            "shadowing" => Some(LintRule::Shadowing),
            "narrowing-cast" | "narrowing_cast" => Some(LintRule::NarrowingCast),
            _ => None,
        }
    }

    /// Stable error code assigned when this rule is escalated to a hard error
    /// via `--deny`. `None` for rules without a reserved code yet.
    pub fn stable_error_code(&self) -> Option<&'static str> {
        match self {
            LintRule::NarrowingCast => Some("E035"),
            LintRule::UnusedBinding | LintRule::UnreachableCode | LintRule::Shadowing => None,
        }
    }
}

impl FromStr for LintRule {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_ascii_lowercase();
        LintRule::from_code(&normalized).ok_or(())
    }
}

#[derive(Debug, Clone)]
pub struct LintDiagnostic {
    pub rule: LintRule,
    pub message: String,
    pub span: Span,
    pub note: Option<String>,
    pub secondary_span: Option<Span>,
}

#[derive(Debug, Clone)]
pub struct LintOptions {
    pub enabled: HashSet<LintRule>,
    pub deny: HashSet<LintRule>,
}

impl Default for LintOptions {
    fn default() -> Self {
        Self::all()
    }
}

impl LintOptions {
    pub fn disabled() -> Self {
        Self {
            enabled: HashSet::new(),
            deny: HashSet::new(),
        }
    }

    pub fn all() -> Self {
        let enabled: HashSet<LintRule> = LintRule::all().iter().copied().collect();
        Self {
            enabled,
            deny: HashSet::new(),
        }
    }

    pub fn is_enabled(&self, rule: LintRule) -> bool {
        self.enabled.contains(&rule)
    }

    pub fn is_denied(&self, rule: LintRule) -> bool {
        self.deny.contains(&rule)
    }

    pub fn enable_rule(&mut self, rule: LintRule) {
        self.enabled.insert(rule);
        self.deny.remove(&rule);
    }

    pub fn disable_rule(&mut self, rule: LintRule) {
        self.enabled.remove(&rule);
        self.deny.remove(&rule);
    }

    pub fn deny_rule(&mut self, rule: LintRule) {
        self.enabled.insert(rule);
        self.deny.insert(rule);
    }
}

pub fn lint_module(
    module: &Module,
    options: &LintOptions,
) -> Result<Vec<LintDiagnostic>, crate::error::SemanticError> {
    if options.enabled.is_empty() {
        return Ok(Vec::new());
    }

    LintRunner::new(options).run(module)
}

struct LintRunner<'a> {
    options: &'a LintOptions,
    diagnostics: Vec<LintDiagnostic>,
    scope_stack: Vec<Scope>,
    /// Current recursion depth of the lint walk, capped by the shared
    /// frontend budget (`P013`) so pathological ASTs fail with a coded
    /// diagnostic instead of overflowing the stack.
    depth: usize,
    /// Ensures the lint `P013` diagnostic is reported only once.
    depth_limit_reported: bool,
    /// Stack address captured when the runner was created; used to estimate
    /// how much stack the recursive walk has consumed.
    stack_probe: usize,
    /// Set when the recursion guard trips; `lint_module` surfaces it as a
    /// hard coded error instead of partial (possibly misleading) warnings.
    guard_error: Option<crate::error::SemanticError>,
}

impl<'a> LintRunner<'a> {
    fn new(options: &'a LintOptions) -> Self {
        Self {
            options,
            diagnostics: Vec::new(),
            scope_stack: Vec::new(),
            depth: 0,
            depth_limit_reported: false,
            stack_probe: crate::parser::Parser::capture_stack_probe(),
            guard_error: None,
        }
    }

    fn run(mut self, module: &Module) -> Result<Vec<LintDiagnostic>, crate::error::SemanticError> {
        for item in &module.items {
            self.visit_item(item);
        }

        match self.guard_error {
            Some(error) => Err(error),
            None => Ok(self.diagnostics),
        }
    }

    /// Enters one level of lint recursion. Returns `Err(())` — after recording
    /// a single `P013` guard diagnostic at `span` — when the walk exceeds the
    /// shared frontend depth cap or stack budget, so deeply nested input fails
    /// cleanly instead of exhausting the stack. The failed level always exits
    /// again so sibling subtrees keep getting analyzed.
    fn enter_visit_depth(&mut self, span: Span) -> Result<(), ()> {
        self.depth += 1;
        let over_depth = self.depth > crate::parser::MAX_PARSE_DEPTH;
        let over_stack =
            crate::parser::Parser::stack_used_bytes(self.stack_probe)
                > crate::parser::MAX_STACK_USE_BYTES;
        if over_depth || over_stack {
            if !self.depth_limit_reported {
                self.depth_limit_reported = true;
                self.guard_error = Some(
                    crate::error::SemanticError::new("nesting too deep", span)
                        .with_code("P013")
                        .with_context("lint walk recursion exceeded its nesting/stack guard")
                        .with_hint(format!(
                            "Reduce nesting of expressions, statements, blocks, or patterns to at most {} levels.",
                            crate::parser::MAX_PARSE_DEPTH
                        )),
                );
            }
            self.depth = self.depth.saturating_sub(1);
            return Err(());
        }
        Ok(())
    }

    fn exit_visit_depth(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn visit_item(&mut self, item: &Item) {
        match item {
            Item::Function(function) => self.visit_function(function),
            Item::Impl(impl_block) => self.visit_impl_block(impl_block),
            Item::TraitImpl(trait_impl) => self.visit_trait_impl(trait_impl),
            Item::Trait(trait_decl) => self.visit_trait(trait_decl),
            Item::Import(_) | Item::Struct(_) | Item::Enum(_) => {}
            Item::TypeAlias(_) | Item::Const(_) | Item::Static(_) => {}
        }
    }

    fn visit_function(&mut self, function: &Function) {
        self.enter_scope();
        for param in &function.params {
            let exact_num = ExactNum::from_annotation(param.ty.as_ref());
            self.declare_binding(
                param.name.clone(),
                param.span,
                BindingKind::Parameter,
                exact_num,
            );
        }
        self.visit_block(&function.body, false);
        self.exit_scope();
    }

    fn visit_impl_block(&mut self, impl_block: &ImplBlock) {
        for method in &impl_block.methods {
            self.visit_method(method);
        }
    }

    fn visit_trait_impl(&mut self, trait_impl: &TraitImpl) {
        for method in &trait_impl.methods {
            self.visit_method(method);
        }
    }

    fn visit_trait(&mut self, trait_decl: &TraitDeclaration) {
        for method in &trait_decl.methods {
            if let Some(body) = &method.body {
                self.visit_trait_method(method, body);
            }
        }
    }

    fn visit_trait_method(&mut self, method: &TraitMethod, body: &Block) {
        self.enter_scope();
        for param in &method.params {
            if param.is_self {
                continue;
            }
            self.declare_binding(
                param.name.clone(),
                param.span,
                BindingKind::Parameter,
                ExactNum::from_annotation(param.type_annotation.as_ref()),
            );
        }
        self.visit_block(body, false);
        self.exit_scope();
    }

    fn visit_method(&mut self, method: &Method) {
        self.enter_scope();
        for param in &method.params {
            if param.is_self {
                continue;
            }
            self.declare_binding(
                param.name.clone(),
                param.span,
                BindingKind::Parameter,
                ExactNum::from_annotation(param.type_annotation.as_ref()),
            );
        }
        self.visit_block(&method.body, false);
        self.exit_scope();
    }

    fn visit_block(&mut self, block: &Block, introduce_scope: bool) -> bool {
        if introduce_scope {
            self.enter_scope();
        }

        let mut reachable = true;
        let mut last_terminator: Option<Span> = None;
        for statement in &block.statements {
            if !reachable {
                self.emit_unreachable(statement.span, last_terminator);
                continue;
            }

            let fallthrough = self.visit_statement(statement);
            if !fallthrough {
                reachable = false;
                last_terminator = Some(statement.span);
            }
        }

        if introduce_scope {
            self.exit_scope();
        }

        reachable
    }

    fn visit_statement(&mut self, statement: &Statement) -> bool {
        if self.enter_visit_depth(statement.span).is_err() {
            // Guard already reported once; treat the skipped subtree as
            // conservatively fall-through so no false unreachable warnings.
            return true;
        }
        let fallthrough = self.visit_statement_inner(statement);
        self.exit_visit_depth();
        fallthrough
    }

    fn visit_statement_inner(&mut self, statement: &Statement) -> bool {
        match &statement.kind {
            StatementKind::Let(let_stmt) => {
                if let Some(value) = &let_stmt.value {
                    self.visit_expression(value);
                }
                self.declare_let_pattern_bindings(
                    &let_stmt.pattern,
                    let_stmt.ty.as_ref(),
                    let_stmt.span,
                );
                true
            }
            StatementKind::Assignment(assign_stmt) => {
                self.visit_assignment(assign_stmt);
                true
            }
            StatementKind::Return(ret_stmt) => {
                if let Some(value) = &ret_stmt.value {
                    self.visit_expression(value);
                }
                false
            }
            StatementKind::Expression(expr) => {
                self.visit_expression(expr);
                true
            }
            StatementKind::While(while_loop) => {
                self.visit_expression(&while_loop.condition);
                self.visit_block(&while_loop.body, true);
                true
            }
            StatementKind::DoWhile(do_while_loop) => {
                self.visit_block(&do_while_loop.body, true);
                self.visit_expression(&do_while_loop.condition);
                true
            }
            StatementKind::For(for_loop) => {
                self.visit_expression(&for_loop.iterable);
                self.enter_scope();
                self.declare_binding(
                    for_loop.iterator.clone(),
                    for_loop.span,
                    BindingKind::ForIterator,
                    None,
                );
                self.visit_block(&for_loop.body, false);
                self.exit_scope();
                true
            }
            StatementKind::Loop(loop_stmt) => {
                self.visit_block(&loop_stmt.body, true);
                // A `loop {}` without any reachable `break` is an infinite
                // loop — code after it is unreachable. `block_has_break`
                // descends through expression-position if/match/block
                // statements, so `loop { if c { break } }` correctly marks
                // the code after the loop as reachable.
                block_has_break(&loop_stmt.body)
            }
            StatementKind::Switch(switch_stmt) => {
                self.visit_expression(&switch_stmt.value);
                for case in &switch_stmt.cases {
                    self.enter_scope();
                    self.visit_expression(&case.pattern);
                    self.visit_block(&case.body, true);
                    self.exit_scope();
                }
                if let Some(default_block) = &switch_stmt.default {
                    self.visit_block(default_block, true);
                }
                true
            }
            StatementKind::Break | StatementKind::Continue => false,
            StatementKind::IfLet(stmt) => {
                self.visit_expression(&stmt.value);
                self.visit_block(&stmt.then_block, true);
                if let Some(else_b) = &stmt.else_block {
                    self.visit_block(else_b, true);
                }
                true
            }
            StatementKind::WhileLet(stmt) => {
                self.visit_expression(&stmt.value);
                self.visit_block(&stmt.body, true);
                true
            }
        }
    }

    fn visit_assignment(&mut self, assignment: &ast::AssignmentStatement) {
        match &assignment.target {
            LValue::Identifier(name) => {
                self.mark_binding_use(name);
            }
            LValue::IndexAccess { array, index } => {
                self.visit_expression(array);
                self.visit_expression(index);
            }
            LValue::FieldAccess { object, .. } => {
                self.visit_expression(object);
            }
        }
        self.visit_expression(&assignment.value);
    }

    fn declare_let_pattern_bindings(
        &mut self,
        pattern: &ast::Pattern,
        ty: Option<&TypeAnnotation>,
        span: Span,
    ) {
        match pattern {
            ast::Pattern::Identifier(name, _) => {
                let exact_num =
                    ty.and_then(|annotation| ExactNum::from_annotation(Some(annotation)));
                self.declare_binding(name.clone(), span, BindingKind::Variable, exact_num);
            }
            ast::Pattern::Tuple(elements) => {
                for element in elements {
                    self.declare_let_pattern_bindings(element, None, span);
                }
            }
            ast::Pattern::Struct { fields, .. } => {
                for (_, pattern) in fields {
                    self.declare_let_pattern_bindings(pattern, None, span);
                }
            }
            ast::Pattern::EnumVariant {
                data, struct_data, ..
            } => {
                if let Some(patterns) = data {
                    for pattern in patterns {
                        self.declare_let_pattern_bindings(pattern, None, span);
                    }
                }
                if let Some(fields) = struct_data {
                    for (_, pattern) in fields {
                        self.declare_let_pattern_bindings(pattern, None, span);
                    }
                }
            }
            ast::Pattern::Or(patterns) => {
                if let Some(first) = patterns.first() {
                    self.declare_let_pattern_bindings(first, None, span);
                }
            }
            ast::Pattern::Wildcard(_) | ast::Pattern::Literal(_) => {}
        }
    }

    fn visit_expression(&mut self, expression: &Expression) {
        if self.enter_visit_depth(expression.span).is_err() {
            return;
        }
        self.visit_expression_inner(expression);
        self.exit_visit_depth();
    }

    fn visit_expression_inner(&mut self, expression: &Expression) {
        match &expression.kind {
            ExpressionKind::Identifier(name) => {
                self.mark_binding_use(name);
            }
            ExpressionKind::NumberLiteral(_)
            | ExpressionKind::StringLiteral(_)
            | ExpressionKind::BoolLiteral(_) => {}
            ExpressionKind::Binary { left, right, .. } => {
                self.visit_expression(left);
                self.visit_expression(right);
            }
            ExpressionKind::Unary { operand, .. } => {
                self.visit_expression(operand);
            }
            ExpressionKind::Call { callee, arguments } => {
                self.visit_expression(callee);
                for arg in arguments {
                    self.visit_expression(arg);
                }
            }
            ExpressionKind::If {
                condition,
                then_block,
                elif_blocks,
                else_block,
            } => {
                self.visit_expression(condition);
                self.visit_block(then_block, true);
                for (elif_condition, elif_block) in elif_blocks {
                    self.visit_expression(elif_condition);
                    self.visit_block(elif_block, true);
                }
                if let Some(block) = else_block {
                    self.visit_block(block, true);
                }
            }
            ExpressionKind::Unless {
                condition,
                then_block,
                else_block,
            } => {
                self.visit_expression(condition);
                self.visit_block(then_block, true);
                if let Some(block) = else_block {
                    self.visit_block(block, true);
                }
            }
            ExpressionKind::Grouping(inner) => {
                self.visit_expression(inner);
            }
            ExpressionKind::ArrayLiteral { elements } => {
                for element in elements {
                    self.visit_expression(element);
                }
            }
            ExpressionKind::IndexAccess { array, index } => {
                self.visit_expression(array);
                self.visit_expression(index);
            }
            ExpressionKind::TupleLiteral { elements } => {
                for element in elements {
                    self.visit_expression(element);
                }
            }
            ExpressionKind::TupleAccess { tuple, .. } => {
                self.visit_expression(tuple);
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                for (_, value) in fields {
                    self.visit_expression(value);
                }
            }
            ExpressionKind::FieldAccess { object, .. } => {
                self.visit_expression(object);
            }
            ExpressionKind::EnumVariant {
                data, struct_data, ..
            } => {
                if let Some(values) = data {
                    for value in values {
                        self.visit_expression(value);
                    }
                }
                if let Some(fields) = struct_data {
                    for (_, value) in fields {
                        self.visit_expression(value);
                    }
                }
            }
            ExpressionKind::Match { scrutinee, arms } => {
                self.visit_expression(scrutinee);
                for arm in arms {
                    self.enter_scope();
                    self.visit_expression(&arm.body);
                    self.exit_scope();
                }
            }
            ExpressionKind::MethodCall {
                object, arguments, ..
            } => {
                self.visit_expression(object);
                for argument in arguments {
                    self.visit_expression(argument);
                }
            }
            ExpressionKind::CharLiteral(_) => {}
            ExpressionKind::FString(parts) => {
                for part in parts {
                    if let crate::ast::FStringPart::Interpolated(expr) = part {
                        self.visit_expression(expr);
                    }
                }
            }
            ExpressionKind::Lambda { body, .. } => {
                self.visit_expression(body);
            }
            ExpressionKind::Try(inner) | ExpressionKind::Await(inner) => {
                self.visit_expression(inner);
            }
            ExpressionKind::Range { start, end, .. } => {
                self.visit_expression(start);
                self.visit_expression(end);
            }
            ExpressionKind::Block(block) => {
                self.visit_block(block, true);
            }
            ExpressionKind::DifferentiableBlock(block) => {
                self.visit_block(block, true);
            }
            ExpressionKind::AsyncBlock(block) => {
                self.visit_block(block, true);
            }
            ExpressionKind::Cast {
                expr,
                target_type,
                mode,
            } => {
                self.visit_expression(expr);
                // BEGIN narrowing-cast lint (CastLint)
                self.check_narrowing_cast(expression, expr, target_type, *mode);
            }
        }
    }

    fn declare_binding(
        &mut self,
        name: String,
        span: Span,
        kind: BindingKind,
        exact_num: Option<ExactNum>,
    ) {
        if self.scope_stack.is_empty() {
            self.enter_scope();
        }

        if name.starts_with('_') {
            let binding = Binding {
                span,
                kind,
                used: false,
                allow_unused: true,
                exact_num,
            };
            if let Some(scope) = self.scope_stack.last_mut() {
                scope.bindings.insert(name, binding);
            }
            return;
        }

        if self.options.is_enabled(LintRule::Shadowing) {
            if let Some(previous) = self.find_in_outer_scopes(&name) {
                let note = format!(
                    "previous binding declared at line {}",
                    previous.span.start_location.line
                );
                self.diagnostics.push(LintDiagnostic {
                    rule: LintRule::Shadowing,
                    message: format!("binding '{}' shadows a previous binding", name),
                    span,
                    note: Some(note),
                    secondary_span: Some(previous.span),
                });
            }
        }

        let allow_unused = name == "_";
        let binding = Binding {
            span,
            kind,
            used: false,
            allow_unused,
            exact_num,
        };

        if let Some(scope) = self.scope_stack.last_mut() {
            scope.bindings.insert(name, binding);
        }
    }

    fn mark_binding_use(&mut self, name: &str) {
        for scope in self.scope_stack.iter_mut().rev() {
            if let Some(binding) = scope.bindings.get_mut(name) {
                binding.used = true;
                break;
            }
        }
    }

    fn enter_scope(&mut self) {
        self.scope_stack.push(Scope::default());
    }

    fn exit_scope(&mut self) {
        if let Some(scope) = self.scope_stack.pop() {
            if self.options.is_enabled(LintRule::UnusedBinding) {
                for (name, binding) in scope.bindings.iter() {
                    if binding.used || binding.allow_unused {
                        continue;
                    }

                    self.diagnostics.push(LintDiagnostic {
                        rule: LintRule::UnusedBinding,
                        message: format!("{} '{}' is never used", binding.kind.description(), name),
                        span: binding.span,
                        note: None,
                        secondary_span: None,
                    });
                }
            }
        }
    }

    fn emit_unreachable(&mut self, span: Span, cause: Option<Span>) {
        if !self.options.is_enabled(LintRule::UnreachableCode) {
            return;
        }

        let note = cause.map(|terminator| {
            format!(
                "control flow never reaches this statement because the previous statement at line {} terminates the block",
                terminator.start_location.line
            )
        });

        self.diagnostics.push(LintDiagnostic {
            rule: LintRule::UnreachableCode,
            message: "unreachable code".to_string(),
            span,
            note,
            secondary_span: cause,
        });
    }

    fn find_in_outer_scopes(&self, name: &str) -> Option<&Binding> {
        self.scope_stack
            .iter()
            .rev()
            .skip(1)
            .find_map(|scope| scope.bindings.get(name))
    }
}

struct Binding {
    span: Span,
    kind: BindingKind,
    used: bool,
    allow_unused: bool,
    /// Exact-width numeric type resolved from the binding's annotation, when
    /// declared with one (`i8`, `u32`, `f32`, ...). Used by the narrowing-cast
    /// lint to reason about cast source widths without full type inference.
    exact_num: Option<ExactNum>,
}

#[derive(Default)]
struct Scope {
    bindings: HashMap<String, Binding>,
}

#[derive(Clone, Copy)]
enum BindingKind {
    Parameter,
    Variable,
    ForIterator,
}

impl BindingKind {
    fn description(&self) -> &'static str {
        match self {
            BindingKind::Parameter => "parameter",
            BindingKind::Variable => "variable",
            BindingKind::ForIterator => "loop variable",
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers for `loop` termination analysis
// ---------------------------------------------------------------------------

/// Returns `true` if `block` contains a `break` statement at any depth,
/// **except** inside nested `loop`, `while`, `do-while`, or `for` bodies
/// (those `break`s would exit the *inner* loop, not the one being analysed).
///
/// `pub(crate)`: the semantic return-path analysis reuses this exact
/// break-reachability logic so lint reachability and guaranteed-return can
/// never disagree about what a `loop { ... }` body may do.
pub(crate) fn block_has_break(block: &Block) -> bool {
    block.statements.iter().any(stmt_has_break)
}

pub(crate) fn stmt_has_break(stmt: &Statement) -> bool {
    match &stmt.kind {
        StatementKind::Break => true,
        // Descend into switch cases — a `break` inside exits *this* loop.
        StatementKind::Switch(sw) => {
            sw.cases.iter().any(|c| block_has_break(&c.body))
                || sw.default.as_ref().is_some_and(block_has_break)
        }
        // `if`/`unless`/`match`/block statements parse as expression
        // statements; a `break` inside them still targets *this* loop.
        StatementKind::Expression(expr) => expr_has_break(expr),
        // `if let` is a dedicated statement kind; its arms are not loops.
        StatementKind::IfLet(stmt) => {
            block_has_break(&stmt.then_block)
                || stmt.else_block.as_ref().is_some_and(block_has_break)
        }
        // Do NOT descend into nested loops — their `break` belongs to them.
        StatementKind::Loop(_)
        | StatementKind::While(_)
        | StatementKind::DoWhile(_)
        | StatementKind::For(_) => false,
        // Other statements cannot directly contain a `break`.
        _ => false,
    }
}

/// Descends into expression-position control flow to find `break` statements
/// that target the enclosing loop.
fn expr_has_break(expr: &Expression) -> bool {
    match &expr.kind {
        ExpressionKind::If {
            then_block,
            elif_blocks,
            else_block,
            ..
        } => {
            block_has_break(then_block)
                || elif_blocks.iter().any(|(_, block)| block_has_break(block))
                || else_block.as_ref().is_some_and(block_has_break)
        }
        ExpressionKind::Unless {
            then_block,
            else_block,
            ..
        } => {
            block_has_break(then_block) || else_block.as_ref().is_some_and(block_has_break)
        }
        ExpressionKind::Match { arms, .. } => {
            arms.iter().any(|arm| expr_has_break(&arm.body))
        }
        ExpressionKind::Block(block)
        | ExpressionKind::AsyncBlock(block)
        | ExpressionKind::DifferentiableBlock(block) => block_has_break(block),
        ExpressionKind::Grouping(inner) => expr_has_break(inner),
        _ => false,
    }
}

// BEGIN narrowing-cast lint (CastLint)
// Real child module so the rule can hook into the private LintRunner below
// (descendant privacy). Implementation lives in ../semantic/semantic_cast_lint.rs.
#[path = "../semantic/semantic_cast_lint.rs"]
mod semantic_cast_lint;

use crate::ast::TypeAnnotation;
use semantic_cast_lint::ExactNum;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expression, ExpressionKind, Statement, StatementKind};
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::span::{Location, Span};

    fn parse(source: &str) -> Module {
        let tokens = Lexer::new(source)
            .tokenize()
            .expect("lexer should succeed in lint tests");
        Parser::new(tokens)
            .parse()
            .expect("parser should succeed in lint tests")
    }

    fn lint_codes(source: &str) -> Result<Vec<LintRule>, crate::error::SemanticError> {
        lint_module(&parse(source), &LintOptions::all()).map(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.rule)
                .collect()
        })
    }

    #[test]
    fn loop_with_conditional_break_marks_following_code_reachable() {
        // Regression: `if` statements parse as expression statements, and the
        // break-reachability scan used to miss them, so every `loop` looked
        // infinite and everything after it was flagged as unreachable.
        let rules = lint_codes(
            r#"
            module demo

            func classify(flag: bool) returns int {
                let counter = 0
                loop {
                    if flag {
                        break
                    }
                    counter = counter + 1
                }
                let after = counter
                return after
            }
        "#,
        )
        .expect("lint walk must not trip the recursion guard");

        assert!(
            !rules.contains(&LintRule::UnreachableCode),
            "code after a loop with a conditional break is reachable, got {rules:?}"
        );
    }

    #[test]
    fn loop_with_match_break_marks_following_code_reachable() {
        let rules = lint_codes(
            r#"
            module demo

            func pick(value: int) returns int {
                loop {
                    match value {
                        when 0 then {
                            break
                        },
                        otherwise then {
                            break
                        },
                    }
                }
                return 0
            }
        "#,
        )
        .expect("lint walk must not trip the recursion guard");

        assert!(
            !rules.contains(&LintRule::UnreachableCode),
            "code after a loop broken from a match arm is reachable, got {rules:?}"
        );
    }

    #[test]
    fn infinite_loop_still_marks_following_code_unreachable() {
        // Control: the fix must not weaken the genuine detection.
        let rules = lint_codes(
            r#"
            module demo

            func forever() returns int {
                loop {
                    let tick = 1
                }
                return 0
            }
        "#,
        )
        .expect("lint walk must not trip the recursion guard");

        assert!(
            rules.contains(&LintRule::UnreachableCode),
            "code after an infinite loop must still be flagged, got {rules:?}"
        );
    }

    fn deep_grouping_expression(depth: usize) -> Expression {
        let mut expression = Expression {
            span: Span::new(0, 1, Location::new(1, 1), Location::new(1, 2)),
            kind: ExpressionKind::NumberLiteral("1".to_string()),
        };
        for _ in 0..depth {
            expression = Expression {
                span: expression.span,
                kind: ExpressionKind::Grouping(Box::new(expression)),
            };
        }
        expression
    }

    #[test]
    fn deeply_nested_ast_fails_with_coded_guard_instead_of_overflow() {
        let mut module = parse(
            "
            module demo

            func main() {
                let x = 1
            }
        ",
        );
        let Item::Function(function) = &mut module.items[0] else {
            panic!("expected function item");
        };
        function.body.statements = vec![Statement {
            span: Span::new(0, 1, Location::new(1, 1), Location::new(1, 2)),
            kind: StatementKind::Expression(deep_grouping_expression(50_000)),
        }];

        let error = lint_module(&module, &LintOptions::all())
            .expect_err("deep AST must fail the lint walk with a coded diagnostic");
        assert_eq!(
            error.code.as_deref(),
            Some("P013"),
            "expected the shared P013 nesting guard: {error:?}"
        );
        // The 50k-deep Box chain would recurse just as deeply in its `Drop`,
        // which is unrelated to the walk being tested; leaking it keeps the
        // test focused on the guard.
        std::mem::forget(module);
    }
}
