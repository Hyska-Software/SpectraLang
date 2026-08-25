use super::*;

impl SemanticAnalyzer {
    // Third pass: fill type information in method calls
    pub(crate) fn fill_method_call_types_in_item(&mut self, item: &mut Item) {
        match item {
            Item::Function(func) => {
                self.fill_method_call_types_in_block(&mut func.body);
            }
            Item::Impl(impl_block) => {
                for method in &mut impl_block.methods {
                    self.fill_method_call_types_in_block(&mut method.body);
                }
            }
            Item::TraitImpl(trait_impl) => {
                for method in &mut trait_impl.methods {
                    self.fill_method_call_types_in_block(&mut method.body);
                }
            }
            _ => {}
        }
    }

    fn fill_method_call_types_in_block(&mut self, block: &mut crate::ast::Block) {
        for stmt in &mut block.statements {
            self.fill_method_call_types_in_statement(stmt);
        }
    }

    fn fill_method_call_types_in_statement(&mut self, stmt: &mut Statement) {
        use crate::ast::StatementKind;

        match &mut stmt.kind {
            StatementKind::Let(let_stmt) => {
                if let Some(value) = &mut let_stmt.value {
                    self.fill_method_call_types_in_expression(value);
                }
            }
            StatementKind::Assignment(assign) => {
                self.fill_method_call_types_in_expression(&mut assign.value);
            }
            StatementKind::While(while_loop) => {
                self.fill_method_call_types_in_expression(&mut while_loop.condition);
                self.fill_method_call_types_in_block(&mut while_loop.body);
            }
            StatementKind::DoWhile(do_while) => {
                self.fill_method_call_types_in_block(&mut do_while.body);
                self.fill_method_call_types_in_expression(&mut do_while.condition);
            }
            StatementKind::For(for_loop) => {
                self.fill_method_call_types_in_expression(&mut for_loop.iterable);
                self.fill_method_call_types_in_block(&mut for_loop.body);
            }
            StatementKind::Loop(loop_stmt) => {
                self.fill_method_call_types_in_block(&mut loop_stmt.body);
            }
            StatementKind::Expression(expr) => {
                self.fill_method_call_types_in_expression(expr);
            }
            StatementKind::Return(ret_stmt) => {
                if let Some(expr) = &mut ret_stmt.value {
                    self.fill_method_call_types_in_expression(expr);
                }
            }
            _ => {}
        }
    }

    fn fill_method_call_types_in_expression(&mut self, expr: &mut Expression) {
        use crate::ast::ExpressionKind;

        match &mut expr.kind {
            ExpressionKind::MethodCall {
                object,
                method_name: _,
                arguments,
                type_name,
            } => {
                // Primeiro, processar recursivamente o objeto e argumentos
                self.fill_method_call_types_in_expression(object);
                for arg in arguments {
                    self.fill_method_call_types_in_expression(arg);
                }

                // Se type_name ainda não foi preenchido, inferir agora
                if type_name.is_none() {
                    let obj_type = self.infer_expression_type(object);
                    *type_name = match obj_type {
                        Type::Struct { name } => Some(name),
                        Type::Enum { name, .. } => Some(name),
                        _ => None,
                    };
                }
            }
            ExpressionKind::Call { callee, arguments } => {
                self.fill_method_call_types_in_expression(callee);
                for arg in arguments {
                    self.fill_method_call_types_in_expression(arg);
                }
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.fill_method_call_types_in_expression(left);
                self.fill_method_call_types_in_expression(right);
            }
            ExpressionKind::Unary { operand, .. } => {
                self.fill_method_call_types_in_expression(operand);
            }
            ExpressionKind::FieldAccess { object, .. } => {
                self.fill_method_call_types_in_expression(object);
            }
            ExpressionKind::IndexAccess { array, index } => {
                self.fill_method_call_types_in_expression(array);
                self.fill_method_call_types_in_expression(index);
            }
            ExpressionKind::TupleAccess { tuple, .. } => {
                self.fill_method_call_types_in_expression(tuple);
            }
            ExpressionKind::Match { scrutinee, arms } => {
                self.fill_method_call_types_in_expression(scrutinee);
                for arm in arms {
                    self.fill_method_call_types_in_expression(&mut arm.body);
                }
            }
            _ => {}
        }
    }

    // ============= Type Inference Pass =============

