use crate::{
    ast::{
        Attribute, AttributeArgument, BinaryOperator, Block, CastMode, ConstDecl, Expression,
        ExpressionKind, FStringPart, FloatWidth, Function, IntWidth, Item, Module, Pattern,
        Statement, StatementKind, StaticDecl, Type, UnaryOperator, Visibility,
    },
    error::SemanticError,
    span::Span,
};
use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, RwLock};

fn import_local_name(
    module_alias: Option<&str>,
    item_alias: Option<&str>,
    exported_name: &str,
) -> String {
    if let Some(alias) = module_alias {
        format!("{}.{}", alias, exported_name)
    } else {
        item_alias.unwrap_or(exported_name).to_string()
    }
}

pub mod builtin_modules;
pub mod module_registry;

use builtin_modules::register_builtin_modules;
use module_registry::{
    ExportVisibility, ExportedFunction, ExportedMethod, ExportedSelfParamKind, ExportedStatic,
    ExportedTrait, ExportedTraitImpl, ExportedTraitMethod, ExportedType, ModuleExports,
    ModuleRegistry,
};

type GenericStructDefinition = (
    Vec<crate::ast::TypeParameter>,
    Vec<(String, crate::ast::TypeAnnotation)>,
);

#[derive(Debug, Clone)]
struct TraitMethodSignature {
    params: Vec<ParameterInfo>,
    return_type: Option<TypeAnnotationPattern>,
    has_default_body: bool,
    is_async: bool,
}

#[derive(Debug, Clone)]
struct ParameterInfo {
    is_self: bool,
    is_reference: bool,
    is_mutable: bool,
    ty: Option<TypeAnnotationPattern>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeAnnotationPattern {
    Simple(Vec<String>),
    Tuple(Vec<TypeAnnotationPattern>),
}

impl fmt::Display for TypeAnnotationPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeAnnotationPattern::Simple(segments) => {
                write!(f, "{}", segments.join("::"))
            }
            TypeAnnotationPattern::Tuple(elements) => {
                write!(f, "(")?;
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", element)?;
                }
                write!(f, ")")
            }
        }
    }
}

/// Returns `true` if `from_ty as to_ty` is a permitted cast.
fn is_cast_valid(from: &Type, to: &Type) -> bool {
    use Type::*;
    match (from, to) {
        // Same type — trivially OK
        (a, b) if a == b => true,
        // Numeric conversions
        (Int, Float) | (Float, Int) => true,
        (Int, ExactInt { .. })
        | (ExactInt { .. }, Int)
        | (ExactInt { .. }, ExactInt { .. })
        | (Float, ExactFloat { .. })
        | (ExactFloat { .. }, Float)
        | (ExactFloat { .. }, ExactFloat { .. })
        | (Int, ExactFloat { .. })
        | (ExactFloat { .. }, Int)
        | (Float, ExactInt { .. })
        | (ExactInt { .. }, Float) => true,
        (ExactFloat { .. }, ExactInt { .. }) | (ExactInt { .. }, ExactFloat { .. }) => true,
        // int ↔ char
        (Int, Char) | (Char, Int) => true,
        (ExactInt { .. }, Char) | (Char, ExactInt { .. }) => true,
        // coerce concrete type to dyn Trait (validated at trait-impl level)
        (Struct { .. }, DynTrait { .. }) => true,
        _ => false,
    }
}

/// Maps a `BinaryOperator` to the Spectra trait name and method name that can overload it.
/// Returns `None` for operators that cannot be overloaded (`&&` / `||`).
fn operator_trait_and_method(
    op: crate::ast::BinaryOperator,
) -> Option<(&'static str, &'static str)> {
    use crate::ast::BinaryOperator;
    match op {
        BinaryOperator::Add => Some(("Add", "add")),
        BinaryOperator::Subtract => Some(("Sub", "sub")),
        BinaryOperator::Multiply => Some(("Mul", "mul")),
        BinaryOperator::Divide => Some(("Div", "div")),
        BinaryOperator::Modulo => Some(("Rem", "rem")),
        BinaryOperator::Equal | BinaryOperator::NotEqual => Some(("Eq", "eq")),
        BinaryOperator::Less => Some(("Ord", "lt")),
        BinaryOperator::LessEqual => Some(("Ord", "le")),
        BinaryOperator::Greater => Some(("Ord", "gt")),
        BinaryOperator::GreaterEqual => Some(("Ord", "ge")),
        BinaryOperator::And | BinaryOperator::Or => None, // logical ops not overloadable
    }
}

