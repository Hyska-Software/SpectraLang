use super::*;

impl ASTLowering {
    pub(crate) fn source_span(&self, span: Span) -> SourceSpan {
        SourceSpan {
            file: self.source_file.clone(),
            start_line: span.start_location.line as u32,
            start_column: span.start_location.column as u32,
            end_line: span.end_location.line as u32,
            end_column: span.end_location.column as u32,
        }
    }
    pub fn new() -> Self {
        let mut lowering = Self {
            source_file: "<unknown>".to_string(),
            builder: IRBuilder::new(),
            current_function: None,
            value_map: ScopeStack::new(),
            variable_types: TypeScopeStack::new(),
            alloca_map: HashMap::new(),
            array_map: ArrayScopeStack::new(),
            range_map: RangeScopeStack::new(),
            struct_definitions: HashMap::new(),
            struct_var_map: StructScopeStack::new(),
            enum_definitions: HashMap::new(),
            enum_variant_field_names: HashMap::new(),
            loop_stack: Vec::new(),
            generic_functions: HashMap::new(),
            generic_structs: HashMap::new(),
            generic_enums: HashMap::new(),
            type_aliases: HashMap::new(),
            pending_specializations: Vec::new(),
            generated_specializations: HashMap::new(),
            type_substitution_map: HashMap::new(),
            generic_impl_methods: HashMap::new(),
            instantiated_structs: HashMap::new(),
            pending_method_specializations: Vec::new(),
            trait_implementations: HashMap::new(),
            function_return_types: HashMap::new(),
            function_parameter_types: HashMap::new(),
            std_import_aliases: HashMap::new(),
            lambda_counter: 0,
            pending_lambdas: Vec::new(),
            closure_var_map: HashMap::new(),
            pending_coroutines: Vec::new(),
            current_function_return_annotation: None,
            current_async_output_type: None,
            lowering_async_poll: false,
            current_expected_annotation: None,
            trait_method_order: HashMap::new(),
            trait_method_signatures: HashMap::new(),
            trait_declarations: HashMap::new(),
            errors: Vec::new(),
            const_values: HashMap::new(),
            static_globals: HashMap::new(),
            drop_excluded_names: HashSet::new(),
        };
        lowering.register_builtin_error_struct();
        lowering.register_builtin_generic_enums();
        lowering.register_builtin_async_traits();
        lowering.register_builtin_api_traits();
        lowering
    }

    pub fn set_source_file(&mut self, file: impl Into<String>) {
        self.source_file = file.into();
    }

    pub(crate) fn register_builtin_async_traits(&mut self) {
        self.trait_method_order.insert(
            "Future".to_string(),
            vec!["poll".to_string(), "cancel".to_string()],
        );
        self.trait_method_order.insert(
            "Stream".to_string(),
            vec!["next".to_string(), "cancel".to_string()],
        );
        self.trait_method_signatures.insert(
            "Future".to_string(),
            HashMap::from([
                (
                    "poll".to_string(),
                    (
                        Vec::new(),
                        IRType::Task {
                            output: Box::new(IRType::Int),
                        },
                    ),
                ),
                ("cancel".to_string(), (Vec::new(), IRType::Int)),
            ]),
        );
        self.trait_method_signatures.insert(
            "Stream".to_string(),
            HashMap::from([
                (
                    "next".to_string(),
                    (
                        Vec::new(),
                        IRType::Task {
                            output: Box::new(IRType::Int),
                        },
                    ),
                ),
                ("cancel".to_string(), (Vec::new(), IRType::Int)),
            ]),
        );
    }

