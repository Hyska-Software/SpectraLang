// Module Registry — cross-module symbol table
// Stores the exported symbols of each module so the semantic analyzer
// can resolve imports from other modules.

use std::collections::HashMap;

use crate::ast::{Function, Type, TypeAnnotation};

/// How visible an exported symbol is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportVisibility {
    /// Visible to any importer.
    Public,
    /// Visible only to modules within the same package (`spectra.toml` `name`).
    Internal,
}

/// A function exported from a module.
#[derive(Debug, Clone)]
pub struct ExportedFunction {
    pub params: Vec<Type>,
    pub return_type: Type,
    pub visibility: ExportVisibility,
    pub is_async: bool,
}

/// A module-level mutable static exported to downstream modules.
#[derive(Debug, Clone)]
pub struct ExportedStatic {
    pub ty: Type,
    pub visibility: ExportVisibility,
    /// Fully-qualified IR global key used by JIT/AOT data symbols.
    pub qualified_name: String,
}

/// How an exported method receives `self`, when it is an instance method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportedSelfParamKind {
    Value,
    Reference { mutable: bool },
}

/// A method exported from an inherent impl block.
#[derive(Debug, Clone)]
pub struct ExportedMethod {
    pub params: Vec<Type>,
    pub return_type: Type,
    pub visibility: ExportVisibility,
    pub self_kind: Option<ExportedSelfParamKind>,
    pub is_async: bool,
}

/// A method exported from a trait declaration.
#[derive(Debug, Clone)]
pub struct ExportedTraitMethod {
    pub params: Vec<Type>,
    pub return_type: Type,
    pub self_kind: Option<ExportedSelfParamKind>,
    pub is_async: bool,
    pub has_default: bool,
}

/// A trait exported from a module.
#[derive(Debug, Clone)]
pub struct ExportedTrait {
    pub methods: HashMap<String, ExportedTraitMethod>,
    pub visibility: ExportVisibility,
}

/// A trait implementation made available by an imported module.
///
/// Trait implementations are coherence metadata rather than callable exports,
/// so they must travel through the registry even when the implementation's
/// methods are not independently public.
#[derive(Debug, Clone)]
pub struct ExportedTraitImpl {
    pub trait_name: String,
    pub type_name: String,
    pub type_args: Vec<Type>,
}

/// A type (struct or enum) exported from a module.
#[derive(Debug, Clone)]
pub struct ExportedType {
    /// Field names for structs, variant names for enums (in declaration order).
    pub members: Vec<String>,
    pub visibility: ExportVisibility,
    /// True if it's an enum, false if struct.
    pub is_enum: bool,
    /// For structs: field name -> type annotation.
    pub struct_fields: Option<HashMap<String, TypeAnnotation>>,
    /// For enums: variant name -> tuple payload types (None for unit/struct-data variants).
    pub enum_variants: Option<HashMap<String, Option<Vec<TypeAnnotation>>>>,
    /// For enums: variant name -> named-field list, for struct-data variants only.
    /// e.g. `Variant { x: int, y: int }` -> `Some([("x", int), ("y", int)])`.
    pub enum_struct_variants: Option<HashMap<String, Vec<(String, TypeAnnotation)>>>,
}

/// All public symbols exported by a single module.
#[derive(Debug, Clone, Default)]
pub struct ModuleExports {
    pub functions: HashMap<String, ExportedFunction>,
    pub statics: HashMap<String, ExportedStatic>,
    /// Public generic function templates retained for downstream
    /// monomorphization in a multi-module build.
    pub generic_functions: HashMap<String, Function>,
    pub types: HashMap<String, ExportedType>,
    pub traits: HashMap<String, ExportedTrait>,
    /// Trait implementations available to downstream modules.
    pub trait_impls: Vec<ExportedTraitImpl>,
    /// Inherent methods exported by type name, then method name.
    pub methods: HashMap<String, HashMap<String, ExportedMethod>>,
    /// Package this module belongs to (from `spectra.toml` `name` field).
    pub package_name: Option<String>,
    /// Stdlib module path segments for builtin modules (e.g. ["std","io"]).
    /// When set, callee resolution maps bare names to this path prefix.
    pub stdlib_path: Option<Vec<String>>,
}

/// The global cross-module symbol registry.
/// One registry is shared across all modules compiled in a single pipeline run.
#[derive(Debug, Default)]
pub struct ModuleRegistry {
    /// Maps module canonical path (e.g. "std.io", "utils.math") to its exports.
    modules: HashMap<String, ModuleExports>,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the exports of a compiled module.
    pub fn register_module(&mut self, path: String, exports: ModuleExports) {
        self.modules.insert(path, exports);
    }

    /// Look up the exports of a module by its canonical path.
    pub fn get_module(&self, path: &str) -> Option<&ModuleExports> {
        self.modules.get(path)
    }

    /// Look up a specific exported function.
    pub fn lookup_function(&self, module_path: &str, name: &str) -> Option<&ExportedFunction> {
        self.modules.get(module_path)?.functions.get(name)
    }

    /// Look up a specific exported type.
    pub fn lookup_type(&self, module_path: &str, name: &str) -> Option<&ExportedType> {
        self.modules.get(module_path)?.types.get(name)
    }

    pub fn lookup_static(&self, module_path: &str, name: &str) -> Option<&ExportedStatic> {
        self.modules.get(module_path)?.statics.get(name)
    }

    /// Returns true if the module is already registered.
    pub fn is_registered(&self, module_path: &str) -> bool {
        self.modules.contains_key(module_path)
    }

    /// Iterate over registered modules for tooling and contract extraction.
    ///
    /// The compiler remains the authority for the semantic shape of builtin
    /// exports; consumers must not reconstruct this table from source-text
    /// regexes when producing diagnostics or contract snapshots.
    pub fn iter_modules(&self) -> impl Iterator<Item = (&str, &ModuleExports)> {
        self.modules
            .iter()
            .map(|(path, exports)| (path.as_str(), exports))
    }
}