pub fn analyze_modules(modules: &mut [&mut Module]) -> Result<(), Vec<SemanticError>> {
    let mut errors = Vec::new();
    // Build a shared registry seeded with the stdlib virtual modules.
    let registry = {
        let mut reg = ModuleRegistry::new();
        register_builtin_modules(&mut reg);
        Arc::new(RwLock::new(reg))
    };

    // Analyze dependency modules before importers, even when the workspace's
    // filesystem order puts an importer first. This keeps forward imports and
    // public re-exports deterministic across project layouts.
    let mut pending: Vec<&mut Module> = modules.iter_mut().map(|module| &mut **module).collect();
    while !pending.is_empty() {
        let pending_names: HashSet<String> =
            pending.iter().map(|module| module.name.clone()).collect();
        let next_index = pending
            .iter()
            .position(|module| {
                module
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        Item::Import(import) => Some(import.path.join(".")),
                        _ => None,
                    })
                    .all(|path| {
                        let registered = registry
                            .read()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .get_module(&path)
                            .is_some();
                        registered || !pending_names.contains(&path)
                    })
            })
            .unwrap_or(0);
        let module = pending.remove(next_index);
        let mut analyzer = SemanticAnalyzer::new_with_registry(Arc::clone(&registry), None);
        let module_errors = analyzer.analyze_module(module);

        // Register the exports of this module so subsequent modules can import it.
        let exports = analyzer.collect_module_exports(module, None);
        let mut reg = registry.write().unwrap_or_else(|p| p.into_inner());
        reg.register_module(module.name.clone(), exports);

        errors.extend(module_errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub is_local: bool,
    pub def_span: Option<Span>,
    pub ty: Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelfParamKind {
    Value,
    Reference { mutable: bool },
}

impl From<SelfParamKind> for ExportedSelfParamKind {
    fn from(kind: SelfParamKind) -> Self {
        match kind {
            SelfParamKind::Value => ExportedSelfParamKind::Value,
            SelfParamKind::Reference { mutable } => ExportedSelfParamKind::Reference { mutable },
        }
    }
}

impl From<ExportedSelfParamKind> for SelfParamKind {
    fn from(kind: ExportedSelfParamKind) -> Self {
        match kind {
            ExportedSelfParamKind::Value => SelfParamKind::Value,
            ExportedSelfParamKind::Reference { mutable } => SelfParamKind::Reference { mutable },
        }
    }
}

impl From<SelfParamKind> for ParameterInfo {
    fn from(kind: SelfParamKind) -> Self {
        match kind {
            SelfParamKind::Value => ParameterInfo {
                is_self: true,
                is_reference: false,
                is_mutable: false,
                ty: None,
            },
            SelfParamKind::Reference { mutable } => ParameterInfo {
                is_self: true,
                is_reference: true,
                is_mutable: mutable,
                ty: None,
            },
        }
    }
}

#[derive(Debug, Clone)]
struct FunctionSignature {
    params: Vec<Type>,
    return_type: Type,
    self_kind: Option<SelfParamKind>,
    is_async: bool,
}

#[derive(Debug, Clone)]
enum ConstValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Char(char),
}

impl ConstValue {
    fn ty(&self) -> Type {
        match self {
            ConstValue::Int(_) => Type::Int,
            ConstValue::Float(_) => Type::Float,
            ConstValue::Bool(_) => Type::Bool,
            ConstValue::String(_) => Type::String,
            ConstValue::Char(_) => Type::Char,
        }
    }
}

#[derive(Debug, Clone)]
struct TraitMethodInfo {
    signature: FunctionSignature,
    has_default: bool, // true if method has default implementation
    #[allow(dead_code)]
    default_body: Option<crate::ast::Block>, // corpo da implementação padrão, se houver
}

