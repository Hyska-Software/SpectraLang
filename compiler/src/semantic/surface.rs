//! Derived public-surface snapshot for tooling (`spectralang surface --json`).
//!
//! The snapshot is deliberately plain data: the compiler exposes the semantic
//! shape of the project and the CLI owns serialization, so `serde` stays out
//! of this crate. Ordering is deterministic (modules by path, symbols by name)
//! so two runs over the same sources produce byte-identical output.

use std::collections::BTreeMap;

use crate::ast::{TypeAnnotation, TypeAnnotationKind};
use crate::semantic::module_registry::{ModuleExports, ModuleRegistry};
use crate::semantic::type_name;

/// A function, method or static exported by a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceFunction {
    /// Bare source name (`cobrar`, `from_json`).
    pub name: String,
    /// Owner type for methods and associated functions (`Recibo::from_json`).
    pub owner: Option<String>,
    /// Rendered parameter and return types, e.g. `(x: int) -> bool`.
    pub signature: String,
    pub is_async: bool,
    /// `public` or `internal`.
    pub visibility: String,
    /// `function`, `method` or `static`.
    pub kind: String,
}

/// A record or enum exported by a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceType {
    pub name: String,
    /// Field names for records, variant names for enums, in declaration order.
    pub members: Vec<String>,
    pub is_enum: bool,
    pub visibility: String,
}

/// A trait exported by a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceTrait {
    pub name: String,
    /// Rendered method signatures in name order.
    pub methods: Vec<String>,
    pub visibility: String,
}

/// Everything a module exposes to importers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceModule {
    /// Canonical module path (`std.io`, `utils.math`).
    pub path: String,
    pub package: Option<String>,
    /// True for stdlib modules registered as builtins.
    pub is_builtin: bool,
    pub functions: Vec<SurfaceFunction>,
    pub types: Vec<SurfaceType>,
    pub traits: Vec<SurfaceTrait>,
}

/// Deterministic snapshot of the public surface of a compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceSnapshot {
    pub modules: Vec<SurfaceModule>,
}

/// Build a surface snapshot from a populated module registry.
///
/// Builtins are excluded unless `include_builtins` is set; callers distinguish
/// them through `ModuleExports::stdlib_path`, which only builtin modules set.
pub fn snapshot_from_registry(registry: &ModuleRegistry, include_builtins: bool) -> SurfaceSnapshot {
    let mut modules: Vec<SurfaceModule> = registry
        .iter_modules()
        .filter(|(_, exports)| include_builtins || exports.stdlib_path.is_none())
        .map(|(path, exports)| module_snapshot(path, exports))
        .collect();
    modules.sort_by(|a, b| a.path.cmp(&b.path));
    SurfaceSnapshot { modules }
}

