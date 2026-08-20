fn item_to_completion_items(item: &spectra_compiler::ast::Item) -> Vec<CompletionItem> {
    match item {
        spectra_compiler::ast::Item::Import(import) => vec![CompletionItem {
            label: import
                .alias
                .clone()
                .unwrap_or_else(|| import.path.join("::")),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some(format!("import {}", import.path.join("::"))),
            ..Default::default()
        }],
        spectra_compiler::ast::Item::Function(function) => vec![CompletionItem {
            label: function.name.clone(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(format_function_signature(function)),
            ..Default::default()
        }],
        spectra_compiler::ast::Item::Struct(struct_def) => {
            let mut items = vec![CompletionItem {
                label: struct_def.name.clone(),
                kind: Some(CompletionItemKind::STRUCT),
                detail: Some(format!("record {}", struct_def.name)),
                ..Default::default()
            }];

            for field in &struct_def.fields {
                items.push(CompletionItem {
                    label: field.name.clone(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some(format!(
                        "{}: {}",
                        field.name,
                        format_type_annotation(&field.ty)
                    )),
                    ..Default::default()
                });
            }

            items
        }
        spectra_compiler::ast::Item::Enum(enum_def) => {
            let mut items = vec![CompletionItem {
                label: enum_def.name.clone(),
                kind: Some(CompletionItemKind::ENUM),
                detail: Some(format!("enum {}", enum_def.name)),
                ..Default::default()
            }];

            for variant in &enum_def.variants {
                items.push(CompletionItem {
                    label: variant.name.clone(),
                    kind: Some(CompletionItemKind::ENUM_MEMBER),
                    detail: Some(format!("{}::{}", enum_def.name, variant.name)),
                    ..Default::default()
                });
            }

            items
        }
        spectra_compiler::ast::Item::Impl(impl_block) => impl_block
            .methods
            .iter()
            .map(|method| CompletionItem {
                label: method.name.clone(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(format_method_signature(&impl_block.type_name, method)),
                ..Default::default()
            })
            .collect(),
        spectra_compiler::ast::Item::Trait(trait_def) => {
            let mut items = vec![CompletionItem {
                label: trait_def.name.clone(),
                kind: Some(CompletionItemKind::INTERFACE),
                detail: Some(format!("trait {}", trait_def.name)),
                ..Default::default()
            }];

            for method in &trait_def.methods {
                items.push(CompletionItem {
                    label: method.name.clone(),
                    kind: Some(CompletionItemKind::METHOD),
                    detail: Some(format_trait_method_signature(method)),
                    ..Default::default()
                });
            }

            items
        }
        spectra_compiler::ast::Item::TraitImpl(trait_impl) => trait_impl
            .methods
            .iter()
            .map(|method| CompletionItem {
                label: method.name.clone(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(format_method_signature(&trait_impl.type_name, method)),
                ..Default::default()
            })
            .collect(),
        spectra_compiler::ast::Item::TypeAlias(ta) => vec![CompletionItem {
            label: ta.name.clone(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some(format!("type {}", ta.name)),
            ..Default::default()
        }],
        spectra_compiler::ast::Item::Const(c) => vec![CompletionItem {
            label: c.name.clone(),
            kind: Some(CompletionItemKind::CONSTANT),
            detail: Some(format!("const {}", c.name)),
            ..Default::default()
        }],
        spectra_compiler::ast::Item::Static(s) => vec![CompletionItem {
            label: s.name.clone(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some(format!("static {}", s.name)),
            ..Default::default()
        }],
    }
}

fn document_symbols(module: &spectra_compiler::ast::Module) -> Vec<DocumentSymbol> {
    module.items.iter().map(item_to_document_symbol).collect()
}

fn item_to_document_symbol(item: &spectra_compiler::ast::Item) -> DocumentSymbol {
    match item {
        spectra_compiler::ast::Item::Import(import) => document_symbol_node(
            import
                .alias
                .clone()
                .unwrap_or_else(|| import.path.join("::")),
            Some(format!("import {}", import.path.join("::"))),
            SymbolKind::MODULE,
            import.span,
            None,
        ),
        spectra_compiler::ast::Item::Function(function) => document_symbol_node(
            function.name.clone(),
            Some(format_function_signature(function)),
            SymbolKind::FUNCTION,
            function.span,
            Some(
                function
                    .params
                    .iter()
                    .map(|param| {
                        document_symbol_node(
                            param.name.clone(),
                            param.ty.as_ref().map(format_type_annotation),
                            SymbolKind::VARIABLE,
                            param.span,
                            None,
                        )
                    })
                    .collect(),
            ),
        ),
        spectra_compiler::ast::Item::Struct(struct_def) => document_symbol_node(
            struct_def.name.clone(),
            Some(format!("record {}", struct_def.name)),
            SymbolKind::STRUCT,
            struct_def.span,
            Some(
                struct_def
                    .fields
                    .iter()
                    .map(|field| {
                        document_symbol_node(
                            field.name.clone(),
                            Some(format_type_annotation(&field.ty)),
                            SymbolKind::FIELD,
                            field.span,
                            None,
                        )
                    })
                    .collect(),
            ),
        ),
        spectra_compiler::ast::Item::Enum(enum_def) => document_symbol_node(
            enum_def.name.clone(),
            Some(format!("enum {}", enum_def.name)),
            SymbolKind::ENUM,
            enum_def.span,
            Some(
                enum_def
                    .variants
                    .iter()
                    .map(|variant| {
                        document_symbol_node(
                            variant.name.clone(),
                            Some(format_variant_signature(enum_def, variant)),
                            SymbolKind::ENUM_MEMBER,
                            variant.span,
                            None,
                        )
                    })
                    .collect(),
            ),
        ),
        spectra_compiler::ast::Item::Impl(impl_block) => document_symbol_node(
            impl_block
                .trait_name
                .as_ref()
                .map(|trait_name| format!("impl {} for {}", trait_name, impl_block.type_name))
                .unwrap_or_else(|| format!("impl {}", impl_block.type_name)),
            Some("implementation".to_string()),
            SymbolKind::OBJECT,
            impl_block.span,
            Some(
                impl_block
                    .methods
                    .iter()
                    .map(method_to_document_symbol)
                    .collect(),
            ),
        ),
        spectra_compiler::ast::Item::Trait(trait_def) => document_symbol_node(
            trait_def.name.clone(),
            Some(format!("trait {}", trait_def.name)),
            SymbolKind::INTERFACE,
            trait_def.span,
            Some(
                trait_def
                    .methods
                    .iter()
                    .map(trait_method_to_document_symbol)
                    .collect(),
            ),
        ),
        spectra_compiler::ast::Item::TraitImpl(trait_impl) => document_symbol_node(
            format!(
                "impl {} for {}",
                trait_impl.trait_name, trait_impl.type_name
            ),
            Some("trait implementation".to_string()),
            SymbolKind::OBJECT,
            trait_impl.span,
            Some(
                trait_impl
                    .methods
                    .iter()
                    .map(method_to_document_symbol)
                    .collect(),
            ),
        ),
        spectra_compiler::ast::Item::TypeAlias(ta) => document_symbol_node(
            ta.name.clone(),
            Some("type alias".to_string()),
            SymbolKind::TYPE_PARAMETER,
            ta.span,
            None,
        ),
        spectra_compiler::ast::Item::Const(c) => document_symbol_node(
            c.name.clone(),
            Some("constant".to_string()),
            SymbolKind::CONSTANT,
            c.span,
            None,
        ),
        spectra_compiler::ast::Item::Static(s) => document_symbol_node(
            s.name.clone(),
            Some("static variable".to_string()),
            SymbolKind::VARIABLE,
            s.span,
            None,
        ),
    }
}

fn method_to_document_symbol(method: &spectra_compiler::ast::Method) -> DocumentSymbol {
    document_symbol_node(
        method.name.clone(),
        Some(format_method_signature("Self", method)),
        SymbolKind::METHOD,
        method.span,
        Some(
            method
                .params
                .iter()
                .map(parameter_to_document_symbol)
                .collect(),
        ),
    )
}

fn trait_method_to_document_symbol(method: &spectra_compiler::ast::TraitMethod) -> DocumentSymbol {
    document_symbol_node(
        method.name.clone(),
        Some(format_trait_method_signature(method)),
        SymbolKind::METHOD,
        method.span,
        Some(
            method
                .params
                .iter()
                .map(parameter_to_document_symbol)
                .collect(),
        ),
    )
}

fn parameter_to_document_symbol(param: &spectra_compiler::ast::Parameter) -> DocumentSymbol {
    document_symbol_node(
        param.name.clone(),
        param.type_annotation.as_ref().map(format_type_annotation),
        SymbolKind::VARIABLE,
        param.span,
        None,
    )
}

#[allow(deprecated)]
fn document_symbol_node(
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    span: Span,
    children: Option<Vec<DocumentSymbol>>,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail,
        kind,
        range: span_to_range(span),
        selection_range: span_to_range(span),
        children,
        tags: None,
        deprecated: None,
    }
}

fn format_function_signature(function: &spectra_compiler::ast::Function) -> String {
    let params = function
        .params
        .iter()
        .map(|param| match &param.ty {
            Some(ty) => format!("{}: {}", param.name, format_type_annotation(ty)),
            None => param.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let return_type = function
        .return_type
        .as_ref()
        .map(format_type_annotation)
        .unwrap_or_else(|| "unit".to_string());
    let prefix = if function.is_async { "async func" } else { "func" };
    format!(
        "{} {}({}) returns {}",
        prefix, function.name, params, return_type
    )
}

fn format_method_signature(type_name: &str, method: &spectra_compiler::ast::Method) -> String {
    let params = method
        .params
        .iter()
        .map(format_parameter_signature)
        .collect::<Vec<_>>()
        .join(", ");
    let return_type = method
        .return_type
        .as_ref()
        .map(format_type_annotation)
        .unwrap_or_else(|| "unit".to_string());
    let prefix = if method.is_async { "async func" } else { "func" };
    format!(
        "{} {}::{}({}) returns {}",
        prefix, type_name, method.name, params, return_type
    )
}

fn format_trait_method_signature(method: &spectra_compiler::ast::TraitMethod) -> String {
    let params = method
        .params
        .iter()
        .map(format_parameter_signature)
        .collect::<Vec<_>>()
        .join(", ");
    let return_type = method
        .return_type
        .as_ref()
        .map(format_type_annotation)
        .unwrap_or_else(|| "unit".to_string());
    let prefix = if method.is_async { "async func" } else { "func" };
    format!("{} {}({}) returns {}", prefix, method.name, params, return_type)
}

fn format_parameter_signature(param: &spectra_compiler::ast::Parameter) -> String {
    if param.is_self {
        return match (param.is_reference, param.is_mutable) {
            (true, true) => "&mut self".to_string(),
            (true, false) => "&self".to_string(),
            (false, _) => "self".to_string(),
        };
    }

    match &param.type_annotation {
        Some(ty) => format!("{}: {}", param.name, format_type_annotation(ty)),
        None => param.name.clone(),
    }
}

fn format_variant_signature(
    enum_def: &spectra_compiler::ast::Enum,
    variant: &spectra_compiler::ast::EnumVariant,
) -> String {
    if let Some(fields) = &variant.struct_data {
        let fields = fields
            .iter()
            .map(|(name, ty)| format!("{}: {}", name, format_type_annotation(ty)))
            .collect::<Vec<_>>()
            .join(", ");
        return format!("{}::{} {{ {} }}", enum_def.name, variant.name, fields);
    }

    if let Some(items) = &variant.data {
        let items = items
            .iter()
            .map(format_type_annotation)
            .collect::<Vec<_>>()
            .join(", ");
        return format!("{}::{}({})", enum_def.name, variant.name, items);
    }

    format!("{}::{}", enum_def.name, variant.name)
}

fn format_type_annotation(ty: &spectra_compiler::ast::TypeAnnotation) -> String {
    match &ty.kind {
        spectra_compiler::ast::TypeAnnotationKind::Simple { segments } => segments.join("::"),
        spectra_compiler::ast::TypeAnnotationKind::Tuple { elements } => format!(
            "({})",
            elements
                .iter()
                .map(format_type_annotation)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        spectra_compiler::ast::TypeAnnotationKind::Function {
            params,
            return_type,
        } => format!(
            "func({}) returns {}",
            params
                .iter()
                .map(format_type_annotation)
                .collect::<Vec<_>>()
                .join(", "),
            format_type_annotation(return_type)
        ),
        spectra_compiler::ast::TypeAnnotationKind::Generic { name, type_args } => format!(
            "{}<{}>",
            name,
            type_args
                .iter()
                .map(format_type_annotation)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        spectra_compiler::ast::TypeAnnotationKind::DynTrait {
            trait_name,
            auto_traits,
        } => {
            if auto_traits.is_empty() {
                format!("dyn {}", trait_name)
            } else {
                format!("dyn {} + {}", trait_name, auto_traits.join(" + "))
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum CallKind {
    Function { callee_span: Span },
    Method { call_span: Span },
}

#[derive(Debug, Clone, Copy)]
struct CallSite {
    span: Span,
    search_start: usize,
    kind: CallKind,
}

const TOKEN_NAMESPACE: u32 = 0;
const TOKEN_TYPE: u32 = 1;
const TOKEN_ENUM: u32 = 2;
const TOKEN_INTERFACE: u32 = 3;
const TOKEN_FUNCTION: u32 = 4;
const TOKEN_METHOD: u32 = 5;
const TOKEN_VARIABLE: u32 = 6;
const TOKEN_PARAMETER: u32 = 7;
const TOKEN_PROPERTY: u32 = 8;
const TOKEN_ENUM_MEMBER: u32 = 9;