#[derive(Debug, Clone)]
struct StructFieldInfo {
    ty: crate::ast::TypeAnnotation,
    #[allow(dead_code)]
    span: Span,
    visibility: Visibility,
}

#[derive(Debug, Clone)]
struct StructInfo {
    visibility: Visibility,
    type_params: Vec<String>,
    fields: HashMap<String, StructFieldInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JsonDeriveSet {
    serialize: bool,
    deserialize: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JsonFieldOptions {
    json_name: String,
    optional: bool,
}

#[derive(Debug, Clone)]
struct JsonDerivedFieldInfo {
    source_name: String,
    json_name: String,
    optional: bool,
    ty: crate::ast::TypeAnnotation,
}

#[derive(Debug, Clone)]
struct JsonDerivedStructInfo {
    fields: Vec<JsonDerivedFieldInfo>,
}

#[derive(Debug, Clone)]
struct EnumVariantInfo {
    data: Option<Vec<crate::ast::TypeAnnotation>>,
    struct_data: Option<Vec<(String, crate::ast::TypeAnnotation)>>,
    #[allow(dead_code)]
    span: Span,
}

#[derive(Debug, Clone)]
struct EnumInfo {
    visibility: Visibility,
    type_params: Vec<String>,
    variants: HashMap<String, EnumVariantInfo>,
}

#[derive(Debug, Default)]
struct TensorMetadata {
    rank: Option<usize>,
    dims: Option<Vec<Option<usize>>>,
    layout: Option<String>,
    device: Option<String>,
}

#[derive(Debug, Clone)]
struct AsyncLocalEvent {
    name: String,
    ty: Type,
    span: Span,
    order: usize,
}

#[derive(Debug, Clone)]
struct AsyncUseEvent {
    name: String,
    order: usize,
}

#[derive(Debug, Clone)]
struct AsyncAwaitEvent {
    span: Span,
    order: usize,
}

#[derive(Debug, Clone)]
struct AsyncTaskBoundaryEvent {
    span: Span,
    names: Vec<String>,
}

#[derive(Debug, Clone)]
struct AsyncSendSyncEvents {
    next_order: usize,
    locals: Vec<AsyncLocalEvent>,
    uses: Vec<AsyncUseEvent>,
    awaits: Vec<AsyncAwaitEvent>,
    task_boundaries: Vec<AsyncTaskBoundaryEvent>,
    env: HashMap<String, Type>,
}

impl AsyncSendSyncEvents {
    fn new() -> Self {
        Self {
            next_order: 0,
            locals: Vec::new(),
            uses: Vec::new(),
            awaits: Vec::new(),
            task_boundaries: Vec::new(),
            env: HashMap::new(),
        }
    }

    fn bump(&mut self) -> usize {
        let order = self.next_order;
        self.next_order += 1;
        order
    }
}

pub struct SemanticAnalyzer {
    errors: Vec<SemanticError>,
    // Symbol table: maps variable/function names to their type info
    symbols: Vec<HashMap<String, SymbolInfo>>,
    // Function table: maps function names to their signatures
    functions: HashMap<String, FunctionSignature>,
    // Generic function type parameters and bounds, keyed by function name.
    function_type_params: HashMap<String, Vec<crate::ast::TypeParameter>>,
    // Enum definitions: maps enum names to their variants
    enum_definitions: HashMap<String, Vec<String>>,
    // Methods: maps type_name to (method_name, signature)
    methods: HashMap<String, HashMap<String, FunctionSignature>>,
    method_definitions: HashMap<String, HashMap<String, Span>>,
    // Per-method visibility: maps type_name -> method_name -> Visibility
    method_visibility: HashMap<String, HashMap<String, Visibility>>,
    // Types that implement the Drop trait (have a destructor).
    drop_types: HashSet<String>,
    // Traits: maps trait_name to (method_name, method_info)
    traits: HashMap<String, HashMap<String, TraitMethodInfo>>,
    trait_signatures: HashMap<String, HashMap<String, TraitMethodSignature>>,
    // Generic type parameters of traits: trait Container<T> (R-213).
    trait_type_params: HashMap<String, Vec<crate::ast::TypeParameter>>,
    // Trait implementations: maps (trait_name, type_name) to validation status
    trait_impls: HashMap<(String, String), bool>,
    // Concrete arguments used by generic trait implementations, keyed by the
    // same pair as `trait_impls` (R-216).
    trait_impl_type_args: HashMap<(String, String), Vec<Type>>,
    // Struct metadata for validation and lookup
    struct_infos: HashMap<String, StructInfo>,
    json_struct_derives: HashMap<String, JsonDerivedStructInfo>,
    // Enum metadata (including variant payload types)
    enum_infos: HashMap<String, EnumInfo>,
    // Generic structs: maps struct_name to (type_params, field_definitions)
    generic_structs: HashMap<String, GenericStructDefinition>,
    // Generic enums: maps enum_name to (type_params, variants)
    generic_enums: HashMap<String, (Vec<crate::ast::TypeParameter>, Vec<String>)>,
    // Type aliases are kept as syntax annotations so the midend can expand
    // them without losing aggregate layout information.
    type_aliases: HashMap<String, crate::ast::TypeAnnotation>,
    // Track if we're inside a loop (for break/continue validation)
    loop_depth: usize,
    // Track if we're inside a function (for return validation)
    current_function: Option<String>,
    current_return_type: Option<Type>,
    current_expected_type: Option<Type>,
    async_context_depth: usize,
    // Stack of in-scope generic type parameters
    generic_params: Vec<HashSet<String>>,
    generic_param_bounds: Vec<HashMap<String, Vec<String>>>,
    // Cross-module registry shared across all modules compiled by a pipeline.
    registry: Arc<RwLock<ModuleRegistry>>,
    // Package name from `spectra.toml` used to check `internal` visibility.
    current_package: Option<String>,
    // Track resolutions of symbols to their definitions to support ide features like Hover and GoToDef
    pub symbol_resolutions: HashMap<Span, SymbolInfo>,
    // Module namespace prefixes registered via imports (e.g. "std", "std.string")
    // so that qualified stdlib calls like `std.string.len(x)` are accepted.
    module_namespaces: HashSet<String>,
    // Maps an imported module alias (for example `compat`) to its canonical
    // stdlib path so contract-sensitive specialization does not lose the
    // namespace provenance behind a qualified call.
    stdlib_namespace_aliases: HashMap<String, String>,
    // Functions discovered via qualified paths (module::fn) during expression analysis.
    // Flushed to module.imported_function_return_types at the end of analyze_module.
    qualified_fn_types: Vec<(String, crate::ast::Type)>,
    const_values: HashMap<String, ConstValue>,
}

// ---------------------------------------------------------------------------
// Standalone helpers
// ---------------------------------------------------------------------------

/// Compute the Levenshtein edit distance between two strings.
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let m = a.len();
    let n = b.len();
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for (i, row) in dp.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, value) in dp[0].iter_mut().enumerate() {
        *value = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if a[i - 1] == b[j - 1] {
                dp[i - 1][j - 1]
            } else {
                1 + dp[i - 1][j].min(dp[i][j - 1]).min(dp[i - 1][j - 1])
            };
        }
    }
    dp[m][n]
}

