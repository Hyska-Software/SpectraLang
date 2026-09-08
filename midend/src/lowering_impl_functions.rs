use super::*;

impl ASTLowering {
    /// Infere o tipo IR de uma expressão AST (análise simplificada)
    pub(crate) fn infer_expr_ir_type(&mut self, expr: &Expression) -> IRType {
        match &expr.kind {
            ExpressionKind::NumberLiteral(s) => {
                if spectra_compiler::numeric::number_literal_is_float(s) {
                    IRType::Float
                } else {
                    IRType::Int
                }
            }
            ExpressionKind::StringLiteral(_) => IRType::String,
            ExpressionKind::BoolLiteral(_) => IRType::Bool,
            ExpressionKind::Identifier(name) => {
                if let Some((_, ty)) = self.static_globals.get(name) {
                    ty.clone()
                } else if let Some(constant) = self.const_values.get(name) {
                    match constant {
                        LoweredConstValue::Int(_) => IRType::Int,
                        LoweredConstValue::Float(_) => IRType::Float,
                        LoweredConstValue::Bool(_) => IRType::Bool,
                        LoweredConstValue::String(_) => IRType::String,
                        LoweredConstValue::Char(_) => IRType::Char,
                    }
                } else if let Some((_, struct_name)) = self.struct_var_map.get(name) {
                    let fields = self
                        .struct_definitions
                        .get(&struct_name)
                        .cloned()
                        .or_else(|| {
                            if let Some(IRType::Struct { fields, .. }) =
                                self.variable_types.get(name)
                            {
                                Some(fields)
                            } else {
                                None
                            }
                        })
                        .unwrap_or_default();
                    IRType::Struct {
                        name: struct_name,
                        fields,
                    }
                } else if let Some(info) = self.array_map.get(name) {
                    IRType::Array {
                        element_type: Box::new(info.element_type.clone()),
                        size: info.size,
                    }
                } else if let Some(ty) = self.variable_types.get(name) {
                    ty
                } else {
                    IRType::Unknown
                }
            }
            ExpressionKind::ArrayLiteral { elements } => {
                let elem_type = self.infer_array_element_type(elements);
                IRType::Array {
                    element_type: Box::new(elem_type),
                    size: elements.len(),
                }
            }
            ExpressionKind::TupleLiteral { elements } => {
                let element_types: Vec<IRType> = elements
                    .iter()
                    .map(|e| self.infer_expr_ir_type(e))
                    .collect();
                IRType::Tuple {
                    elements: element_types,
                }
            }
            ExpressionKind::StructLiteral {
                name, type_args, ..
            } => self
                .resolve_struct_type(name, type_args)
                .unwrap_or(IRType::Unknown),
            ExpressionKind::FieldAccess { object, field } => {
                let object_type = self.infer_expr_ir_type(object);
                match self.ir_type_representation(&object_type) {
                    IRType::Struct { fields, .. } => fields
                        .iter()
                        .find(|(fname, _)| fname == field)
                        .map(|(_, ty)| ty.clone())
                        .unwrap_or(IRType::Unknown),
                    _ => IRType::Unknown,
                }
            }
            ExpressionKind::EnumVariant {
                module_path: _,
                enum_name,
                type_args,
                variant_name,
                data,
                struct_data,
            } => {
                // R-212: UFCS Trait::method(obj, args) return type from the trait.
                if self.trait_method_order.contains_key(enum_name.as_str()) {
                    if let Some(receiver_expr) = data.as_ref().and_then(|args| args.first()) {
                        if let IRType::Struct { name, .. } = self.infer_expr_ir_type(receiver_expr)
                        {
                            let function_name = format!("{}_{}", name, variant_name);
                            if let Some(return_type) = self.function_return_types.get(&function_name)
                            {
                                return return_type.clone();
                            }
                        }
                    }
                    if let Some((_, return_type)) = self
                        .trait_method_signatures
                        .get(enum_name.as_str())
                        .and_then(|methods| methods.get(variant_name.as_str()))
                    {
                        return return_type.clone();
                    }
                }

                let looks_like_call = data.is_some() || struct_data.is_some();
                let is_known_type = self.struct_definitions.contains_key(enum_name.as_str())
                    || self.generic_structs.contains_key(enum_name.as_str())
                    || self.enum_definitions.contains_key(enum_name.as_str())
                    || self.generic_enums.contains_key(enum_name.as_str());

                // Cross-module function calls such as `tasks::ready(x)` are parsed as
                // EnumVariant nodes. Imported function returns are registered under the
                // exported function name, so resolve them before enum refinement.
                if looks_like_call && !is_known_type {
                    if let Some(ret) = self.function_return_types.get(variant_name) {
                        return ret.clone();
                    }
                    if self.generic_functions.contains_key(variant_name) {
                        let concrete_types =
                            self.infer_argument_types(data.as_deref().unwrap_or(&[]));
                        let request = MonomorphizationRequest {
                            generic_name: variant_name.clone(),
                            concrete_types,
                        };
                        let mangled = request.mangled_name();
                        if let Some(ret) = self.function_return_types.get(&mangled) {
                            return ret.clone();
                        }
                    }
                }

                // Handle StructName::method(args) — the parser treats `Name::Other(...)` as
                // EnumVariant even for struct static/associated-function calls.
                if let Some(fields) = self.struct_definitions.get(enum_name.as_str()) {
                    // JSON derive methods are generated by semantic analysis and do not have
                    // a lowered function body.  Keep their expression type aligned with the
                    // dedicated lowering paths below instead of falling back to the receiver
                    // struct (which made `Profile::json_error_field(...) != ""` lower as
                    // `Profile_eq`).
                    if variant_name == "json_error_field" {
                        return IRType::String;
                    }
                    if variant_name == "from_json" {
                        return IRType::Struct {
                            name: enum_name.clone(),
                            fields: fields.clone(),
                        };
                    }
                    let mangled = format!("{}_{}", enum_name, variant_name);
                    if let Some(ret) = self.function_return_types.get(&mangled) {
                        return ret.clone();
                    }
                    // Fallback: assume the static call returns an instance of the struct.
                    return IRType::Struct {
                        name: enum_name.clone(),
                        fields: fields.clone(),
                    };
                }
                if self.generic_structs.contains_key(enum_name.as_str()) {
                    if variant_name == "json_error_field" {
                        return IRType::String;
                    }
                    let mangled = format!("{}_{}", enum_name, variant_name);
                    if let Some(ret) = self.function_return_types.get(&mangled) {
                        return ret.clone();
                    }
                    return IRType::Struct {
                        name: enum_name.clone(),
                        fields: vec![],
                    };
                }

                let needs_refinement = type_args.is_empty()
                    || type_args
                        .iter()
                        .any(|ann| self.type_annotation_needs_refinement(ann));

                let inferred_args = if needs_refinement {
                    if let Some(data_exprs) = data {
                        self.infer_enum_type_args_from_data(enum_name, variant_name, data_exprs)
                            .or_else(|| self.default_type_args_for_enum(enum_name))
                    } else if let Some(named_fields) = struct_data {
                        self.infer_enum_type_args_from_named_fields(
                            enum_name,
                            variant_name,
                            named_fields,
                        )
                        .or_else(|| self.default_type_args_for_enum(enum_name))
                    } else {
                        self.default_type_args_for_enum(enum_name)
                    }
                } else {
                    None
                };

                let mut final_args: Vec<TypeAnnotation> = if let Some(args) = inferred_args {
                    args
                } else {
                    type_args.clone()
                };

                Self::fill_builtin_enum_defaults(enum_name, &mut final_args);

                let resolved = self
                    .resolve_enum_type(enum_name, final_args.as_slice())
                    .unwrap_or(IRType::Unknown);

                match (resolved, data.as_ref()) {
                    (IRType::Enum { name, variants }, Some(data_exprs)) => {
                        let refined_variants = variants
                            .into_iter()
                            .map(|(name, variant_data)| {
                                if name == *variant_name {
                                    if let Some(data_types) = variant_data {
                                        let refined = data_types
                                            .into_iter()
                                            .enumerate()
                                            .map(|(idx, ty)| {
                                                if matches!(ty, IRType::Void) {
                                                    data_exprs
                                                        .get(idx)
                                                        .map(|expr| self.infer_expr_ir_type(expr))
                                                        .unwrap_or(IRType::Unknown)
                                                } else {
                                                    ty
                                                }
                                            })
                                            .collect();
                                        return (name, Some(refined));
                                    }
                                }
                                (name, variant_data)
                            })
                            .collect();
                        IRType::Enum {
                            name,
                            variants: refined_variants,
                        }
                    }
                    (resolved, _) => resolved,
                }
            }
            ExpressionKind::IndexAccess { array, .. } => match self.infer_expr_ir_type(array) {
                IRType::Array { element_type, .. } => *element_type,
                IRType::String => IRType::Char,
                _ => IRType::Unknown,
            },
            ExpressionKind::TupleAccess { tuple, index } => match self.infer_expr_ir_type(tuple) {
                IRType::Tuple { elements } if *index < elements.len() => elements[*index].clone(),
                _ => IRType::Unknown,
            },
            ExpressionKind::Call { callee, arguments } => {
                if let ExpressionKind::Identifier(name) = &callee.kind {
                    if name == "block_on" {
                        return arguments
                            .first()
                            .map(|arg| match self.infer_expr_ir_type(arg) {
                                IRType::Task { output } => *output,
                                _ => IRType::Unknown,
                            })
                            .unwrap_or(IRType::Unknown);
                    }
                }

                if let Some(descriptor) = self.host_function_descriptor_for_call(callee, arguments) {
                    return descriptor.return_type.clone();
                }

                if let ExpressionKind::Identifier(name) = &callee.kind {
                    if let Some(ret) = self.function_return_types.get(name) {
                        return ret.clone();
                    }
                    if let Some(IRType::Function { return_type, .. }) =
                        self.variable_types.get(name)
                    {
                        return (*return_type).clone();
                    }

                    if self.generic_functions.contains_key(name) {
                        let concrete_types = self.infer_argument_types(arguments);
                        let request = MonomorphizationRequest {
                            generic_name: name.clone(),
                            concrete_types: concrete_types.clone(),
                        };
                        let mangled = request.mangled_name();

                        if let Some(ret) = self.function_return_types.get(&mangled) {
                            return ret.clone();
                        }

                        if let Some(generic_func) = self.generic_functions.get(name) {
                            let mut type_map: HashMap<String, IRType> = HashMap::new();
                            for (param, concrete) in generic_func
                                .type_params
                                .iter()
                                .zip(concrete_types)
                            {
                                type_map.insert(param.name.clone(), concrete);
                            }

                            if let Some(ret_ann) = &generic_func.return_type {
                                return self.lower_type_annotation_with_map(ret_ann, &type_map);
                            } else {
                                return IRType::Void;
                            }
                        }
                    }
                }

                IRType::Unknown
            }
            ExpressionKind::MethodCall {
                object,
                method_name,
                arguments,
                type_name,
            } => {
                if let Some(descriptor) =
                    self.std_method_host_function_descriptor_for_call(
                        object,
                        method_name,
                        arguments,
                    )
                {
                    return descriptor.return_type.clone();
                }

                if let IRType::DynTrait { trait_name, .. } = self.infer_expr_ir_type(object) {
                    if let Some((_, return_type)) = self
                        .trait_method_signatures
                        .get(&trait_name)
                        .and_then(|methods| methods.get(method_name))
                    {
                        return return_type.clone();
                    }
                }

                let obj_type_name = if let Some(name) = type_name {
                    name.clone()
                } else {
                    match self.infer_expr_ir_type(object) {
                        IRType::Struct { name, .. } => name,
                        IRType::Enum { name, .. } => name,
                        _ => return IRType::Unknown,
                    }
                };

                let function_name = format!("{}_{}", obj_type_name, method_name);

                if let Some(ret) = self.function_return_types.get(&function_name) {
                    ret.clone()
                } else if let Some((base_name, concrete_types)) =
                    self.instantiated_structs.get(&obj_type_name).cloned()
                {
                    // R-211: return type of a generic impl method specialization is
                    // derived from the template with the type parameters substituted.
                    let key = format!("{}_{}", base_name, method_name);
                    if let Some((method, type_params)) = self.generic_impl_methods.get(&key) {
                        let mut type_map: HashMap<String, IRType> = HashMap::new();
                        for (i, type_param) in type_params.iter().enumerate() {
                            if let Some(concrete_type) = concrete_types.get(i) {
                                type_map.insert(type_param.name.clone(), concrete_type.clone());
                            }
                        }
                        method
                            .return_type
                            .as_ref()
                            .map(|ann| {
                                let mut ann = ann.clone();
                                self.substitute_type_in_annotation(&mut ann, &type_map);
                                self.lower_type_annotation(&ann)
                            })
                            .unwrap_or(IRType::Unknown)
                    } else {
                        IRType::Unknown
                    }
                } else {
                    IRType::Unknown
                }
            }
            ExpressionKind::If {
                then_block,
                elif_blocks,
                else_block,
                ..
            } => {
                let mut branch_types = Vec::new();

                if let Some(ty) = self.infer_block_result_type(then_block) {
                    branch_types.push(ty);
                }

                for (_, block) in elif_blocks {
                    if let Some(ty) = self.infer_block_result_type(block) {
                        branch_types.push(ty);
                    }
                }

                if let Some(block) = else_block {
                    if let Some(ty) = self.infer_block_result_type(block) {
                        branch_types.push(ty);
                    }
                } else {
                    branch_types.push(IRType::Void);
                }

                self.unify_types(branch_types)
            }
            ExpressionKind::Unless {
                then_block,
                else_block,
                ..
            } => {
                let mut branch_types = Vec::new();

                if let Some(ty) = self.infer_block_result_type(then_block) {
                    branch_types.push(ty);
                }

                if let Some(block) = else_block {
                    if let Some(ty) = self.infer_block_result_type(block) {
                        branch_types.push(ty);
                    }
                } else {
                    branch_types.push(IRType::Void);
                }

                self.unify_types(branch_types)
            }
            ExpressionKind::Match { scrutinee, arms } => {
                // Match-arm identifiers are scoped pattern bindings.  Inferring
                // only the arm body without installing those bindings turns a
                // valid `Some(value) -> value` into Unknown before the actual
                // match lowering has a chance to bind it, poisoning the local
                // result type and eventually the IR verifier.
                let scrutinee_type = self.infer_expr_ir_type(scrutinee);
                let scrutinee_enum_name = match &scrutinee_type {
                    IRType::Enum { name, .. } => Some(name.clone()),
                    IRType::Generic { .. } => self.ir_nominal_name(&scrutinee_type).map(str::to_string),
                    _ => None,
                };
                let arm_types: Vec<IRType> = arms
                    .iter()
                    .map(|arm| {
                        self.infer_match_arm_type(
                            &arm.pattern,
                            &arm.body,
                            scrutinee_enum_name.as_deref(),
                            &scrutinee_type,
                        )
                    })
                    .collect();
                self.unify_types(arm_types)
            }
            ExpressionKind::Grouping(inner) => self.infer_expr_ir_type(inner),
            ExpressionKind::Unary { operator, operand } => match operator {
                UnaryOperator::Negate => self.infer_expr_ir_type(operand),
                UnaryOperator::Not => IRType::Bool,
            },
            ExpressionKind::Binary {
                left,
                operator,
                right,
            } => {
                let left_type = self.infer_expr_ir_type(left);
                let right_type = self.infer_expr_ir_type(right);

                if matches!(left_type, IRType::Unknown) || matches!(right_type, IRType::Unknown) {
                    return IRType::Unknown;
                }

                match operator {
                    BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
                    | BinaryOperator::Modulo => {
                        // Struct operand: operator is overloaded, return the struct type itself.
                        if let IRType::Struct { .. } = &left_type {
                            return left_type.clone();
                        }
                        let (left_is_float, left_is_string) = match left_type {
                            IRType::Float => (true, false),
                            IRType::String => (false, true),
                            _ => (false, false),
                        };
                        let (right_is_float, right_is_string) = match right_type {
                            IRType::Float => (true, false),
                            IRType::String => (false, true),
                            _ => (false, false),
                        };

                        if left_is_float || right_is_float {
                            IRType::Float
                        } else if left_is_string || right_is_string {
                            IRType::String
                        } else {
                            IRType::Int
                        }
                    }
                    BinaryOperator::Equal
                    | BinaryOperator::NotEqual
                    | BinaryOperator::Less
                    | BinaryOperator::LessEqual
                    | BinaryOperator::Greater
                    | BinaryOperator::GreaterEqual
                    | BinaryOperator::And
                    | BinaryOperator::Or => IRType::Bool,
                }
            }
            ExpressionKind::CharLiteral(_) => IRType::Char,
            ExpressionKind::FString(_) => IRType::String,
            ExpressionKind::Lambda { is_async, params, body } => {
                // Async closures retain the normal function parameter shape,
                // but their public result is a lazy Task<T>.
                let param_types: Vec<IRType> = params
                    .iter()
                    .map(|p| p.ty.as_ref().map(|t| self.lower_type_annotation(t)).unwrap_or(IRType::Unknown))
                    .collect();
                self.variable_types.push_scope();
                for (param, param_type) in params.iter().zip(param_types.iter()) {
                    self.variable_types.insert(param.name.clone(), param_type.clone());
                }
                let ret = self.infer_expr_ir_type(body);
                self.variable_types.pop_scope();
                let return_type = if *is_async {
                    IRType::Task { output: Box::new(ret) }
                } else {
                    ret
                };
                IRType::Function { params: param_types, return_type: Box::new(return_type) }
            }
            ExpressionKind::Try(inner) => {
                // `?` unwraps the Ok payload; infer from the inner type's first data field.
                let inner_type = self.infer_expr_ir_type(inner);
                match inner_type {
                    IRType::Enum {
                        name: _,
                        ref variants,
                    } => {
                        // Expect Ok at tag 0 with a single data field
                        if let Some((_, Some(payload_types))) = variants.first() {
                            if let Some(first) = payload_types.first() {
                                return first.clone();
                            }
                        }
                        IRType::Unknown
                    }
                    _ => IRType::Unknown,
                }
            }
            ExpressionKind::Range { .. } => IRType::Range,
            ExpressionKind::Block(block) => self
                .infer_block_result_type(block)
                .unwrap_or(IRType::Void),
            ExpressionKind::DifferentiableBlock(block) => self
                .infer_block_result_type(block)
                .unwrap_or(IRType::Void),
            ExpressionKind::Await(inner) => match self.infer_expr_ir_type(inner) {
                IRType::Task { output } => *output,
                _ => IRType::Unknown,
            },
            ExpressionKind::AsyncBlock(block) => IRType::Task {
                output: Box::new(
                    self.expected_async_output_type()
                        .or_else(|| self.infer_block_result_type(block))
                        .unwrap_or(IRType::Void),
                ),
            },
            ExpressionKind::Cast { target_type, .. } => self.lower_type_annotation(target_type),
        }
    }