fn module_snapshot(path: &str, exports: &ModuleExports) -> SurfaceModule {
    let mut functions: Vec<SurfaceFunction> = exports
        .functions
        .iter()
        .map(|(name, function)| SurfaceFunction {
            name: name.clone(),
            owner: None,
            signature: render_signature(&function.params, &function.return_type, None),
            is_async: function.is_async,
            visibility: render_visibility(&function.visibility),
            kind: "function".to_string(),
        })
        .collect();

    let mut traits: Vec<SurfaceTrait> = exports
        .traits
        .iter()
        .map(|(name, declared)| {
            let mut methods: Vec<String> = declared
                .methods
                .iter()
                .map(|(method_name, method)| {
                    format!(
                        "{}{}",
                        method_name,
                        render_signature(
                            &method.params,
                            &method.return_type,
                            method.self_kind.as_ref()
                        )
                    )
                })
                .collect();
            methods.sort();
            SurfaceTrait {
                name: name.clone(),
                methods,
                visibility: render_visibility(&declared.visibility),
            }
        })
        .collect();

    // Inherent methods and associated functions are exported by owner type.
    for (owner, methods) in &exports.methods {
        for (name, method) in methods {
            functions.push(SurfaceFunction {
                name: name.clone(),
                owner: Some(owner.clone()),
                signature: render_signature(
                    &method.params,
                    &method.return_type,
                    method.self_kind.as_ref(),
                ),
                is_async: method.is_async,
                visibility: render_visibility(&method.visibility),
                kind: if method.self_kind.is_some() {
                    "method".to_string()
                } else {
                    "static".to_string()
                },
            });
        }
    }

    let mut types: Vec<SurfaceType> = exports
        .types
        .iter()
        .map(|(name, ty)| SurfaceType {
            name: name.clone(),
            members: ty.members.clone(),
            is_enum: ty.is_enum,
            visibility: render_visibility(&ty.visibility),
        })
        .collect();

    functions.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.owner.cmp(&b.owner))
            .then_with(|| a.kind.cmp(&b.kind))
    });
    types.sort_by(|a, b| a.name.cmp(&b.name));
    traits.sort_by(|a, b| a.name.cmp(&b.name));

    SurfaceModule {
        path: path.to_string(),
        package: exports.package_name.clone(),
        is_builtin: exports.stdlib_path.is_some(),
        functions,
        types,
        traits,
    }
}

/// Render the textual field type of a record or enum member.
pub fn render_annotation(annotation: &TypeAnnotation) -> String {
    match &annotation.kind {
        TypeAnnotationKind::Simple { segments } => segments.join("::"),
        TypeAnnotationKind::Tuple { elements } => {
            let inner = elements
                .iter()
                .map(render_annotation)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({inner})")
        }
        TypeAnnotationKind::Function {
            params,
            return_type,
        } => {
            let inner = params
                .iter()
                .map(render_annotation)
                .collect::<Vec<_>>()
                .join(", ");
            format!("fn({inner}) -> {}", render_annotation(return_type))
        }
        TypeAnnotationKind::Generic { name, type_args } => {
            let inner = type_args
                .iter()
                .map(render_annotation)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}<{inner}>")
        }
        TypeAnnotationKind::DynTrait {
            trait_name,
            auto_traits,
        } => {
            let mut rendered = format!("dyn {trait_name}");
            for auto_trait in auto_traits {
                rendered.push_str(" + ");
                rendered.push_str(auto_trait);
            }
            rendered
        }
    }
}

/// Render member field types for a snapshot type (declaration order preserved).
pub fn render_member_types(exports_type: &crate::semantic::module_registry::ExportedType) -> BTreeMap<String, String> {
    let mut rendered = BTreeMap::new();
    if let Some(fields) = &exports_type.struct_fields {
        for (name, annotation) in fields {
            rendered.insert(name.clone(), render_annotation(annotation));
        }
    }
    rendered
}

fn render_signature(
    params: &[crate::ast::Type],
    return_type: &crate::ast::Type,
    self_kind: Option<&crate::semantic::module_registry::ExportedSelfParamKind>,
) -> String {
    let mut rendered_params: Vec<String> = Vec::with_capacity(params.len() + 1);
    match self_kind {
        Some(crate::semantic::module_registry::ExportedSelfParamKind::Value) => {
            rendered_params.push("self".to_string())
        }
        Some(crate::semantic::module_registry::ExportedSelfParamKind::Reference { mutable }) => {
            if *mutable {
                rendered_params.push("&mut self".to_string());
            } else {
                rendered_params.push("&self".to_string());
            }
        }
        None => {}
    }
    rendered_params.extend(params.iter().map(type_name));
    format!(
        "({}) -> {}",
        rendered_params.join(", "),
        type_name(return_type)
    )
}

fn render_visibility(visibility: &crate::semantic::module_registry::ExportVisibility) -> String {
    match visibility {
        crate::semantic::module_registry::ExportVisibility::Public => "public".to_string(),
        crate::semantic::module_registry::ExportVisibility::Internal => "internal".to_string(),
    }
}