/// Format a `Type` for display in user-facing error messages.
pub fn type_name(ty: &Type) -> String {
    match ty {
        Type::Int => "int".into(),
        Type::Float => "float".into(),
        Type::ExactInt { signed, width } => format!(
            "{}{}",
            if *signed { "i" } else { "u" },
            match width {
                IntWidth::I8 => "8",
                IntWidth::I16 => "16",
                IntWidth::I32 => "32",
                IntWidth::I64 => "64",
                IntWidth::Isize => "size",
                IntWidth::Usize => "size",
            }
        ),
        Type::ExactFloat { width } => match width {
            FloatWidth::F32 => "f32".into(),
            FloatWidth::F64 => "f64".into(),
        },
        Type::Bool => "bool".into(),
        Type::String => "string".into(),
        Type::Char => "char".into(),
        Type::Unit => "unit".into(),
        Type::Unknown => "unknown".into(),
        Type::Array {
            element_type,
            size: Some(n),
        } => format!("[{}; {}]", type_name(element_type), n),
        Type::Array {
            element_type,
            size: None,
        } => format!("[{}]", type_name(element_type)),
        Type::Tuple { elements } => {
            let inner = elements
                .iter()
                .map(type_name)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({})", inner)
        }
        Type::Struct { name } => name.clone(),
        Type::Enum { name } => name.clone(),
        Type::Applied { name, args } => format!(
            "{}<{}>",
            name,
            args.iter().map(type_name).collect::<Vec<_>>().join(", ")
        ),
        Type::TypeParameter { name } => name.clone(),
        Type::SelfType => "Self".into(),
        Type::Fn {
            params,
            return_type,
        } => {
            let ps = params.iter().map(type_name).collect::<Vec<_>>().join(", ");
            format!("func({}) returns {}", ps, type_name(return_type))
        }
        Type::Task { output } => format!("Task<{}>", type_name(output)),
        Type::Range => "Range".into(),
        Type::Tensor {
            dtype,
            rank,
            dims,
            layout,
            device,
        } => format_tensor_type_name(type_name(dtype), *rank, dims, layout, device),
        Type::DynTrait {
            trait_name,
            auto_traits,
        } => format_dyn_trait_name(trait_name, auto_traits),
    }
}

