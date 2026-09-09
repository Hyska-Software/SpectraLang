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
    DeprecatedTaskSpawn,
    DeprecatedTextEmbed,
    DeprecatedOnnxExport,
}

impl LintRule {
    pub fn code(&self) -> &'static str {
        match self {
            LintRule::UnusedBinding => "unused-binding",
            LintRule::UnreachableCode => "unreachable-code",
            LintRule::Shadowing => "shadowing",
            LintRule::NarrowingCast => "narrowing-cast",
            LintRule::DeprecatedTaskSpawn => "deprecated-task-spawn",
            LintRule::DeprecatedTextEmbed => "deprecated-text-embed",
            LintRule::DeprecatedOnnxExport => "deprecated-onnx-export",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            LintRule::UnusedBinding => "unused binding",
            LintRule::UnreachableCode => "unreachable code",
            LintRule::Shadowing => "shadowed binding",
            LintRule::NarrowingCast => "narrowing numeric cast",
            LintRule::DeprecatedTaskSpawn => "deprecated concurrent.task_spawn call",
            LintRule::DeprecatedTextEmbed => "deprecated ml.text_embed hashing-baseline call",
            LintRule::DeprecatedOnnxExport => "deprecated ml.onnx_export fixture-template call",
        }
    }

    pub fn all() -> &'static [LintRule] {
        const ALL: &[LintRule] = &[
            LintRule::UnusedBinding,
            LintRule::UnreachableCode,
            LintRule::Shadowing,
            LintRule::NarrowingCast,
            LintRule::DeprecatedTaskSpawn,
            LintRule::DeprecatedTextEmbed,
            LintRule::DeprecatedOnnxExport,
        ];
        ALL
    }

    pub fn from_code(value: &str) -> Option<Self> {
        match value {
            "unused-binding" | "unused_binding" => Some(LintRule::UnusedBinding),
            "unreachable-code" | "unreachable_code" => Some(LintRule::UnreachableCode),
            "shadowing" => Some(LintRule::Shadowing),
            "narrowing-cast" | "narrowing_cast" => Some(LintRule::NarrowingCast),
            "deprecated-task-spawn" | "deprecated_task_spawn" => Some(LintRule::DeprecatedTaskSpawn),
            "deprecated-text-embed" | "deprecated_text_embed" => Some(LintRule::DeprecatedTextEmbed),
            "deprecated-onnx-export" | "deprecated_onnx_export" => Some(LintRule::DeprecatedOnnxExport),
            _ => None,
        }
    }

    /// Stable error code assigned when this rule is escalated to a hard error
    /// via `--deny`. `None` for rules without a reserved code yet.
    pub fn stable_error_code(&self) -> Option<&'static str> {
        match self {
            LintRule::NarrowingCast => Some("E035"),
            LintRule::DeprecatedTaskSpawn => Some("E036"),
            LintRule::DeprecatedTextEmbed => Some("E037"),
            LintRule::DeprecatedOnnxExport => Some("E038"),
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

pub fn lint_module(module: &Module, options: &LintOptions) -> Vec<LintDiagnostic> {
    if options.enabled.is_empty() {
        return Vec::new();
    }

    LintRunner::new(options).run(module)
}

struct LintRunner<'a> {
    options: &'a LintOptions,
    diagnostics: Vec<LintDiagnostic>,
    scope_stack: Vec<Scope>,
}

impl<'a> LintRunner<'a> {
    fn new(options: &'a LintOptions) -> Self {
        Self {
            options,
            diagnostics: Vec::new(),
            scope_stack: Vec::new(),
        }
    }

    fn run(mut self, module: &Module) -> Vec<LintDiagnostic> {
        for item in &module.items {
            self.visit_item(item);
        }

        self.diagnostics
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
            self.declare_binding(param.name.clone(), param.span, BindingKind::Parameter, exact_num);
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
        match &statement.kind {
            StatementKind::Let(let_stmt) => {
                if let Some(value) = &let_stmt.value {
                    self.visit_expression(value);
                }
                self.declare_let_pattern_bindings(&let_stmt.pattern, let_stmt.ty.as_ref(), let_stmt.span);
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
                // A `loop {}` without any `break` is an infinite loop — code after
                // it is unreachable.  We check via a simple structural scan.
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
                let exact_num = ty.and_then(|annotation| ExactNum::from_annotation(Some(annotation)));
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
                // BEGIN deprecated-task-spawn lint (DeprecateSpawn)
                self.check_deprecated_task_spawn(callee);
                // END deprecated-task-spawn lint (DeprecateSpawn)
                // BEGIN deprecated-text-embed lint (ML)
                self.check_deprecated_text_embed(callee);
                // END deprecated-text-embed lint (ML)
                // BEGIN deprecated-onnx-export lint (ML)
                self.check_deprecated_onnx_export(callee);
                // END deprecated-onnx-export lint (ML)
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
                // BEGIN deprecated-task-spawn lint (DeprecateSpawn)
                self.check_deprecated_task_spawn(expression);
                // END deprecated-task-spawn lint (DeprecateSpawn)
                // BEGIN deprecated-text-embed lint (ML)
                self.check_deprecated_text_embed(expression);
                // END deprecated-text-embed lint (ML)
                // BEGIN deprecated-onnx-export lint (ML)
                self.check_deprecated_onnx_export(expression);
                // END deprecated-onnx-export lint (ML)
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
fn block_has_break(block: &Block) -> bool {
    block.statements.iter().any(stmt_has_break)
}

fn stmt_has_break(stmt: &Statement) -> bool {
    match &stmt.kind {
        StatementKind::Break => true,
        // Descend into switch cases — a `break` inside exits *this* loop.
        StatementKind::Switch(sw) => {
            sw.cases.iter().any(|c| block_has_break(&c.body))
                || sw.default.as_ref().is_some_and(block_has_break)
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

// BEGIN narrowing-cast lint (CastLint)
// Real child module so the rule can hook into the private LintRunner below
// (descendant privacy). Implementation lives in ../semantic/semantic_cast_lint.rs.
#[path = "../semantic/semantic_cast_lint.rs"]
mod semantic_cast_lint;

#[path = "../semantic/semantic_deprecated_spawn_lint.rs"]
mod semantic_deprecated_spawn_lint;

use crate::ast::TypeAnnotation;
use semantic_cast_lint::ExactNum;