    /// Register the public std.api trait contracts that are supplied by the
    /// compiler's builtin module registry rather than as local AST traits.
    ///
    /// API handles are opaque integer values in the midend, so Request and
    /// Response both lower to `Int` here.  Keeping the async result as
    /// `Task<Int>` is essential for dyn AsyncHandler: the backend vtable call
    /// must preserve the task boundary for a subsequent `await` (R-2113).
    pub(crate) fn register_builtin_api_traits(&mut self) {
        let api_handle = IRType::Int;
        let api_task = |output: IRType| IRType::Task {
            output: Box::new(output),
        };

        self.trait_method_order.insert(
            "IntoResponse".to_string(),
            vec!["into_response".to_string()],
        );
        self.trait_method_signatures.insert(
            "IntoResponse".to_string(),
            HashMap::from([(
                "into_response".to_string(),
                (Vec::new(), api_handle.clone()),
            )]),
        );

        self.trait_method_order
            .insert("Handler".to_string(), vec!["call".to_string()]);
        self.trait_method_signatures.insert(
            "Handler".to_string(),
            HashMap::from([(
                "call".to_string(),
                (vec![api_handle.clone()], IRType::Int),
            )]),
        );

        self.trait_method_order
            .insert("AsyncHandler".to_string(), vec!["call".to_string()]);
        self.trait_method_signatures.insert(
            "AsyncHandler".to_string(),
            HashMap::from([(
                "call".to_string(),
                (vec![api_handle.clone()], api_task(IRType::Int)),
            )]),
        );

        self.trait_method_order.insert(
            "Middleware".to_string(),
            vec!["on_request".to_string(), "on_response".to_string()],
        );
        self.trait_method_signatures.insert(
            "Middleware".to_string(),
            HashMap::from([
                (
                    "on_request".to_string(),
                    (vec![api_handle.clone()], IRType::Int),
                ),
                (
                    "on_response".to_string(),
                    (vec![api_handle.clone()], IRType::Int),
                ),
            ]),
        );

        self.trait_method_order.insert(
            "AsyncMiddleware".to_string(),
            vec!["on_request".to_string(), "on_response".to_string()],
        );
        self.trait_method_signatures.insert(
            "AsyncMiddleware".to_string(),
            HashMap::from([
                (
                    "on_request".to_string(),
                    (vec![api_handle.clone()], api_task(IRType::Int)),
                ),
                (
                    "on_response".to_string(),
                    (vec![api_handle], api_task(IRType::Int)),
                ),
            ]),
        );
    }

    pub(crate) fn register_trait_metadata(&mut self, trait_decl: &TraitDeclaration) {
        self.trait_declarations
            .insert(trait_decl.name.clone(), trait_decl.clone());
        self.trait_method_order.insert(
            trait_decl.name.clone(),
            trait_decl.methods.iter().map(|method| method.name.clone()).collect(),
        );
        let signatures = trait_decl
            .methods
            .iter()
            .map(|method| {
                let params = method
                    .params
                    .iter()
                    .filter(|param| !param.is_self)
                    .filter_map(|param| param.type_annotation.as_ref())
                    .map(|ann| self.lower_type_annotation(ann))
                    .collect::<Vec<_>>();
                let ret = method
                    .return_type
                    .as_ref()
                    .map(|ann| self.lower_type_annotation(ann))
                    .unwrap_or(IRType::Void);
                let return_type = if method.is_async {
                    IRType::Task {
                        output: Box::new(ret),
                    }
                } else {
                    ret
                };
                (method.name.clone(), (params, return_type))
            })
            .collect::<HashMap<_, _>>();
        self.trait_method_signatures
            .insert(trait_decl.name.clone(), signatures);
    }

    /// Register the concrete layout used by `std.error.Error` host values.
    ///
    /// The semantic module owns the public record contract; the midend keeps
    /// this small intrinsic definition available even when a module imports
    /// `std.fs` without importing `std.error` solely to inspect an error.
    pub(crate) fn register_builtin_error_struct(&mut self) {
        self.struct_definitions.insert(
            "Error".to_string(),
            vec![
                ("code".to_string(), IRType::Int),
                ("message".to_string(), IRType::String),
                ("operation".to_string(), IRType::String),
                ("context".to_string(), IRType::String),
                ("origin".to_string(), IRType::String),
                ("retryable".to_string(), IRType::Bool),
            ],
        );
        self.enum_definitions.insert(
            "ErrorCode".to_string(),
            [
                "InvalidArgument",
                "NotFound",
                "PermissionDenied",
                "Io",
                "Internal",
                "Unsupported",
            ]
            .into_iter()
            .enumerate()
            .map(|(tag, name)| (name.to_string(), tag, None))
            .collect(),
        );
    }