fn format_tensor_type_name(
    dtype: String,
    rank: Option<usize>,
    dims: &Option<Vec<Option<usize>>>,
    layout: &Option<String>,
    device: &Option<String>,
) -> String {
    let mut parts = vec![dtype];
    if let Some(rank) = rank {
        parts.push(format!("rank{}", rank));
    } else {
        parts.push("dynamic".to_string());
    }
    if let Some(dims) = dims {
        for dim in dims {
            match dim {
                Some(size) => parts.push(format!("dim{}", size)),
                None => parts.push("dyn".to_string()),
            }
        }
    }
    if let Some(layout) = layout {
        parts.push(layout.clone());
    }
    if let Some(device) = device {
        parts.push(device.clone());
    }
    format!("Tensor<{}>", parts.join(", "))
}

fn format_dyn_trait_name(trait_name: &str, auto_traits: &[String]) -> String {
    if auto_traits.is_empty() {
        format!("dyn {}", trait_name)
    } else {
        format!("dyn {} + {}", trait_name, auto_traits.join(" + "))
    }
}

fn auto_trait_bounds_satisfied(actual: &[String], expected: &[String]) -> bool {
    expected
        .iter()
        .all(|bound| actual.iter().any(|actual_bound| actual_bound == bound))
}

fn namespace_path(expr: &Expression) -> Option<String> {
    match &expr.kind {
        ExpressionKind::Identifier(name) => Some(name.clone()),
        ExpressionKind::FieldAccess { object, field } => {
            let mut prefix = namespace_path(object)?;
            prefix.push('.');
            prefix.push_str(field);
            Some(prefix)
        }
        _ => None,
    }
}

include!("semantic_init.rs");
include!("semantic_exports_types.rs");
include!("semantic_type_system.rs");
include!("semantic_annotations.rs");
include!("semantic_tensor_validation.rs");
include!("semantic_tensor_const.rs");
include!("semantic_json.rs");
include!("semantic_module_analysis.rs");
include!("semantic_item_import.rs");
include!("semantic_traits.rs");
include!("semantic_async.rs");
include!("semantic_statements.rs");
include!("semantic_expression_inference.rs");
include!("semantic_capture_helpers.rs");
include!("semantic_expression_analysis.rs");
include!("semantic_expression_literals.rs");
include!("semantic_expression_binary.rs");
include!("semantic_expression_calls.rs");
include!("semantic_expression_aggregates.rs");
include!("semantic_expression_struct.rs");
include!("semantic_expression_fields.rs");
include!("semantic_expression_enums.rs");
include!("semantic_expression_matches.rs");
include!("semantic_expression_methods.rs");
include!("semantic_expression_tail.rs");
include!("semantic_patterns.rs");
include!("semantic_pattern_validation.rs");
include!("semantic_pattern_inference.rs");
include!("semantic_returns.rs");
include!("semantic_exhaustiveness.rs");
include!("semantic_method_fill.rs");