    pub(crate) fn infer_pattern_binding_types(
        &self,
        pattern: &spectra_compiler::ast::Pattern,
        scrutinee_enum: Option<&str>,
        scrutinee_type: Option<&IRType>,
        out: &mut HashMap<String, IRType>,
    ) {
        use spectra_compiler::ast::Pattern;

        match pattern {
            Pattern::Wildcard(_) | Pattern::Literal(_) => {}
            Pattern::Identifier(name, _) => {
                if let Some(ty) = scrutinee_type {
                    out.insert(name.clone(), ty.clone());
                }
            }
            Pattern::Tuple(elements) => {
                if let Some(IRType::Tuple {
                    elements: tuple_types,
                }) = scrutinee_type
                {
                    for (pattern, ty) in elements.iter().zip(tuple_types.iter()) {
                        self.infer_pattern_binding_types(pattern, None, Some(ty), out);
                    }
                }
            }
            Pattern::Struct { fields, .. } => {
                if let Some(IRType::Struct {
                    fields: struct_fields,
                    ..
                }) = scrutinee_type.map(Self::ir_type_representation_static)
                {
                    let field_map: HashMap<String, IRType> =
                        struct_fields.iter().cloned().collect();
                    for (field_name, pattern) in fields {
                        if let Some(field_ty) = field_map.get(field_name) {
                            self.infer_pattern_binding_types(pattern, None, Some(field_ty), out);
                        }
                    }
                }
            }
            Pattern::EnumVariant {
                enum_name,
                type_args,
                variant_name,
                data,
                struct_data,
                ..
            } => {
                let ordered_patterns: Vec<&spectra_compiler::ast::Pattern> = if let Some(patterns) =
                    data
                {
                    patterns.iter().collect()
                } else if let Some(named_patterns) = struct_data {
                    self.reorder_named_variant_patterns(
                        scrutinee_enum.unwrap_or(enum_name),
                        variant_name,
                        named_patterns,
                    )
                    .unwrap_or_else(|| named_patterns.iter().map(|(_, pattern)| pattern).collect())
                } else {
                    Vec::new()
                };

                if ordered_patterns.is_empty() {
                    return;
                }

                let mut variants = scrutinee_enum
                    .and_then(|name| self.enum_definitions.get(name).cloned())
                    .or_else(|| {
                        if let Some(IRType::Enum { name, .. }) = scrutinee_type
                            .map(Self::ir_type_representation_static)
                        {
                            self.enum_definitions.get(name).cloned()
                        } else {
                            None
                        }
                    })
                    .or_else(|| self.enum_variants_from_ir_type(scrutinee_type))
                    .or_else(|| self.enum_definitions.get(enum_name).cloned());

                if variants.is_none() && !type_args.is_empty() {
                    if let Some(IRType::Enum {
                        variants: specialized,
                        ..
                    }) = self
                        .resolve_enum_type(enum_name, type_args.as_slice())
                        .as_ref()
                        .map(Self::ir_type_representation_static)
                        .cloned()
                    {
                        variants = Some(
                            specialized
                                .into_iter()
                                .enumerate()
                                .map(|(tag, (name, data))| (name, tag, data))
                                .collect(),
                        );
                    }
                }

                if let Some(variants) = variants {
                    if let Some((_, _, Some(types))) =
                        variants.iter().find(|(name, _, _)| name == variant_name)
                    {
                        for (idx, sub_pattern) in ordered_patterns.iter().enumerate() {
                            if let Some(sub_type) = types.get(idx) {
                                let next_enum = match Self::ir_type_representation_static(sub_type) {
                                    IRType::Enum { name, .. } => Some(name.as_str()),
                                    _ => None,
                                };
                                self.infer_pattern_binding_types(
                                    sub_pattern,
                                    next_enum,
                                    Some(sub_type),
                                    out,
                                );
                            }
                        }
                    }
                }
            }
            Pattern::Or(patterns) => {
                if let Some(first) = patterns.first() {
                    self.infer_pattern_binding_types(first, scrutinee_enum, scrutinee_type, out);
                }
            }
        }
    }

    pub(crate) fn infer_match_arm_type(
        &mut self,
        pattern: &spectra_compiler::ast::Pattern,
        body: &Expression,
        scrutinee_enum_name: Option<&str>,
        scrutinee_type: &IRType,
    ) -> IRType {
        let mut bindings = HashMap::new();
        self.infer_pattern_binding_types(
            pattern,
            scrutinee_enum_name,
            Some(scrutinee_type),
            &mut bindings,
        );
        self.variable_types.push_scope();
        for (name, ty) in bindings {
            self.variable_types.insert(name, ty);
        }
        let result = self.infer_expr_ir_type(body);
        self.variable_types.pop_scope();
        result
    }

}