    /// Pre-register `Option<T>` and `Result<T, E>` as built-in generic enums so
    /// that user code doesn't need to declare them.
    pub(crate) fn register_builtin_generic_enums(&mut self) {
        let dummy = Span::dummy();

        let make_type_param = |name: &str| TypeParameter {
            name: name.to_string(),
            bounds: vec![],
            span: dummy,
        };

        let simple_type_ann = |name: &str| TypeAnnotation {
            kind: TypeAnnotationKind::Simple {
                segments: vec![name.to_string()],
            },
            span: dummy,
        };

        // ── Option<T> ──────────────────────────────────────────────────────────
        let option_enum = ASTEnum {
            name: "Option".to_string(),
            span: dummy,
            visibility: Visibility::Public,
            attributes: Vec::new(),
            type_params: vec![make_type_param("T")],
            variants: vec![
                EnumVariant {
                    // Keep the ABI tag aligned with runtime std.option:
                    // Some = 0 and None = 1.
                    name: "Some".to_string(),
                    span: dummy,
                    attributes: Vec::new(),
                    data: Some(vec![simple_type_ann("T")]),
                    struct_data: None,
                },
                EnumVariant {
                    name: "None".to_string(),
                    span: dummy,
                    attributes: Vec::new(),
                    data: None,
                    struct_data: None,
                },
            ],
        };
        self.generic_enums.insert("Option".to_string(), option_enum);

        // ── Result<T, E> ───────────────────────────────────────────────────────
        let result_enum = ASTEnum {
            name: "Result".to_string(),
            span: dummy,
            visibility: Visibility::Public,
            attributes: Vec::new(),
            type_params: vec![make_type_param("T"), make_type_param("E")],
            variants: vec![
                EnumVariant {
                    name: "Ok".to_string(),
                    span: dummy,
                    attributes: Vec::new(),
                    data: Some(vec![simple_type_ann("T")]),
                    struct_data: None,
                },
                EnumVariant {
                    name: "Err".to_string(),
                    span: dummy,
                    attributes: Vec::new(),
                    data: Some(vec![simple_type_ann("E")]),
                    struct_data: None,
                },
            ],
        };
        self.generic_enums.insert("Result".to_string(), result_enum);
    }

    pub(crate) fn error(&mut self, message: impl Into<String>) {
        self.errors.push(MidendError::new(message));
    }

    /// Report an impossible lowering state and return a non-emittable poison
    /// value. `lower_module` rejects the module whenever `errors` is non-empty,
    /// so this sentinel can never reach verification or backend codegen.
    pub(crate) fn invalid_value(&mut self, message: impl Into<String>) -> Value {
        self.error(message);
        Value { id: usize::MAX }
    }

    pub(crate) fn require_value(&mut self, value: Option<Value>, message: impl Into<String>) -> Value {
        match value {
            Some(value) => value,
            None => self.invalid_value(message),
        }
    }