    pub(crate) fn infer_generic_types_in_item(&mut self, item: &mut Item) {
        match item {
            Item::Function(func) => {
                self.infer_generic_types_in_block(&mut func.body);
            }
            Item::Impl(impl_block) => {
                for method in &mut impl_block.methods {
                    self.infer_generic_types_in_block(&mut method.body);
                }
            }
            Item::TraitImpl(trait_impl) => {
                for method in &mut trait_impl.methods {
                    self.infer_generic_types_in_block(&mut method.body);
                }
            }
            _ => {}
        }
    }

    fn infer_generic_types_in_block(&mut self, block: &mut Block) {
        for stmt in &mut block.statements {
            self.infer_generic_types_in_statement(stmt);
        }
    }

    fn infer_generic_types_in_statement(&mut self, stmt: &mut Statement) {
        use crate::ast::StatementKind;

        match &mut stmt.kind {
            StatementKind::Let(let_stmt) => {
                if let Some(value) = &mut let_stmt.value {
                    self.infer_generic_types_in_expression(value);
                }
            }
            StatementKind::Assignment(assign) => {
                self.infer_generic_types_in_expression(&mut assign.value);
            }
            StatementKind::While(while_loop) => {
                self.infer_generic_types_in_expression(&mut while_loop.condition);
                self.infer_generic_types_in_block(&mut while_loop.body);
            }
            StatementKind::DoWhile(do_while_loop) => {
                self.infer_generic_types_in_block(&mut do_while_loop.body);
                self.infer_generic_types_in_expression(&mut do_while_loop.condition);
            }
            StatementKind::For(for_loop) => {
                self.infer_generic_types_in_expression(&mut for_loop.iterable);
                self.infer_generic_types_in_block(&mut for_loop.body);
            }
            StatementKind::Expression(expr) => {
                self.infer_generic_types_in_expression(expr);
            }
            StatementKind::Return(ret_stmt) => {
                if let Some(value) = &mut ret_stmt.value {
                    self.infer_generic_types_in_expression(value);
                }
            }
            _ => {}
        }
    }

