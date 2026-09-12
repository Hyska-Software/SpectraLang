use super::*;

impl SemanticAnalyzer {
    pub(crate) fn tensor_call_name(callee: &Expression) -> Option<String> {
        let path = namespace_path(callee)?;
        let parts = path.split('.').collect::<Vec<_>>();
        match parts.as_slice() {
            ["tensor", name] => Some((*name).to_string()),
            ["std", "tensor", name] => Some((*name).to_string()),
            ["spectra", "std", "tensor", name] => Some((*name).to_string()),
            _ => None,
        }
    }

    pub(crate) fn tensor_dims(ty: &Type) -> Option<&[Option<usize>]> {
        match ty {
            Type::Tensor {
                dims: Some(dims), ..
            } => Some(dims.as_slice()),
            _ => None,
        }
    }

    pub(crate) fn tensor_dim_slices_match(left: &[Option<usize>], right: &[Option<usize>]) -> bool {
        left.len() == right.len()
            && left
                .iter()
                .zip(right.iter())
                .all(|(a, b)| a.is_none() || b.is_none() || a == b)
    }

    pub(crate) fn known_element_count(dims: &[Option<usize>]) -> Option<usize> {
        dims.iter()
            .try_fold(1usize, |acc, dim| Some(acc.saturating_mul((*dim)?)))
    }

    pub(crate) fn const_int_expression(expr: &Expression) -> Option<i64> {
        match &expr.kind {
            ExpressionKind::NumberLiteral(raw) => match crate::numeric::parse_number_literal(raw) {
                Some(crate::numeric::ParsedNumber::Int(v)) => Some(v),
                _ => None,
            },
            ExpressionKind::Grouping(inner) => Self::const_int_expression(inner),
            _ => None,
        }
    }

    pub(crate) fn validate_differentiable_block_operations(&mut self, block: &Block) {
        for stmt in &block.statements {
            self.validate_differentiable_statement(stmt);
        }
    }

