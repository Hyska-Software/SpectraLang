use super::*;

impl SemanticAnalyzer {
    pub(crate) fn infer_expression_type(&mut self, expr: &Expression) -> Type {
        match &expr.kind {
            ExpressionKind::NumberLiteral(num) => {
                if crate::numeric::number_literal_is_float(num) {
                    Type::Float
                } else {
                    Type::Int
                }
            }
            ExpressionKind::StringLiteral(_) => Type::String,
            ExpressionKind::BoolLiteral(_) => Type::Bool,
            ExpressionKind::Identifier(name) => {
                if let Some(info) = self.lookup_symbol(name) {
                    info.ty.clone()
                } else if let Some(sig) = self.functions.get(name) {
                    // Fallback for functions not yet registered in the symbol
                    // table — return the full Fn type, not just return_type.
                    Type::Fn {
                        params: sig.params.clone(),
                        return_type: Box::new(sig.return_type.clone()),
                    }
                } else {
                    Type::Unknown
                }
            }
            ExpressionKind::Binary {
                left,
                operator,
                right,
            } => {
                let left_type = self.infer_expression_type(left);
                let right_type = self.infer_expression_type(right);

                use crate::ast::BinaryOperator;
                match operator {
                    BinaryOperator::Add => {
                        if matches!(left_type, Type::String) || matches!(right_type, Type::String) {
                            Type::String
                        } else if let Some(sn) = self.nominal_lookup_name(&left_type) {
                            // Operator overloading: preserve the applied receiver type while
                            // using the compatibility key used by the trait registry.
                            if self.trait_impls.contains_key(&("Add".to_string(), sn)) {
                                left_type
                            } else {
                                self.numeric_result_type(&left_type, &right_type)
                            }
                        } else {
                            self.numeric_result_type(&left_type, &right_type)
                        }
                    }
                    BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
                    | BinaryOperator::Modulo => {
                        if let Some(sn) = self.nominal_lookup_name(&left_type) {
                            let trait_name = match operator {
                                BinaryOperator::Subtract => "Sub",
                                BinaryOperator::Multiply => "Mul",
                                BinaryOperator::Divide => "Div",
                                _ => "Rem",
                            };
                            if self.trait_impls.contains_key(&(trait_name.to_string(), sn)) {
                                return left_type;
                            }
                        }
                        self.numeric_result_type(&left_type, &right_type)
                    }
                    BinaryOperator::Equal
                    | BinaryOperator::NotEqual
                    | BinaryOperator::Less
                    | BinaryOperator::Greater
                    | BinaryOperator::LessEqual
                    | BinaryOperator::GreaterEqual
                    | BinaryOperator::And
                    | BinaryOperator::Or => Type::Bool,
                }
            }
            ExpressionKind::Unary { operand, .. } => self.infer_expression_type(operand),
            ExpressionKind::Call { callee, arguments } => {
                if let ExpressionKind::Identifier(name) = &callee.kind {
                    if let Some(return_type) = self.std_generic_unwrap_return(name, arguments) {
                        return return_type;
                    }
                    if name == "block_on" {
                        return arguments
                            .first()
                            .map(|arg| match self.infer_expression_type(arg) {
                                Type::Task { output } => *output,
                                Type::Unknown => Type::Unknown,
                                _ => Type::Unknown,
                            })
                            .unwrap_or(Type::Unknown);
                    }
                    if let Some(sig) = self.functions.get(name).cloned() {
                        let substitutions =
                            self.infer_type_parameter_substitutions(&sig.params, arguments);
                        return self.substitute_type_parameters(&sig.return_type, &substitutions);
                    }
                }
                if let Type::Fn { return_type, .. } = self.infer_expression_type(callee) {
                    return (*return_type).clone();
                }
                Type::Unknown
            }
            ExpressionKind::If {
                then_block,
                elif_blocks,
                else_block,
                ..
            } => {
                let mut branch_types = Vec::new();
                branch_types.push(self.infer_block_type(then_block));
                for (_, elif_block) in elif_blocks {
                    branch_types.push(self.infer_block_type(elif_block));
                }
                branch_types.push(match else_block {
                    Some(block) => self.infer_block_type(block),
                    None => Type::Unit,
                });

                if self.branch_type_mismatch(&branch_types).is_some() {
                    Type::Unknown
                } else {
                    self.first_non_unknown_type(&branch_types)
                        .unwrap_or(Type::Unknown)
                }
            }
            ExpressionKind::Unless {
                then_block,
                else_block,
                ..
            } => {
                let mut branch_types = Vec::new();
                branch_types.push(self.infer_block_type(then_block));
                branch_types.push(match else_block {
                    Some(block) => self.infer_block_type(block),
                    None => Type::Unit,
                });

                if self.branch_type_mismatch(&branch_types).is_some() {
                    Type::Unknown
                } else {
                    self.first_non_unknown_type(&branch_types)
                        .unwrap_or(Type::Unknown)
                }
            }
            ExpressionKind::Grouping(inner) => self.infer_expression_type(inner),
            ExpressionKind::ArrayLiteral { elements } => {
                if elements.is_empty() {
                    match self.current_expected_type.clone() {
                        Some(Type::Array { element_type, .. })
                            if !matches!(element_type.as_ref(), Type::Unknown) =>
                        {
                            Type::Array {
                                element_type,
                                size: Some(0),
                            }
                        }
                        _ => Type::Unknown,
                    }
                } else {
                    let elem_type = self.infer_expression_type(&elements[0]);
                    Type::Array {
                        element_type: Box::new(elem_type),
                        size: Some(elements.len()),
                    }
                }
            }
            ExpressionKind::IndexAccess { array, .. } => {
                let array_type = self.infer_expression_type(array);
                match array_type {
                    Type::Array { element_type, .. } => *element_type,
                    _ => Type::Unknown,
                }
            }
            ExpressionKind::TupleLiteral { elements } => {
                if elements.is_empty() {
                    Type::Tuple { elements: vec![] }
                } else {
                    let element_types: Vec<Type> = elements
                        .iter()
                        .map(|e| self.infer_expression_type(e))
                        .collect();
                    Type::Tuple {
                        elements: element_types,
                    }
                }
            }
            ExpressionKind::TupleAccess { tuple, index } => {
                let tuple_type = self.infer_expression_type(tuple);
                match tuple_type {
                    Type::Tuple { elements } => {
                        if *index < elements.len() {
                            elements[*index].clone()
                        } else {
                            Type::Unknown
                        }
                    }
                    _ => Type::Unknown,
                }
            }
            ExpressionKind::StructLiteral {
                name,
                type_args,
                fields,
            } => {
                if self.struct_infos.contains_key(name) {
                    let inferred_type_args = if type_args.is_empty() {
                        self.generic_structs
                            .get(name)
                            .cloned()
                            .map(|(type_params, field_defs)| {
                                self.infer_struct_type_args(&type_params, &field_defs, fields)
                            })
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    let effective_type_args = if type_args.is_empty() {
                        inferred_type_args.as_slice()
                    } else {
                        type_args.as_slice()
                    };
                    if self.generic_structs.contains_key(name) {
                        let args = effective_type_args
                            .iter()
                            .map(|arg| self.type_annotation_to_type(&Some(arg.clone())))
                            .collect::<Vec<_>>();
                        if args.is_empty() {
                            Type::Unknown
                        } else {
                            Type::Applied {
                                name: name.clone(),
                                args,
                            }
                        }
                    } else {
                        Type::Struct { name: name.clone() }
                    }
                } else {
                    Type::Unknown
                }
            }
            ExpressionKind::FieldAccess { object, field } => {
                let object_type = self.infer_expression_type(object);
                match object_type.clone() {
                    Type::Struct { .. } | Type::Applied { .. } => self
                        .specialized_struct_context_for_type(&object_type)
                        .and_then(|(_, info, substitutions)| {
                            info.fields.get(field).map(|field_info| {
                                if substitutions.is_empty() {
                                    self.type_annotation_to_type(&Some(field_info.ty.clone()))
                                } else {
                                    self.type_annotation_to_type_with_substitutions(
                                        &field_info.ty,
                                        &substitutions,
                                    )
                                }
                            })
                        })
                        .unwrap_or(Type::Unknown),
                    _ => Type::Unknown,
                }
            }
            ExpressionKind::EnumVariant {
                enum_name,
                variant_name,
                type_args,
                data,
                struct_data,
                module_path: _,
            } => {
                if self.generic_enums.contains_key(enum_name) {
                    if !type_args.is_empty() {
                        return Type::Applied {
                            name: enum_name.clone(),
                            args: type_args
                                .iter()
                                .map(|arg| self.type_annotation_to_type(&Some(arg.clone())))
                                .collect(),
                        };
                    }

                    if let Some(Type::Applied {
                        name: expected_name,
                        args,
                    }) = self.current_expected_type.clone()
                    {
                        if expected_name == *enum_name {
                            return Type::Applied {
                                name: expected_name,
                                args,
                            };
                        }
                    }

                    if let Some(args) = data {
                        let inferred_args =
                            self.infer_enum_type_args(enum_name, variant_name, args.as_slice());
                        if !inferred_args.is_empty() {
                            return Type::Applied {
                                name: enum_name.clone(),
                                args: inferred_args
                                    .iter()
                                    .map(|arg| self.type_annotation_to_type(&Some(arg.clone())))
                                    .collect(),
                            };
                        }
                    }

                    if let Some(fields) = struct_data {
                        let inferred_args = self.infer_enum_type_args_from_named_fields(
                            enum_name,
                            variant_name,
                            fields,
                        );
                        if let Some(inferred_args) = inferred_args {
                            return Type::Applied {
                                name: enum_name.clone(),
                                args: inferred_args
                                    .iter()
                                    .map(|arg| self.type_annotation_to_type(&Some(arg.clone())))
                                    .collect(),
                            };
                        }
                    }

                    Type::Applied {
                        name: enum_name.clone(),
                        args: self
                            .generic_enums
                            .get(enum_name)
                            .map(|(params, _)| {
                                params
                                    .iter()
                                    .map(|param| Type::TypeParameter {
                                        name: param.name.clone(),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    }
                } else if self.enum_infos.contains_key(enum_name) {
                    Type::Enum {
                        name: enum_name.clone(),
                    }
                } else if self.struct_infos.contains_key(enum_name.as_str()) {
                    // Static method call on a struct: StructName::method(...)
                    // JSON derive methods are generated metadata rather than
                    // user-authored impl items. Keep their result types
                    // explicit here so `json_error_field(...)` cannot fall
                    // back to the receiver struct type during expression
                    // inference.
                    if variant_name == "json_error_field" {
                        return Type::String;
                    }
                    if variant_name == "from_json" {
                        return Type::Struct {
                            name: enum_name.clone(),
                        };
                    }
                    self.methods
                        .get(enum_name.as_str())
                        .and_then(|mm| mm.get(variant_name.as_str()))
                        .map(|sig| sig.return_type.clone())
                        .unwrap_or(Type::Struct {
                            name: enum_name.clone(),
                        })
                } else {
                    // Fallback: the semantic pass may have resolved this as a
                    // cross-module function call and recorded the type.
                    self.symbol_resolutions
                        .get(&expr.span)
                        .map(|info| info.ty.clone())
                        .unwrap_or(Type::Unknown)
                }
            }
            ExpressionKind::Match { scrutinee: _, arms } => {
                if arms.is_empty() {
                    return Type::Unknown;
                }

                let scrutinee_type = match &expr.kind {
                    ExpressionKind::Match { scrutinee, .. } => {
                        self.infer_expression_type(scrutinee)
                    }
                    _ => Type::Unknown,
                };

                let arm_types: Vec<Type> = arms
                    .iter()
                    .map(|arm| {
                        self.push_scope();
                        self.register_pattern_bindings(&arm.pattern);
                        self.bind_pattern_types(&arm.pattern, &scrutinee_type);
                        let arm_type = self.infer_expression_type(&arm.body);
                        self.pop_scope();
                        arm_type
                    })
                    .collect();

                if self.branch_type_mismatch(&arm_types).is_some() {
                    Type::Unknown
                } else {
                    self.first_non_unknown_type(&arm_types)
                        .unwrap_or(Type::Unknown)
                }
            }
            ExpressionKind::MethodCall {
                object,
                method_name,
                arguments,
                ..
            } => {
                if let Some(path) = namespace_path(object) {
                    let qualified_name = format!("{}.{}", path, method_name);
                    if let Some(return_type) =
                        self.std_generic_unwrap_return(&qualified_name, arguments)
                    {
                        return return_type;
                    }
                    if let Some(signature) = self.functions.get(&qualified_name).cloned() {
                        let specialized = self.specialize_std_collection_signature(
                            &qualified_name,
                            &signature,
                            arguments,
                        );
                        return specialized.return_type;
                    }

                    let exports_cloned: Option<ModuleExports> = self
                        .registry
                        .read()
                        .unwrap_or_else(|p| p.into_inner())
                        .get_module(&path)
                        .cloned();
                    if let Some(exports) = exports_cloned {
                        if let Some(func) = exports.functions.get(method_name.as_str()) {
                            let signature = FunctionSignature {
                                params: func.params.clone(),
                                return_type: func.return_type.clone(),
                                self_kind: None,
                                is_async: func.is_async,
                            };
                            let specialized = self.specialize_std_collection_signature(
                                &qualified_name,
                                &signature,
                                arguments,
                            );
                            return specialized.return_type;
                        }
                    }
                }

                let obj_type = self.infer_expression_type(object);
                if let Type::TypeParameter { name } = &obj_type {
                    if let Some((signature, _trait_name)) =
                        self.trait_method_signature_for_type_param(name, method_name)
                    {
                        return signature.return_type;
                    }
                }
                // For dyn Trait: look up the method return type from trait definition
                if let Type::DynTrait { trait_name, .. } = &obj_type {
                    if let Some(trait_methods) = self.traits.get(trait_name.as_str()) {
                        if let Some(sig) = trait_methods.get(method_name.as_str()) {
                            return sig.signature.return_type.clone();
                        }
                    }
                }
                let type_name_str = self.nominal_lookup_name(&obj_type);

                if let Some(type_name_str) = type_name_str {
                    if let Some(type_methods) = self.methods.get(&type_name_str) {
                        if let Some(signature) = type_methods.get(method_name) {
                            return signature.return_type.clone();
                        }
                    }
                    // R-211: instantiated generic struct method return type.
                    if let Some(signature) =
                        self.instantiated_method_signature(&type_name_str, method_name)
                    {
                        return signature.return_type;
                    }
                }

                Type::Unknown
            }
            ExpressionKind::CharLiteral(_) => Type::Char,
            ExpressionKind::FString(_) => Type::String,
            ExpressionKind::Lambda {
                is_async,
                params,
                body,
            } => {
                let expected_fn = match self.current_expected_type.as_ref() {
                    Some(Type::Fn {
                        params,
                        return_type,
                    }) => Some((params.clone(), return_type.as_ref().clone())),
                    _ => None,
                };
                let expected_output = expected_fn.as_ref().and_then(|(_, return_type)| {
                    if *is_async {
                        match return_type {
                            Type::Task { output } => Some(output.as_ref().clone()),
                            _ => None,
                        }
                    } else {
                        Some(return_type.clone())
                    }
                });

                self.push_scope();

                let param_types: Vec<Type> = params
                    .iter()
                    .enumerate()
                    .map(|(index, param)| {
                        let annotated = self.type_annotation_to_type_checked(&param.ty);
                        let ty = if matches!(annotated, Type::Unknown) {
                            expected_fn
                                .as_ref()
                                .and_then(|(expected_params, _)| expected_params.get(index))
                                .cloned()
                                .unwrap_or(annotated)
                        } else {
                            annotated
                        };
                        self.declare_symbol(param.name.clone(), param.span, ty.clone());
                        ty
                    })
                    .collect();

                let saved_expected = self.current_expected_type.clone();
                self.current_expected_type = expected_output.clone();
                let body_type = self.infer_expression_type(body);
                self.current_expected_type = saved_expected;
                self.pop_scope();

                let output_type = if matches!(body_type, Type::Unknown) {
                    expected_output.unwrap_or(body_type)
                } else if *is_async {
                    if let Some(expected) = expected_output.as_ref() {
                        if !self.types_match(&body_type, expected) {
                            self.error_with_hint(
                                format!(
                                    "Async closure body has type {}, but {} was expected",
                                    type_name(&body_type),
                                    type_name(expected)
                                ),
                                expr.span,
                                "Return a value matching the callback's Task output type.",
                            );
                        }
                    }
                    body_type
                } else {
                    body_type
                };

                if *is_async {
                    Type::Fn {
                        params: param_types,
                        return_type: Box::new(Type::Task {
                            output: Box::new(output_type),
                        }),
                    }
                } else {
                    Type::Fn {
                        params: param_types,
                        return_type: Box::new(output_type),
                    }
                }
            }
            ExpressionKind::Try(inner) => self.infer_expression_type(inner),
            ExpressionKind::Await(inner) => match self.infer_expression_type(inner) {
                Type::Task { output } => *output,
                Type::Unknown => Type::Unknown,
                other => {
                    self.error(
                        format!("`await` expects Task<T>, found {}", type_name(&other)),
                        expr.span,
                    );
                    Type::Unknown
                }
            },
            ExpressionKind::Cast { target_type, .. } => {
                self.type_annotation_to_type(&Some(target_type.clone()))
            }
            ExpressionKind::Range { .. } => Type::Range,
            ExpressionKind::Block(block) => {
                if self.block_guaranteed_return(block) {
                    return Type::Unknown;
                }

                // Block-local bindings must be available while inferring the
                // final expression. Without this, a closure body such as
                // `let inner = |...| ...; inner(value)` was inferred as
                // `unknown` even though the semantic analyzer had already
                // validated the nested closure.
                self.push_scope();
                let mut result = Type::Unit;
                for stmt in &block.statements {
                    match &stmt.kind {
                        crate::ast::StatementKind::Let(let_stmt) => {
                            let declared = let_stmt.ty.as_ref().map(|annotation| {
                                self.type_annotation_to_type_checked(&Some(annotation.clone()))
                            });
                            let inferred = let_stmt
                                .value
                                .as_ref()
                                .map(|value| self.infer_expression_type(value));
                            let binding_type = declared.or(inferred).unwrap_or(Type::Unknown);
                            self.register_typed_pattern_bindings(&let_stmt.pattern, &binding_type);
                        }
                        crate::ast::StatementKind::Expression(expr) => {
                            result = self.infer_expression_type(expr);
                        }
                        crate::ast::StatementKind::Return(ret) => {
                            result = ret
                                .value
                                .as_ref()
                                .map(|e| self.infer_expression_type(e))
                                .unwrap_or(Type::Unit);
                        }
                        _ => {}
                    }
                }
                self.pop_scope();
                result
            }
            ExpressionKind::DifferentiableBlock(block) => {
                let result = self.infer_block_type(block);
                match result {
                    Type::Tensor { .. } | Type::Unknown => result,
                    _ => Type::Unknown,
                }
            }
            ExpressionKind::AsyncBlock(block) => {
                if let Some(Type::Task { output }) =
                    self.symbol_resolutions.get(&expr.span).map(|info| &info.ty)
                {
                    return Type::Task {
                        output: output.clone(),
                    };
                }

                let output = match self.current_expected_type.clone() {
                    Some(Type::Task { output }) if !matches!(*output, Type::Unknown) => *output,
                    _ => self.infer_block_type(block),
                };
                Type::Task {
                    output: Box::new(output),
                }
            }
        }
    }

    pub(crate) fn record_expression_type(&mut self, expr: &Expression) {
        let ty = self.infer_expression_type(expr);
        match self.symbol_resolutions.entry(expr.span) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().ty = ty;
            }
            Entry::Vacant(entry) => {
                entry.insert(SymbolInfo {
                    is_local: false,
                    def_span: None,
                    ty,
                });
            }
        }
    }

    pub(crate) fn collect_lambda_capture_names(
        &self,
        params: &[crate::ast::LambdaParam],
        body: &Expression,
    ) -> HashSet<String> {
        let mut locals: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
        let mut captures = HashSet::new();
        self.collect_capture_names_expr(body, &mut locals, &mut captures);
        captures
    }

    fn collect_capture_names_expr(
        &self,
        expr: &Expression,
        locals: &mut HashSet<String>,
        captures: &mut HashSet<String>,
    ) {
        match &expr.kind {
            ExpressionKind::Identifier(name) => {
                if !locals.contains(name) && self.lookup_symbol(name).is_some() {
                    captures.insert(name.clone());
                }
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.collect_capture_names_expr(left, locals, captures);
                self.collect_capture_names_expr(right, locals, captures);
            }
            ExpressionKind::Unary { operand, .. }
            | ExpressionKind::Try(operand)
            | ExpressionKind::Await(operand) => {
                self.collect_capture_names_expr(operand, locals, captures);
            }
            ExpressionKind::Call { callee, arguments } => {
                self.collect_capture_names_expr(callee, locals, captures);
                for arg in arguments {
                    self.collect_capture_names_expr(arg, locals, captures);
                }
            }
            ExpressionKind::MethodCall {
                object, arguments, ..
            } => {
                self.collect_capture_names_expr(object, locals, captures);
                for arg in arguments {
                    self.collect_capture_names_expr(arg, locals, captures);
                }
            }
            ExpressionKind::Lambda { params, body, .. } => {
                let mut nested_locals = locals.clone();
                for param in params {
                    nested_locals.insert(param.name.clone());
                }
                self.collect_capture_names_expr(body, &mut nested_locals, captures);
            }
            ExpressionKind::Block(block) => {
                let mut block_locals = locals.clone();
                for stmt in &block.statements {
                    self.collect_capture_names_stmt(stmt, &mut block_locals, captures);
                }
            }
            ExpressionKind::DifferentiableBlock(block) | ExpressionKind::AsyncBlock(block) => {
                let mut block_locals = locals.clone();
                for stmt in &block.statements {
                    self.collect_capture_names_stmt(stmt, &mut block_locals, captures);
                }
            }
            ExpressionKind::If {
                condition,
                then_block,
                elif_blocks,
                else_block,
            } => {
                self.collect_capture_names_expr(condition, locals, captures);
                self.collect_capture_names_block(then_block, locals, captures);
                for (elif_condition, elif_block) in elif_blocks {
                    self.collect_capture_names_expr(elif_condition, locals, captures);
                    self.collect_capture_names_block(elif_block, locals, captures);
                }
                if let Some(block) = else_block {
                    self.collect_capture_names_block(block, locals, captures);
                }
            }
            ExpressionKind::Unless {
                condition,
                then_block,
                else_block,
            } => {
                self.collect_capture_names_expr(condition, locals, captures);
                self.collect_capture_names_block(then_block, locals, captures);
                if let Some(block) = else_block {
                    self.collect_capture_names_block(block, locals, captures);
                }
            }
            ExpressionKind::Grouping(inner) => {
                self.collect_capture_names_expr(inner, locals, captures);
            }
            ExpressionKind::FieldAccess { object, .. } => {
                self.collect_capture_names_expr(object, locals, captures);
            }
            ExpressionKind::TupleAccess { tuple, .. } => {
                self.collect_capture_names_expr(tuple, locals, captures);
            }
            ExpressionKind::IndexAccess { array, index } => {
                self.collect_capture_names_expr(array, locals, captures);
                self.collect_capture_names_expr(index, locals, captures);
            }
            ExpressionKind::ArrayLiteral { elements }
            | ExpressionKind::TupleLiteral { elements } => {
                for element in elements {
                    self.collect_capture_names_expr(element, locals, captures);
                }
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                for (_, value) in fields {
                    self.collect_capture_names_expr(value, locals, captures);
                }
            }
            ExpressionKind::EnumVariant {
                data, struct_data, ..
            } => {
                if let Some(values) = data {
                    for value in values {
                        self.collect_capture_names_expr(value, locals, captures);
                    }
                }
                if let Some(fields) = struct_data {
                    for (_, value) in fields {
                        self.collect_capture_names_expr(value, locals, captures);
                    }
                }
            }
            ExpressionKind::Match { scrutinee, arms } => {
                self.collect_capture_names_expr(scrutinee, locals, captures);
                for arm in arms {
                    let mut arm_locals = locals.clone();
                    Self::collect_pattern_names_for_closure(&arm.pattern, &mut arm_locals);
                    if let Some(guard) = &arm.guard {
                        self.collect_capture_names_expr(guard, &mut arm_locals, captures);
                    }
                    self.collect_capture_names_expr(&arm.body, &mut arm_locals, captures);
                }
            }
            ExpressionKind::Cast { expr, .. } => {
                self.collect_capture_names_expr(expr, locals, captures);
            }
            ExpressionKind::FString(parts) => {
                for part in parts {
                    if let FStringPart::Interpolated(expr) = part {
                        self.collect_capture_names_expr(expr, locals, captures);
                    }
                }
            }
            ExpressionKind::Range { start, end, .. } => {
                self.collect_capture_names_expr(start, locals, captures);
                self.collect_capture_names_expr(end, locals, captures);
            }
            ExpressionKind::NumberLiteral(_)
            | ExpressionKind::StringLiteral(_)
            | ExpressionKind::BoolLiteral(_)
            | ExpressionKind::CharLiteral(_) => {}
        }
    }

    fn collect_capture_names_block(
        &self,
        block: &Block,
        locals: &HashSet<String>,
        captures: &mut HashSet<String>,
    ) {
        let mut block_locals = locals.clone();
        for stmt in &block.statements {
            self.collect_capture_names_stmt(stmt, &mut block_locals, captures);
        }
    }

    fn collect_capture_names_stmt(
        &self,
        stmt: &Statement,
        locals: &mut HashSet<String>,
        captures: &mut HashSet<String>,
    ) {
        match &stmt.kind {
            StatementKind::Let(let_stmt) => {
                if let Some(value) = &let_stmt.value {
                    self.collect_capture_names_expr(value, locals, captures);
                }
                Self::collect_pattern_names_for_closure(&let_stmt.pattern, locals);
            }
            StatementKind::Assignment(assign) => {
                self.collect_capture_names_lvalue(&assign.target, locals, captures);
                self.collect_capture_names_expr(&assign.value, locals, captures);
            }
            StatementKind::Return(ret) => {
                if let Some(value) = &ret.value {
                    self.collect_capture_names_expr(value, locals, captures);
                }
            }
            StatementKind::Expression(expr) => {
                self.collect_capture_names_expr(expr, locals, captures);
            }
            StatementKind::While(loop_stmt) => {
                self.collect_capture_names_expr(&loop_stmt.condition, locals, captures);
                self.collect_capture_names_block(&loop_stmt.body, locals, captures);
            }
            StatementKind::DoWhile(loop_stmt) => {
                self.collect_capture_names_block(&loop_stmt.body, locals, captures);
                self.collect_capture_names_expr(&loop_stmt.condition, locals, captures);
            }
            StatementKind::For(for_loop) => {
                self.collect_capture_names_expr(&for_loop.iterable, locals, captures);
                let mut loop_locals = locals.clone();
                loop_locals.insert(for_loop.iterator.clone());
                self.collect_capture_names_block(&for_loop.body, &loop_locals, captures);
            }
            StatementKind::IfLet(stmt) => {
                self.collect_capture_names_expr(&stmt.value, locals, captures);
                let mut then_locals = locals.clone();
                Self::collect_pattern_names_for_closure(&stmt.pattern, &mut then_locals);
                self.collect_capture_names_block(&stmt.then_block, &then_locals, captures);
                if let Some(block) = &stmt.else_block {
                    self.collect_capture_names_block(block, locals, captures);
                }
            }
            StatementKind::WhileLet(stmt) => {
                self.collect_capture_names_expr(&stmt.value, locals, captures);
                let mut body_locals = locals.clone();
                Self::collect_pattern_names_for_closure(&stmt.pattern, &mut body_locals);
                self.collect_capture_names_block(&stmt.body, &body_locals, captures);
            }
            StatementKind::Loop(loop_stmt) => {
                self.collect_capture_names_block(&loop_stmt.body, locals, captures);
            }
            StatementKind::Switch(switch_stmt) => {
                self.collect_capture_names_expr(&switch_stmt.value, locals, captures);
                for case in &switch_stmt.cases {
                    self.collect_capture_names_expr(&case.pattern, locals, captures);
                    self.collect_capture_names_block(&case.body, locals, captures);
                }
                if let Some(block) = &switch_stmt.default {
                    self.collect_capture_names_block(block, locals, captures);
                }
            }
            StatementKind::Break | StatementKind::Continue => {}
        }
    }

    fn collect_capture_names_lvalue(
        &self,
        target: &crate::ast::LValue,
        locals: &mut HashSet<String>,
        captures: &mut HashSet<String>,
    ) {
        match target {
            crate::ast::LValue::Identifier(name) => {
                if !locals.contains(name) && self.lookup_symbol(name).is_some() {
                    captures.insert(name.clone());
                }
            }
            crate::ast::LValue::IndexAccess { array, index } => {
                self.collect_capture_names_expr(array, locals, captures);
                self.collect_capture_names_expr(index, locals, captures);
            }
            crate::ast::LValue::FieldAccess { object, .. } => {
                self.collect_capture_names_expr(object, locals, captures);
            }
        }
    }
}