    pub(crate) fn eval_const_expression(&self, expr: &Expression) -> Option<LoweredConstValue> {
        match &expr.kind {
            ExpressionKind::NumberLiteral(raw) => {
                match spectra_compiler::numeric::parse_number_literal(raw) {
                    Some(spectra_compiler::numeric::ParsedNumber::Int(v)) => {
                        Some(LoweredConstValue::Int(v))
                    }
                    Some(spectra_compiler::numeric::ParsedNumber::Float(v)) => {
                        Some(LoweredConstValue::Float(v))
                    }
                    None => None,
                }
            }
            ExpressionKind::StringLiteral(value) => Some(LoweredConstValue::String(value.clone())),
            ExpressionKind::BoolLiteral(value) => Some(LoweredConstValue::Bool(*value)),
            ExpressionKind::CharLiteral(value) => Some(LoweredConstValue::Char(*value)),
            ExpressionKind::Identifier(name) => self.const_values.get(name).cloned(),
            ExpressionKind::Grouping(inner) => self.eval_const_expression(inner),
            ExpressionKind::Unary { operator, operand } => {
                let value = self.eval_const_expression(operand)?;
                match (operator, value) {
                    (UnaryOperator::Negate, LoweredConstValue::Int(v)) => {
                        Some(LoweredConstValue::Int(-v))
                    }
                    (UnaryOperator::Negate, LoweredConstValue::Float(v)) => {
                        Some(LoweredConstValue::Float(-v))
                    }
                    (UnaryOperator::Not, LoweredConstValue::Bool(v)) => {
                        Some(LoweredConstValue::Bool(!v))
                    }
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
                ..
            } => {
                let value = self.eval_const_expression(inner)?;
                let target = self.lower_type_annotation(target_type);
                self.cast_const_value(value, &target)
            }
            _ => None,
        }
    }

    pub(crate) fn eval_const_binary(
        &self,
        left: LoweredConstValue,
        operator: BinaryOperator,
        right: LoweredConstValue,
    ) -> Option<LoweredConstValue> {
        use LoweredConstValue::*;

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
                (Int(_), Int(0)) => None,
                (Int(a), Int(b)) => Some(Int(a / b)),
                (Float(a), Float(b)) if b != 0.0 => Some(Float(a / b)),
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

    pub(crate) fn eval_const_order(
        &self,
        left: LoweredConstValue,
        right: LoweredConstValue,
        cmp: impl FnOnce(f64, f64) -> bool,
    ) -> Option<LoweredConstValue> {
        Some(LoweredConstValue::Bool(cmp(
            self.const_value_as_f64(&left)?,
            self.const_value_as_f64(&right)?,
        )))
    }

    pub(crate) fn const_value_as_f64(&self, value: &LoweredConstValue) -> Option<f64> {
        match value {
            LoweredConstValue::Int(v) => Some(*v as f64),
            LoweredConstValue::Float(v) => Some(*v),
            _ => None,
        }
    }

    pub(crate) fn const_values_equal(&self, left: &LoweredConstValue, right: &LoweredConstValue) -> bool {
        match (left, right) {
            (LoweredConstValue::Int(a), LoweredConstValue::Int(b)) => a == b,
            (LoweredConstValue::Float(a), LoweredConstValue::Float(b)) => a == b,
            (LoweredConstValue::Int(a), LoweredConstValue::Float(b)) => (*a as f64) == *b,
            (LoweredConstValue::Float(a), LoweredConstValue::Int(b)) => *a == (*b as f64),
            (LoweredConstValue::Bool(a), LoweredConstValue::Bool(b)) => a == b,
            (LoweredConstValue::String(a), LoweredConstValue::String(b)) => a == b,
            (LoweredConstValue::Char(a), LoweredConstValue::Char(b)) => a == b,
            _ => false,
        }
    }

    pub(crate) fn cast_const_value(
        &self,
        value: LoweredConstValue,
        target: &IRType,
    ) -> Option<LoweredConstValue> {
        match (value, target) {
            (LoweredConstValue::Int(v), IRType::Int) => Some(LoweredConstValue::Int(v)),
            (LoweredConstValue::Int(v), IRType::Float) => Some(LoweredConstValue::Float(v as f64)),
            (LoweredConstValue::Int(v), IRType::ExactInt { signed, width }) => {
                let (min, max) = Self::exact_int_bounds(*signed, *width);
                (min..=max).contains(&v).then_some(LoweredConstValue::Int(v))
            }
            (LoweredConstValue::Int(v), IRType::ExactFloat { .. }) => {
                Some(LoweredConstValue::Float(v as f64))
            }
            (LoweredConstValue::Int(v), IRType::Char) => {
                char::from_u32(v as u32).map(LoweredConstValue::Char)
            }
            (LoweredConstValue::Float(v), IRType::Float) => Some(LoweredConstValue::Float(v)),
            (LoweredConstValue::Float(v), IRType::Int) => Some(LoweredConstValue::Int(v as i64)),
            (LoweredConstValue::Float(v), IRType::ExactFloat { .. }) => {
                Some(LoweredConstValue::Float(v))
            }
            (LoweredConstValue::Float(v), IRType::ExactInt { signed, width }) => {
                if !v.is_finite() || v.fract() != 0.0 {
                    return None;
                }
                let (min, max) = Self::exact_int_bounds(*signed, *width);
                let value = v as i128;
                (value >= min as i128 && value <= max as i128)
                    .then_some(LoweredConstValue::Int(value as i64))
            }
            (LoweredConstValue::Char(v), IRType::Char) => Some(LoweredConstValue::Char(v)),
            (LoweredConstValue::Char(v), IRType::Int) => Some(LoweredConstValue::Int(v as i64)),
            (LoweredConstValue::Char(v), IRType::ExactInt { signed, width }) => {
                let value = v as i64;
                let (min, max) = Self::exact_int_bounds(*signed, *width);
                (min..=max).contains(&value).then_some(LoweredConstValue::Int(value))
            }
            (LoweredConstValue::Bool(v), IRType::Bool) => Some(LoweredConstValue::Bool(v)),
            (LoweredConstValue::String(v), IRType::String) => Some(LoweredConstValue::String(v)),
            _ => None,
        }
    }

    pub(crate) fn exact_int_bounds(signed: bool, width: IRIntWidth) -> (i64, i64) {
        match (signed, width) {
            (true, IRIntWidth::I8) => (i8::MIN as i64, i8::MAX as i64),
            (true, IRIntWidth::I16) => (i16::MIN as i64, i16::MAX as i64),
            (true, IRIntWidth::I32) => (i32::MIN as i64, i32::MAX as i64),
            (true, IRIntWidth::I64 | IRIntWidth::Isize | IRIntWidth::Usize) => (i64::MIN, i64::MAX),
            (false, IRIntWidth::I8) => (0, u8::MAX as i64),
            (false, IRIntWidth::I16) => (0, u16::MAX as i64),
            (false, IRIntWidth::I32) => (0, u32::MAX as i64),
            (false, IRIntWidth::I64 | IRIntWidth::Isize | IRIntWidth::Usize) => (0, i64::MAX),
        }
    }

    pub(crate) fn emit_const_value(&mut self, value: &LoweredConstValue, ir_func: &mut IRFunction) -> Value {
        match value {
            LoweredConstValue::Int(v) => self.builder.build_const_int(ir_func, *v),
            LoweredConstValue::Float(v) => self.builder.build_const_float(ir_func, *v),
            LoweredConstValue::Bool(v) => self.builder.build_const_bool(ir_func, *v),
            LoweredConstValue::String(v) => self.lower_string_literal(v, ir_func),
            LoweredConstValue::Char(v) => self.builder.build_const_int(ir_func, *v as i64),
        }
    }

    pub(crate) fn lowered_const_to_ir_constant(value: &LoweredConstValue) -> Constant {
        match value {
            LoweredConstValue::Int(v) => Constant::Int(*v),
            LoweredConstValue::Float(v) => Constant::Float(*v),
            LoweredConstValue::Bool(v) => Constant::Bool(*v),
            LoweredConstValue::String(v) => Constant::String(v.clone()),
            LoweredConstValue::Char(v) => Constant::Char(*v),
        }
    }

}