    fn infer_generic_types_in_expression(&mut self, expr: &mut Expression) {
        match &mut expr.kind {
            ExpressionKind::StructLiteral {
                name,
                type_args,
                fields,
            } => {
                // Infer type arguments if not provided and struct is generic
                if type_args.is_empty() {
                    if let Some((type_params, field_defs)) = self.generic_structs.get(name).cloned()
                    {
                        // Attempt to infer type arguments from field values
                        let inferred_types =
                            self.infer_struct_type_args(&type_params, &field_defs, fields);
                        if !inferred_types.is_empty() {
                            // Update the expression with inferred type arguments
                            *type_args = inferred_types;
                        }
                    }
                }

                // Recurse into field values
                for (_, field_value) in fields {
                    self.infer_generic_types_in_expression(field_value);
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
                if let Some(args) = data {
                    for arg in args.iter_mut() {
                        self.infer_generic_types_in_expression(arg);
                    }

                    if type_args.is_empty() {
                        let inferred_args =
                            self.infer_enum_type_args(enum_name, variant_name, args.as_slice());
                        if !inferred_args.is_empty() {
                            *type_args = inferred_args;
                        }
                    }
                }

                if let Some(fields) = struct_data {
                    if type_args.is_empty() {
                        if let Some(inferred_args) = self
                            .infer_enum_type_args_from_named_fields(
                                enum_name,
                                variant_name,
                                fields.as_slice(),
                            )
                        {
                            *type_args = inferred_args;
                        }
                    }
                    for (_, field_value) in fields.iter_mut() {
                        self.infer_generic_types_in_expression(field_value);
                    }
                }
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.infer_generic_types_in_expression(left);
                self.infer_generic_types_in_expression(right);
            }
            ExpressionKind::Unary { operand, .. } => {
                self.infer_generic_types_in_expression(operand);
            }
            ExpressionKind::Call { arguments, .. } => {
                for arg in arguments {
                    self.infer_generic_types_in_expression(arg);
                }
            }
            ExpressionKind::If {
                condition,
                then_block,
                elif_blocks,
                else_block,
            } => {
                self.infer_generic_types_in_expression(condition);
                self.infer_generic_types_in_block(then_block);
                for (elif_cond, elif_block) in elif_blocks {
                    self.infer_generic_types_in_expression(elif_cond);
                    self.infer_generic_types_in_block(elif_block);
                }
                if let Some(else_body) = else_block {
                    self.infer_generic_types_in_block(else_body);
                }
            }
            ExpressionKind::Match { scrutinee, arms } => {
                self.infer_generic_types_in_expression(scrutinee);
                for arm in arms {
                    self.infer_generic_types_in_expression(&mut arm.body);
                }
            }
            ExpressionKind::DifferentiableBlock(block) => {
                self.infer_generic_types_in_block(block);
            }
            ExpressionKind::AsyncBlock(block) => {
                self.infer_generic_types_in_block(block);
            }
            ExpressionKind::ArrayLiteral { elements } => {
                for elem in elements {
                    self.infer_generic_types_in_expression(elem);
                }
            }
            ExpressionKind::FieldAccess { object, .. } => {
                self.infer_generic_types_in_expression(object);
            }
            ExpressionKind::IndexAccess { array, index } => {
                self.infer_generic_types_in_expression(array);
                self.infer_generic_types_in_expression(index);
            }
            _ => {}
        }
    }

    /// Infer type arguments for a generic struct from field values
    pub(crate) fn infer_struct_type_args(
        &mut self,
        type_params: &[crate::ast::TypeParameter],
        field_defs: &[(String, crate::ast::TypeAnnotation)],
        field_values: &[(String, Expression)],
    ) -> Vec<crate::ast::TypeAnnotation> {
        // Create a map to store inferred types for each type parameter
        let mut type_map: HashMap<String, Type> = HashMap::new();

        // For each field value, infer its type and match against field definition
        for (field_name, field_expr) in field_values {
            // Find the field definition
            if let Some((_, field_type_ann)) =
                field_defs.iter().find(|(name, _)| name == field_name)
            {
                // Infer the type of the field expression
                let value_type = self.infer_expression_type(field_expr);

                // Try to unify the field type annotation with the value type
                self.unify_type_annotation(field_type_ann, &value_type, &mut type_map);
            }
        }

        // Convert inferred types to TypeAnnotation in the order of type_params
        let mut result = Vec::new();
        for param in type_params {
            if let Some(inferred_type) = type_map.get(&param.name) {
                let type_ann = self.type_to_annotation(inferred_type);
                result.push(type_ann);
            } else {
                // Could not infer this type parameter, return empty to indicate failure
                return Vec::new();
            }
        }

        result
    }

    /// Infer type arguments for a generic enum based on a variant constructor call
    pub(crate) fn infer_enum_type_args(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        arg_exprs: &[Expression],
    ) -> Vec<crate::ast::TypeAnnotation> {
        let (type_params, _) = match self.generic_enums.get(enum_name) {
            Some(info) => info.clone(),
            None => return Vec::new(),
        };

        let variant_info = match self
            .enum_infos
            .get(enum_name)
            .and_then(|info| info.variants.get(variant_name))
            .cloned()
        {
            Some(info) => info,
            None => return Vec::new(),
        };

        let field_type_annotations = match variant_info.data {
            Some(data) if data.len() == arg_exprs.len() && !data.is_empty() => data,
            _ => return Vec::new(),
        };

        let mut type_map: HashMap<String, Type> = HashMap::new();
        for (field_ann, arg_expr) in field_type_annotations.iter().zip(arg_exprs) {
            let value_type = self.infer_expression_type(arg_expr);
            self.unify_type_annotation(field_ann, &value_type, &mut type_map);
        }

        let mut result = Vec::new();
        for param in type_params {
            match type_map.get(&param.name) {
                Some(mapped) if !matches!(mapped, Type::Unknown) => {
                    result.push(self.type_to_annotation(mapped));
                }
                // The compatibility contract for the built-in Option/Result
                // constructors keeps the unmentioned side of an application
                // concrete as `int` (`Result::Ok(1)` and `Result::Err("e")`).
                // This avoids sending an unresolved type parameter into the
                // executable IR while explicit annotations still select the
                // requested type.
                _ if matches!(enum_name, "Option" | "Result") => result.push(
                    self.type_to_annotation(&Type::Int),
                ),
                _ => result.push(self.type_to_annotation(&Type::TypeParameter {
                    name: param.name.clone(),
                })),
            }
        }

        result
    }

    pub(crate) fn generic_enum_pattern_matches(pattern: &Type, concrete: &Type) -> bool {
        let pattern_name = match pattern {
            Type::Enum { name } | Type::Applied { name, .. } => name.as_str(),
            _ => return false,
        };
        let concrete_name = match concrete {
            Type::Enum { name } | Type::Applied { name, .. } => name.as_str(),
            _ => return false,
        };
        // Concrete applications must be compared structurally below.  Treating
        // every `Option<T>`/`Result<T, E>` pair as the same here would accept
        // `Option<int>` where `Option<string>` was declared.
        if matches!(pattern, Type::Applied { .. })
            && matches!(concrete, Type::Applied { .. })
        {
            return false;
        }
        matches!(pattern_name, "Option" | "Result")
            && (concrete_name == pattern_name
                || concrete_name.starts_with(&format!("{pattern_name}_")))
    }

    pub(crate) fn infer_enum_type_args_from_named_fields(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        fields: &[(String, Expression)],
    ) -> Option<Vec<crate::ast::TypeAnnotation>> {
        let (type_params, _) = self.generic_enums.get(enum_name)?.clone();
        let variant_info = self
            .enum_infos
            .get(enum_name)
            .and_then(|info| info.variants.get(variant_name))
            .cloned()?;
        let field_templates = variant_info.struct_data?;

        let mut type_map = HashMap::new();
        for (field_name, field_ann) in &field_templates {
            let field_expr = fields
                .iter()
                .find(|(actual_name, _)| actual_name == field_name)
                .map(|(_, expr)| expr)?;
            let value_type = self.infer_expression_type(field_expr);
            self.unify_type_annotation(field_ann, &value_type, &mut type_map);
        }

        let mut result = Vec::with_capacity(type_params.len());
        for param in type_params {
            match type_map.get(&param.name) {
                Some(mapped) if !matches!(mapped, Type::Unknown) => {
                    result.push(self.type_to_annotation(mapped));
                }
                _ => return None,
            }
        }
        Some(result)
    }

    /// Unify a type annotation (potentially containing type variables) with a concrete type
    pub(crate) fn unify_type_annotation(
        &self,
        type_ann: &crate::ast::TypeAnnotation,
        concrete_type: &Type,
        type_map: &mut HashMap<String, Type>,
    ) {
        use crate::ast::TypeAnnotationKind;

        match &type_ann.kind {
            TypeAnnotationKind::Simple { segments } => {
                // If it's a single segment, it might be a type parameter
                if segments.len() == 1 {
                    let name = &segments[0];
                    // Check if this could be a type parameter (starts with uppercase typically)
                    // For now, we'll assume any single segment could be a type parameter
                    // and try to map it
                    match type_map.entry(name.clone()) {
                        Entry::Occupied(mut entry) => {
                            if !self.types_compatible(entry.get(), concrete_type) {
                                entry.insert(Type::Unknown);
                            }
                        }
                        Entry::Vacant(entry) => {
                            entry.insert(concrete_type.clone());
                        }
                    }
                }
            }
            TypeAnnotationKind::Tuple { elements } => {
                if let Type::Tuple {
                    elements: concrete_elements,
                } = concrete_type
                {
                    if elements.len() != concrete_elements.len() {
                        return;
                    }
                    // Unify each element
                    for (elem_ann, elem_type) in elements.iter().zip(concrete_elements.iter()) {
                        self.unify_type_annotation(elem_ann, elem_type, type_map);
                    }
                }
            }
            TypeAnnotationKind::Function { .. } => {}
            TypeAnnotationKind::Generic { name, type_args } => {
                // Unify type args if the concrete type is a matching enum
                // For now, just record the base name as a simple mapping
                let _ = (name, type_args);
            }
            TypeAnnotationKind::DynTrait { .. } => {}
        }
    }

    /// Convert a Type to TypeAnnotation
    pub(crate) fn type_to_annotation(&self, ty: &Type) -> crate::ast::TypeAnnotation {
        use crate::ast::{TypeAnnotation, TypeAnnotationKind};
        use crate::span::Span;

        let kind = match ty {
            Type::Int => TypeAnnotationKind::Simple {
                segments: vec!["int".to_string()],
            },
            Type::Float => TypeAnnotationKind::Simple {
                segments: vec!["float".to_string()],
            },
            Type::ExactInt { signed, width } => TypeAnnotationKind::Simple {
                segments: vec![format!("{}{}", if *signed { "i" } else { "u" }, match width { IntWidth::I8 => "8", IntWidth::I16 => "16", IntWidth::I32 => "32", IntWidth::I64 => "64", IntWidth::Isize | IntWidth::Usize => "size" })],
            },
            Type::ExactFloat { width } => TypeAnnotationKind::Simple {
                segments: vec![match width { FloatWidth::F32 => "f32".to_string(), FloatWidth::F64 => "f64".to_string() }],
            },
            Type::Bool => TypeAnnotationKind::Simple {
                segments: vec!["bool".to_string()],
            },
            Type::String => TypeAnnotationKind::Simple {
                segments: vec!["string".to_string()],
            },
            Type::Char => TypeAnnotationKind::Simple {
                segments: vec!["char".to_string()],
            },
            Type::Unit => TypeAnnotationKind::Simple {
                segments: vec!["void".to_string()],
            },
            Type::Struct { name } => TypeAnnotationKind::Simple {
                segments: vec![name.clone()],
            },
            Type::Enum { name, .. } => TypeAnnotationKind::Simple {
                segments: vec![name.clone()],
            },
            Type::Applied { name, args } => TypeAnnotationKind::Generic {
                name: name.clone(),
                type_args: args.iter().map(|arg| self.type_to_annotation(arg)).collect(),
            },
            Type::Tuple { elements } => {
                let element_anns = elements
                    .iter()
                    .map(|el| self.type_to_annotation(el))
                    .collect();
                TypeAnnotationKind::Tuple {
                    elements: element_anns,
                }
            }
            Type::Array { element_type, .. } => {
                // For arrays, we'll just use the element type name
                let elem_ann = self.type_to_annotation(element_type);
                // Simplification: return element type (proper array annotation would need size)
                elem_ann.kind
            }
            Type::TypeParameter { name } => TypeAnnotationKind::Simple {
                segments: vec![name.clone()],
            },
            Type::SelfType => TypeAnnotationKind::Simple {
                segments: vec!["Self".to_string()],
            },
            Type::Unknown => TypeAnnotationKind::Simple {
                segments: vec!["unknown".to_string()],
            },
            Type::Fn { .. } => TypeAnnotationKind::Simple {
                segments: vec!["fn".to_string()],
            },
            Type::Task { output } => TypeAnnotationKind::Generic {
                name: "Task".to_string(),
                type_args: vec![self.type_to_annotation(output)],
            },
            Type::Range => TypeAnnotationKind::Simple {
                segments: vec!["Range".to_string()],
            },
            Type::Tensor {
                dtype,
                rank,
                dims,
                layout,
                device,
            } => {
                let dtype_ann = self.type_to_annotation(dtype);
                let mut type_args = vec![dtype_ann];
                if let Some(rank) = rank {
                    type_args.push(Self::simple_type_annotation(&format!("rank{}", rank)));
                }
                if let Some(dims) = dims {
                    for dim in dims {
                        type_args.push(Self::simple_type_annotation(&match dim {
                            Some(size) => format!("dim{}", size),
                            None => "dyn".to_string(),
                        }));
                    }
                }
                if let Some(layout) = layout {
                    type_args.push(Self::simple_type_annotation(layout));
                }
                if let Some(device) = device {
                    type_args.push(Self::simple_type_annotation(device));
                }
                TypeAnnotationKind::Generic {
                    name: "Tensor".to_string(),
                    type_args,
                }
            }
            Type::DynTrait {
                trait_name,
                auto_traits,
            } => TypeAnnotationKind::DynTrait {
                trait_name: trait_name.clone(),
                auto_traits: auto_traits.clone(),
            },
        };

        TypeAnnotation {
            kind,
            span: Span {
                start: 0,
                end: 0,
                start_location: crate::span::Location { line: 0, column: 0 },
                end_location: crate::span::Location { line: 0, column: 0 },
            },
        }
    }

    fn simple_type_annotation(name: &str) -> crate::ast::TypeAnnotation {
        crate::ast::TypeAnnotation {
            kind: crate::ast::TypeAnnotationKind::Simple {
                segments: vec![name.to_string()],
            },
            span: Span::dummy(),
        }
    }
}