    fn validate_differentiable_statement(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::Let(let_stmt) => {
                if let Some(value) = &let_stmt.value {
                    self.validate_differentiable_expression(value);
                }
            }
            StatementKind::Expression(expr) => self.validate_differentiable_expression(expr),
            StatementKind::Return(ret) => {
                if let Some(value) = &ret.value {
                    self.validate_differentiable_expression(value);
                }
            }
            StatementKind::Assignment(assign) => {
                self.validate_differentiable_lvalue(&assign.target);
                self.validate_differentiable_expression(&assign.value);
            }
            StatementKind::While(while_stmt) => {
                self.validate_differentiable_expression(&while_stmt.condition);
                self.validate_differentiable_block_operations(&while_stmt.body);
            }
            StatementKind::For(for_stmt) => {
                self.validate_differentiable_expression(&for_stmt.iterable);
                self.validate_differentiable_block_operations(&for_stmt.body);
            }
            StatementKind::Loop(loop_stmt) => {
                self.validate_differentiable_block_operations(&loop_stmt.body);
            }
            StatementKind::DoWhile(do_while) => {
                self.validate_differentiable_block_operations(&do_while.body);
                self.validate_differentiable_expression(&do_while.condition);
            }
            StatementKind::Switch(switch_stmt) => {
                self.validate_differentiable_expression(&switch_stmt.value);
                for case in &switch_stmt.cases {
                    self.validate_differentiable_expression(&case.pattern);
                    self.validate_differentiable_block_operations(&case.body);
                }
                if let Some(block) = &switch_stmt.default {
                    self.validate_differentiable_block_operations(block);
                }
            }
            StatementKind::IfLet(if_let) => {
                self.validate_differentiable_expression(&if_let.value);
                self.validate_differentiable_block_operations(&if_let.then_block);
                if let Some(block) = &if_let.else_block {
                    self.validate_differentiable_block_operations(block);
                }
            }
            StatementKind::WhileLet(while_let) => {
                self.validate_differentiable_expression(&while_let.value);
                self.validate_differentiable_block_operations(&while_let.body);
            }
            StatementKind::Break | StatementKind::Continue => {}
        }
    }

    fn validate_differentiable_lvalue(&mut self, target: &crate::ast::LValue) {
        match target {
            crate::ast::LValue::Identifier(_) => {}
            crate::ast::LValue::IndexAccess { array, index } => {
                self.validate_differentiable_expression(array);
                self.validate_differentiable_expression(index);
            }
            crate::ast::LValue::FieldAccess { object, .. } => {
                self.validate_differentiable_expression(object);
            }
        }
    }

    fn validate_differentiable_expression(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::Call { callee, arguments } => {
                if let Some(path) = namespace_path(callee) {
                    self.validate_differentiable_path(&path, expr.span);
                }
                self.validate_differentiable_expression(callee);
                for arg in arguments {
                    self.validate_differentiable_expression(arg);
                }
            }
            ExpressionKind::MethodCall {
                object,
                method_name,
                arguments,
                ..
            } => {
                if let Some(path) = namespace_path(object) {
                    let qualified = format!("{}.{}", path, method_name);
                    self.validate_differentiable_path(&qualified, expr.span);
                }
                self.validate_differentiable_expression(object);
                for arg in arguments {
                    self.validate_differentiable_expression(arg);
                }
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.validate_differentiable_expression(left);
                self.validate_differentiable_expression(right);
            }
            ExpressionKind::Unary { operand, .. }
            | ExpressionKind::Try(operand)
            | ExpressionKind::Await(operand)
            | ExpressionKind::Grouping(operand) => {
                self.validate_differentiable_expression(operand);
            }
            ExpressionKind::If {
                condition,
                then_block,
                elif_blocks,
                else_block,
            } => {
                self.validate_differentiable_expression(condition);
                self.validate_differentiable_block_operations(then_block);
                for (condition, block) in elif_blocks {
                    self.validate_differentiable_expression(condition);
                    self.validate_differentiable_block_operations(block);
                }
                if let Some(block) = else_block {
                    self.validate_differentiable_block_operations(block);
                }
            }
            ExpressionKind::Unless {
                condition,
                then_block,
                else_block,
            } => {
                self.validate_differentiable_expression(condition);
                self.validate_differentiable_block_operations(then_block);
                if let Some(block) = else_block {
                    self.validate_differentiable_block_operations(block);
                }
            }
            ExpressionKind::Block(block)
            | ExpressionKind::DifferentiableBlock(block)
            | ExpressionKind::AsyncBlock(block) => {
                self.validate_differentiable_block_operations(block);
            }
            ExpressionKind::ArrayLiteral { elements }
            | ExpressionKind::TupleLiteral { elements } => {
                for element in elements {
                    self.validate_differentiable_expression(element);
                }
            }
            ExpressionKind::IndexAccess { array, index } => {
                self.validate_differentiable_expression(array);
                self.validate_differentiable_expression(index);
            }
            ExpressionKind::FieldAccess { object, .. }
            | ExpressionKind::TupleAccess { tuple: object, .. } => {
                self.validate_differentiable_expression(object);
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                for (_, value) in fields {
                    self.validate_differentiable_expression(value);
                }
            }
            ExpressionKind::EnumVariant {
                data, struct_data, ..
            } => {
                if let Some(values) = data {
                    for value in values {
                        self.validate_differentiable_expression(value);
                    }
                }
                if let Some(fields) = struct_data {
                    for (_, value) in fields {
                        self.validate_differentiable_expression(value);
                    }
                }
            }
            ExpressionKind::Match { scrutinee, arms } => {
                self.validate_differentiable_expression(scrutinee);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.validate_differentiable_expression(guard);
                    }
                    self.validate_differentiable_expression(&arm.body);
                }
            }
            ExpressionKind::Lambda { body, .. } => self.validate_differentiable_expression(body),
            ExpressionKind::Cast { expr, .. } => self.validate_differentiable_expression(expr),
            ExpressionKind::FString(parts) => {
                for part in parts {
                    if let FStringPart::Interpolated(expr) = part {
                        self.validate_differentiable_expression(expr);
                    }
                }
            }
            ExpressionKind::Range { start, end, .. } => {
                self.validate_differentiable_expression(start);
                self.validate_differentiable_expression(end);
            }
            ExpressionKind::Identifier(_)
            | ExpressionKind::NumberLiteral(_)
            | ExpressionKind::StringLiteral(_)
            | ExpressionKind::BoolLiteral(_)
            | ExpressionKind::CharLiteral(_) => {}
        }
    }

    fn validate_differentiable_path(&mut self, path: &str, span: Span) {
        let normalized = path
            .strip_prefix("spectra.")
            .unwrap_or(path)
            .strip_prefix("std.")
            .unwrap_or(path);
        let parts = normalized.split('.').collect::<Vec<_>>();
        let supported = matches!(
            parts.as_slice(),
            [
                "tensor",
                "add"
                    | "sub"
                    | "mul"
                    | "div"
                    | "sum_t"
                    | "mean_t"
                    | "dot_t"
                    | "matmul"
                    | "matmul_batched"
                    | "transpose"
                    | "neg"
                    | "relu"
                    | "exp_f"
                    | "log_f"
                    | "sqrt_f"
                    | "sigmoid_f"
                    | "tanh_f"
                    | "concat"
                    | "stack"
                    | "slice"
                    | "permute"
            ] | [
                "ml",
                "linear"
                    | "mse_loss"
                    | "bce_loss"
                    | "cross_entropy_loss"
                    | "nll_loss"
                    | "conv2d"
                    | "max_pool2d"
                    | "dropout"
            ]
        );
        let is_std = matches!(parts.first(), Some(&"tensor" | &"ml" | &"io" | &"math"));
        if is_std && !supported {
            self.error_coded_with_hint(
                "E1406",
                format!(
                    "Operation '{}' is not supported inside a differentiable block",
                    path
                ),
                span,
                "Move metadata, I/O, lifecycle, or unsupported stdlib calls outside `diff { ... }`; keep differentiable tensor/ML math such as tensor.mul, tensor.sum_t, tensor.matmul, ml.linear, and loss functions inside.",
            );
        }
    }

    pub(crate) fn parse_tensor_metadata(
        type_args: &[crate::ast::TypeAnnotation],
    ) -> TensorMetadata {
        let mut meta = TensorMetadata::default();
        let mut dims = Vec::new();

        for ann in type_args {
            let Some(name) = Self::single_segment_annotation(ann) else {
                continue;
            };

            if let Some(rank) = name
                .strip_prefix("rank")
                .and_then(|raw| raw.parse::<usize>().ok())
            {
                meta.rank = Some(rank);
                continue;
            }

            if let Some(dim) = name
                .strip_prefix("dim")
                .and_then(|raw| raw.parse::<usize>().ok())
            {
                dims.push(Some(dim));
                continue;
            }

            match name.as_str() {
                "dyn" | "dynamic_dim" | "dim_dynamic" => dims.push(None),
                "dynamic" => meta.rank = None,
                "row_major" | "col_major" | "contiguous" | "strided" => {
                    meta.layout = Some(name);
                }
                "cpu" | "wgpu" | "cuda" | "rocm" | "metal" | "directml" | "vulkan" => {
                    meta.device = Some(name);
                }
                _ => {}
            }
        }

        if !dims.is_empty() {
            if meta.rank.is_none() {
                meta.rank = Some(dims.len());
            }
            meta.dims = Some(dims);
        }

        meta
    }

    fn single_segment_annotation(ann: &crate::ast::TypeAnnotation) -> Option<String> {
        use crate::ast::TypeAnnotationKind;
        match &ann.kind {
            TypeAnnotationKind::Simple { segments } if segments.len() == 1 => {
                Some(segments[0].clone())
            }
            _ => None,
        }
    }

    pub(crate) fn branch_type_mismatch(&self, types: &[Type]) -> Option<(Type, Type)> {
        let mut reference: Option<Type> = None;

        for ty in types {
            if matches!(ty, Type::Unknown) {
                continue;
            }

            if let Some(ref expected) = reference {
                if !self.types_compatible(expected, ty) {
                    return Some((expected.clone(), ty.clone()));
                }
            } else {
                reference = Some(ty.clone());
            }
        }

        None
    }

    pub(crate) fn first_non_unknown_type(&self, types: &[Type]) -> Option<Type> {
        types
            .iter()
            .find(|ty| !matches!(ty, Type::Unknown))
            .cloned()
    }

    pub(crate) fn infer_block_type(&mut self, block: &Block) -> Type {
        if self.block_guaranteed_return(block) {
            return Type::Unknown;
        }

        if let Some(last_stmt) = block.statements.last() {
            match &last_stmt.kind {
                StatementKind::Expression(expr) => self.infer_expression_type(expr),
                StatementKind::Return(ret) => {
                    if let Some(value) = &ret.value {
                        self.infer_expression_type(value);
                    }
                    Type::Unknown
                }
                _ => Type::Unit,
            }
        } else {
            Type::Unit
        }
    }

    pub(crate) fn analyze_const_decl(&mut self, decl: &ConstDecl) {
        self.analyze_expression(&decl.value);
        let Some(value) = self.eval_const_expression(&decl.value) else {
            self.error_with_hint(
                format!(
                    "Const '{}' must be initialized with a compile-time constant",
                    decl.name
                ),
                decl.value.span,
                "Use literals, other constants, casts, and pure arithmetic/logical expressions.",
            );
            return;
        };

        let inferred = value.ty();
        let declared = decl
            .ty
            .as_ref()
            .map(|_| self.type_annotation_to_type_checked(&decl.ty))
            .unwrap_or_else(|| inferred.clone());

        if let Err(message) = Self::const_value_fits_type(&value, &declared) {
            self.error_coded("E2903", message, decl.span);
            return;
        }

        if !self.types_match(&inferred, &declared) {
            self.error_with_hint(
                format!(
                    "Const '{}' has type {}, but {} was declared",
                    decl.name,
                    type_name(&inferred),
                    type_name(&declared)
                ),
                decl.span,
                "Change the annotation or cast the initializer explicitly.",
            );
            return;
        }

        if !self.declare_symbol(decl.name.clone(), decl.span, declared) {
            self.error_coded(
                "E002",
                format!("Const '{}' is already declared in this scope", decl.name),
                decl.span,
            );
        }
        self.const_values.insert(decl.name.clone(), value);
    }

    fn const_value_fits_type(value: &ConstValue, target: &Type) -> Result<(), String> {
        match (value, target) {
            (ConstValue::Int(value), Type::ExactInt { signed, width }) => {
                let bits = match width {
                    IntWidth::I8 => 8,
                    IntWidth::I16 => 16,
                    IntWidth::I32 => 32,
                    IntWidth::I64 | IntWidth::Isize | IntWidth::Usize => 64,
                };
                let fits = if *signed {
                    let min = -(1_i128 << (bits - 1));
                    let max = (1_i128 << (bits - 1)) - 1;
                    (*value as i128) >= min && (*value as i128) <= max
                } else {
                    *value >= 0 && (*value as u128) <= ((1_u128 << bits) - 1)
                };
                if fits {
                    Ok(())
                } else {
                    Err(format!(
                        "E2903: constant {} does not fit in {}",
                        value,
                        type_name(target)
                    ))
                }
            }
            (
                ConstValue::Float(value),
                Type::ExactFloat {
                    width: FloatWidth::F32,
                },
            ) => {
                if value.is_finite() && (*value as f32 as f64) == *value {
                    Ok(())
                } else {
                    Err(format!(
                        "E2904: constant {} is not representable as f32",
                        value
                    ))
                }
            }
            (
                ConstValue::Float(value),
                Type::ExactFloat {
                    width: FloatWidth::F64,
                },
            ) if value.is_finite() => Ok(()),
            _ => Ok(()),
        }
    }

    pub(crate) fn analyze_static_decl(&mut self, decl: &StaticDecl) {
        self.analyze_expression(&decl.value);
        let Some(value) = self.eval_const_expression(&decl.value) else {
            self.error_coded_with_hint(
                "E2902",
                format!(
                    "Static '{}' must be initialized with a compile-time constant",
                    decl.name
                ),
                decl.value.span,
                "Use literals, constants, casts, and pure arithmetic/logical expressions; runtime calls are not valid static initializers.",
            );
            return;
        };

        let inferred = value.ty();
        let declared = decl
            .ty
            .as_ref()
            .map(|_| self.type_annotation_to_type_checked(&decl.ty))
            .unwrap_or_else(|| inferred.clone());

        if let Err(message) = Self::const_value_fits_type(&value, &declared) {
            self.error_coded("E2903", message, decl.span);
            return;
        }

        if !matches!(
            declared,
            Type::Int
                | Type::Float
                | Type::Bool
                | Type::Char
                | Type::ExactInt { .. }
                | Type::ExactFloat { .. }
        ) {
            self.error_coded_with_hint(
                "E2905",
                format!(
                    "Static '{}' currently supports only scalar numeric, boolean, and character types",
                    decl.name
                ),
                decl.span,
                "Keep aggregate, string, tensor, and handle globals out of the beta static surface until their storage/drop contract is defined.",
            );
            return;
        }

        if !self.types_match(&inferred, &declared) {
            self.error_with_hint(
                format!(
                    "Static '{}' has type {}, but {} was declared",
                    decl.name,
                    type_name(&inferred),
                    type_name(&declared)
                ),
                decl.span,
                "Change the annotation or initializer so the static has one concrete type.",
            );
        }

        if !self.declare_symbol(decl.name.clone(), decl.span, declared) {
            self.error_coded(
                "E002",
                format!("Static '{}' is already declared in this scope", decl.name),
                decl.span,
            );
        }
    }

    pub(crate) fn eval_const_expression(&self, expr: &Expression) -> Option<ConstValue> {
        match &expr.kind {
            ExpressionKind::NumberLiteral(raw) => match crate::numeric::parse_number_literal(raw) {
                Some(crate::numeric::ParsedNumber::Int(v)) => Some(ConstValue::Int(v)),
                Some(crate::numeric::ParsedNumber::Float(v)) => Some(ConstValue::Float(v)),
                None => None,
            },
            ExpressionKind::StringLiteral(value) => Some(ConstValue::String(value.clone())),
            ExpressionKind::BoolLiteral(value) => Some(ConstValue::Bool(*value)),
            ExpressionKind::CharLiteral(value) => Some(ConstValue::Char(*value)),
            ExpressionKind::Identifier(name) => self.const_values.get(name).cloned(),
            ExpressionKind::Grouping(inner) => self.eval_const_expression(inner),
            ExpressionKind::Unary { operator, operand } => {
                let value = self.eval_const_expression(operand)?;
                match (operator, value) {
                    (UnaryOperator::Negate, ConstValue::Int(v)) => Some(ConstValue::Int(-v)),
                    (UnaryOperator::Negate, ConstValue::Float(v)) => Some(ConstValue::Float(-v)),
                    (UnaryOperator::Not, ConstValue::Bool(v)) => Some(ConstValue::Bool(!v)),
                    _ => None,
                }
            }
            ExpressionKind::Binary {
                left,
                operator,
                right,
            } => {
                let left = self.eval_const_expression(left)?;
                let right = self.eval_const_expression(right)?;
                self.eval_const_binary(left, *operator, right)
            }
            ExpressionKind::Cast {
                expr: inner,
                target_type,
                mode: _,
                ..
            } => {
                let value = self.eval_const_expression(inner)?;
                let target = self.type_annotation_to_type(&Some(target_type.clone()));
                self.cast_const_value(value, &target)
            }
            _ => None,
        }
    }

    fn eval_const_binary(
        &self,
        left: ConstValue,
        operator: BinaryOperator,
        right: ConstValue,
    ) -> Option<ConstValue> {
        use ConstValue::*;

        match operator {
            BinaryOperator::Add => match (left, right) {
                (Int(a), Int(b)) => Some(Int(a + b)),
                (Float(a), Float(b)) => Some(Float(a + b)),
                (Int(a), Float(b)) => Some(Float(a as f64 + b)),
                (Float(a), Int(b)) => Some(Float(a + b as f64)),
                (String(a), String(b)) => Some(String(format!("{}{}", a, b))),
                _ => None,
            },
            BinaryOperator::Subtract => match (left, right) {
                (Int(a), Int(b)) => Some(Int(a - b)),
                (Float(a), Float(b)) => Some(Float(a - b)),
                (Int(a), Float(b)) => Some(Float(a as f64 - b)),
                (Float(a), Int(b)) => Some(Float(a - b as f64)),
                _ => None,
            },
            BinaryOperator::Multiply => match (left, right) {
                (Int(a), Int(b)) => Some(Int(a * b)),
                (Float(a), Float(b)) => Some(Float(a * b)),
                (Int(a), Float(b)) => Some(Float(a as f64 * b)),
                (Float(a), Int(b)) => Some(Float(a * b as f64)),
                _ => None,
            },
            BinaryOperator::Divide => match (left, right) {
                (Int(_), Int(0)) | (Float(_), Float(0.0)) => None,
                (Int(a), Int(b)) => Some(Int(a / b)),
                (Float(a), Float(b)) => Some(Float(a / b)),
                (Int(a), Float(b)) if b != 0.0 => Some(Float(a as f64 / b)),
                (Float(a), Int(b)) if b != 0 => Some(Float(a / b as f64)),
                _ => None,
            },
            BinaryOperator::Modulo => match (left, right) {
                (Int(_), Int(0)) => None,
                (Int(a), Int(b)) => Some(Int(a % b)),
                _ => None,
            },
            BinaryOperator::Equal => Some(Bool(self.const_values_equal(&left, &right))),
            BinaryOperator::NotEqual => Some(Bool(!self.const_values_equal(&left, &right))),
            BinaryOperator::Less => self.eval_const_order(left, right, |a, b| a < b),
            BinaryOperator::LessEqual => self.eval_const_order(left, right, |a, b| a <= b),
            BinaryOperator::Greater => self.eval_const_order(left, right, |a, b| a > b),
            BinaryOperator::GreaterEqual => self.eval_const_order(left, right, |a, b| a >= b),
            BinaryOperator::And => match (left, right) {
                (Bool(a), Bool(b)) => Some(Bool(a && b)),
                _ => None,
            },
            BinaryOperator::Or => match (left, right) {
                (Bool(a), Bool(b)) => Some(Bool(a || b)),
                _ => None,
            },
        }
    }

    fn eval_const_order(
        &self,
        left: ConstValue,
        right: ConstValue,
        cmp: impl FnOnce(f64, f64) -> bool,
    ) -> Option<ConstValue> {
        let left = self.const_value_as_f64(&left)?;
        let right = self.const_value_as_f64(&right)?;
        Some(ConstValue::Bool(cmp(left, right)))
    }

    fn const_value_as_f64(&self, value: &ConstValue) -> Option<f64> {
        match value {
            ConstValue::Int(v) => Some(*v as f64),
            ConstValue::Float(v) => Some(*v),
            _ => None,
        }
    }

    fn const_values_equal(&self, left: &ConstValue, right: &ConstValue) -> bool {
        match (left, right) {
            (ConstValue::Int(a), ConstValue::Int(b)) => a == b,
            (ConstValue::Float(a), ConstValue::Float(b)) => a == b,
            (ConstValue::Int(a), ConstValue::Float(b)) => (*a as f64) == *b,
            (ConstValue::Float(a), ConstValue::Int(b)) => *a == (*b as f64),
            (ConstValue::Bool(a), ConstValue::Bool(b)) => a == b,
            (ConstValue::String(a), ConstValue::String(b)) => a == b,
            (ConstValue::Char(a), ConstValue::Char(b)) => a == b,
            _ => false,
        }
    }

    fn cast_const_value(&self, value: ConstValue, target: &Type) -> Option<ConstValue> {
        match (value, target) {
            (ConstValue::Int(v), Type::Int) => Some(ConstValue::Int(v)),
            (ConstValue::Int(v), Type::Float) => Some(ConstValue::Float(v as f64)),
            (ConstValue::Int(v), Type::Char) => char::from_u32(v as u32).map(ConstValue::Char),
            (ConstValue::Float(v), Type::Float) => Some(ConstValue::Float(v)),
            (ConstValue::Float(v), Type::Int) => Some(ConstValue::Int(v as i64)),
            (ConstValue::Int(v), Type::ExactInt { signed, width }) => {
                let bits = match width {
                    IntWidth::I8 => 8,
                    IntWidth::I16 => 16,
                    IntWidth::I32 => 32,
                    IntWidth::I64 | IntWidth::Isize | IntWidth::Usize => 64,
                };
                let fits = if *signed {
                    let min = -(1_i128 << (bits - 1));
                    let max = (1_i128 << (bits - 1)) - 1;
                    (v as i128) >= min && (v as i128) <= max
                } else {
                    v >= 0 && (v as u128) <= ((1_u128 << bits) - 1)
                };
                fits.then_some(ConstValue::Int(v))
            }
            (ConstValue::Float(v), Type::ExactInt { signed, width })
                if v.is_finite() && v.fract() == 0.0 =>
            {
                let int = v as i128;
                let bits = match width {
                    IntWidth::I8 => 8,
                    IntWidth::I16 => 16,
                    IntWidth::I32 => 32,
                    IntWidth::I64 | IntWidth::Isize | IntWidth::Usize => 64,
                };
                let fits = if *signed {
                    let min = -(1_i128 << (bits - 1));
                    let max = (1_i128 << (bits - 1)) - 1;
                    int >= min && int <= max
                } else {
                    int >= 0 && (int as u128) <= ((1_u128 << bits) - 1)
                };
                fits.then_some(ConstValue::Int(int as i64))
            }
            (
                ConstValue::Int(v),
                Type::ExactFloat {
                    width: FloatWidth::F32,
                },
            ) if (v as f32 as f64) == v as f64 => Some(ConstValue::Float(v as f32 as f64)),
            (
                ConstValue::Int(v),
                Type::ExactFloat {
                    width: FloatWidth::F64,
                },
            ) => Some(ConstValue::Float(v as f64)),
            (
                ConstValue::Float(v),
                Type::ExactFloat {
                    width: FloatWidth::F32,
                },
            ) if v.is_finite() && (v as f32 as f64) == v => {
                Some(ConstValue::Float(v as f32 as f64))
            }
            (
                ConstValue::Float(v),
                Type::ExactFloat {
                    width: FloatWidth::F64,
                },
            ) if v.is_finite() => Some(ConstValue::Float(v)),
            (ConstValue::Char(v), Type::Char) => Some(ConstValue::Char(v)),
            (ConstValue::Char(v), Type::Int) => Some(ConstValue::Int(v as i64)),
            (ConstValue::Bool(v), Type::Bool) => Some(ConstValue::Bool(v)),
            (ConstValue::String(v), Type::String) => Some(ConstValue::String(v)),
            _ => None,
        }
    }
}
