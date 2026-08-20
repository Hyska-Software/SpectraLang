// AST to IR lowering pass
// Converts semantic AST to SSA-based IR

use crate::builder::IRBuilder;
use crate::ir::{
    Constant, ExternalFunction, FloatWidth as IRFloatWidth, Function as IRFunction, Global,
    IntWidth as IRIntWidth, LocalDebugInfo, Module as IRModule, Parameter, SourceSpan, Terminator,
    Type as IRType, Value,
};
use crate::layout;
use spectra_compiler::ast::{
    BinaryOperator, Block, Enum as ASTEnum, EnumVariant, Expression, ExpressionKind, FStringPart,
    Function as ASTFunction, IfLetStatement, Item, Method as ASTMethod, Module as ASTModule,
    Statement, StatementKind, Struct as ASTStruct, TraitDeclaration, Type as ASTType,
    TypeAnnotation, TypeAnnotationKind, TypeParameter, UnaryOperator, Visibility,
    WhileLetStatement,
};
use spectra_compiler::error::MidendError;
use spectra_compiler::span::Span;
use std::collections::{HashMap, HashSet};

/// Stack-based scope system for variable shadowing support

#[derive(Debug, Clone)]
enum LoweredConstValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Char(char),
}

type EnumVariantDefinition = (String, usize, Option<Vec<IRType>>);

/// Maps a `BinaryOperator` to the method name used for operator overloading.
/// Returns `None` if the operator cannot be overloaded.
#[inline]
fn operator_overload_method(op: &BinaryOperator) -> Option<&'static str> {
    match op {
        BinaryOperator::Add => Some("add"),
        BinaryOperator::Subtract => Some("sub"),
        BinaryOperator::Multiply => Some("mul"),
        BinaryOperator::Divide => Some("div"),
        BinaryOperator::Modulo => Some("rem"),
        BinaryOperator::Equal | BinaryOperator::NotEqual => Some("eq"),
        BinaryOperator::Less => Some("lt"),
        BinaryOperator::LessEqual => Some("le"),
        BinaryOperator::Greater => Some("gt"),
        BinaryOperator::GreaterEqual => Some("ge"),
        BinaryOperator::And | BinaryOperator::Or => None,
    }
}

/// Stack-based scope system for variable shadowing support
#[derive(Clone)]
struct ScopeStack {
    scopes: Vec<HashMap<String, Value>>,
}

impl ScopeStack {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::with_capacity(16)],
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::with_capacity(8));
    }

    fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    fn insert(&mut self, name: String, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, value);
        }
    }

    fn get(&self, name: &str) -> Option<Value> {
        // Search from innermost to outermost scope
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Some(*value);
            }
        }
        None
    }

    fn clear(&mut self) {
        self.scopes.clear();
        self.scopes.push(HashMap::new());
    }
}

/// Scoped map that tracks IR types associated with variable names
#[derive(Clone)]
struct TypeScopeStack {
    scopes: Vec<HashMap<String, IRType>>,
}

impl TypeScopeStack {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    fn insert(&mut self, name: String, ty: IRType) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, ty);
        }
    }

    fn get(&self, name: &str) -> Option<IRType> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty.clone());
            }
        }
        None
    }

    fn clear(&mut self) {
        self.scopes.clear();
        self.scopes.push(HashMap::new());
    }
}

/// Metadata about an array lowered into IR
#[derive(Clone)]
struct ArrayInfo {
    ptr: Value,
    element_type: IRType,
    size: usize,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
struct RangeInfo {
    start: Value,
    end: Value,
    inclusive: bool,
}

#[derive(Clone)]
struct ClosureCapture {
    name: String,
    ty: IRType,
}

#[derive(Clone)]
struct ClosureInfo {
    signature_params: Vec<IRType>,
    signature_return: IRType,
}

/// Scoped storage for array metadata (pointer, element type, size)
#[derive(Clone)]
struct ArrayScopeStack {
    scopes: Vec<HashMap<String, ArrayInfo>>,
}

impl ArrayScopeStack {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    fn insert(&mut self, name: String, info: ArrayInfo) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, info);
        }
    }

    fn get(&self, name: &str) -> Option<&ArrayInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info);
            }
        }
        None
    }

    fn clear(&mut self) {
        self.scopes.clear();
        self.scopes.push(HashMap::new());
    }
}

#[derive(Clone)]
struct RangeScopeStack {
    scopes: Vec<HashMap<String, RangeInfo>>,
}

impl RangeScopeStack {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    fn insert(&mut self, name: String, info: RangeInfo) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, info);
        }
    }

    #[allow(dead_code)]
    fn get(&self, name: &str) -> Option<RangeInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(*info);
            }
        }
        None
    }

    fn clear(&mut self) {
        self.scopes.clear();
        self.scopes.push(HashMap::new());
    }
}

/// Scoped storage for struct pointers and their associated type names
#[derive(Clone)]
struct StructScopeStack {
    scopes: Vec<HashMap<String, (Value, String)>>,
}

impl StructScopeStack {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    fn insert(&mut self, name: String, info: (Value, String)) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, info);
        }
    }

    fn get(&self, name: &str) -> Option<(Value, String)> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info.clone());
            }
        }
        None
    }

    fn clear(&mut self) {
        self.scopes.clear();
        self.scopes.push(HashMap::new());
    }
}

/// Loop context for break/continue handling
#[derive(Clone)]
struct LoopContext {
    header_block: usize,
    exit_block: usize,
}

#[derive(Clone)]
struct HostFunctionDescriptor {
    runtime_name: &'static str,
    return_type: IRType,
    returns_value: bool,
}

/// Represents a needed specialization of a generic function
#[derive(Debug, Clone)]
struct MonomorphizationRequest {
    /// Name of the generic function
    generic_name: String,
    /// Concrete types to substitute for type parameters (in order)
    concrete_types: Vec<IRType>,
}

impl MonomorphizationRequest {
    /// Generate mangled name for this specialization
    /// Example: process<Point> -> process_Point
    fn mangled_name(&self) -> String {
        let mut name = self.generic_name.clone();
        for ty in &self.concrete_types {
            name.push('_');
            name.push_str(&Self::type_to_string(ty));
        }
        name
    }

    fn type_to_string(ty: &IRType) -> String {
        match ty {
            IRType::Int => "int".to_string(),
            IRType::Float => "float".to_string(),
            IRType::Bool => "bool".to_string(),
            IRType::String => "string".to_string(),
            IRType::Char => "char".to_string(),
            IRType::Pointer(inner) => format!("ptr_{}", Self::type_to_string(inner)),
            IRType::Struct { name, .. } => name.clone(),
            _ => "unknown".to_string(), // Fallback for other types
        }
    }
}

/// Specialization request for a generic impl method on an instantiated struct.
/// The mangled function name is `{instantiated_struct}_{method}` (e.g.
/// `Par_int_get`), matching the static method-call mangling.
#[derive(Debug, Clone)]
struct MethodMonomorphizationRequest {
    /// Instantiated struct name (e.g. "Par_int").
    instantiated_struct: String,
    /// Method name.
    method_name: String,
    /// Concrete types to substitute for the impl type parameters.
    concrete_types: Vec<IRType>,
}

impl MethodMonomorphizationRequest {
    fn mangled_name(&self) -> String {
        format!("{}_{}", self.instantiated_struct, self.method_name)
    }
}

pub struct ASTLowering {
    source_file: String,
    builder: IRBuilder,
    current_function: Option<IRFunction>,
    value_map: ScopeStack,
    variable_types: TypeScopeStack,
    /// Maps variable names to their allocated memory locations (for mutable variables)
    alloca_map: HashMap<String, Value>,
    /// Maps array names to metadata for lowering (scoped)
    array_map: ArrayScopeStack,
    /// Maps range variables to lowered bounds when known in the current function.
    range_map: RangeScopeStack,
    /// Maps struct names to their field definitions
    struct_definitions: HashMap<String, Vec<(String, IRType)>>,
    /// Maps struct variable names to (pointer, struct_name) for field access (scoped)
    struct_var_map: StructScopeStack,
    /// Maps enum names to their variant definitions: (variant_name, tag, data_types)
    enum_definitions: HashMap<String, Vec<EnumVariantDefinition>>,
    /// Preserves declaration order for struct-style enum variant fields.
    enum_variant_field_names: HashMap<String, HashMap<String, Vec<String>>>,
    loop_stack: Vec<LoopContext>,
    /// Maps generic function names to their AST definitions (for monomorphization)
    generic_functions: HashMap<String, ASTFunction>,
    /// Maps generic struct names to their AST definitions
    generic_structs: HashMap<String, ASTStruct>,
    /// Maps generic enum names to their AST definitions
    generic_enums: HashMap<String, ASTEnum>,
    /// Maps local type aliases to their target annotations.
    type_aliases: HashMap<String, TypeAnnotation>,
    /// Generic impl methods on generic structs: "Base_method" -> (method, type_params).
    /// These are specialized per instantiation when a method call on an
    /// instantiated struct is lowered (R-211).
    generic_impl_methods: HashMap<String, (ASTMethod, Vec<TypeParameter>)>,
    /// Maps instantiated struct names to (base generic name, concrete IR types):
    /// "Par_int" -> ("Par", [Int]).
    instantiated_structs: HashMap<String, (String, Vec<IRType>)>,
    /// Pending specializations of generic impl methods: mangled name -> request.
    pending_method_specializations: Vec<MethodMonomorphizationRequest>,
    /// Requests for monomorphization that need to be processed
    pending_specializations: Vec<MonomorphizationRequest>,
    /// Already generated specializations (mangled_name -> IR function name)
    generated_specializations: HashMap<String, String>,
    /// Type substitution map for current monomorphization (type_param -> concrete_type)
    type_substitution_map: HashMap<String, IRType>,
    /// Maps (type_name, trait_name) -> true to track trait implementations
    trait_implementations: HashMap<(String, String), bool>,
    /// Tracks return types for lowered functions (including specializations)
    function_return_types: HashMap<String, IRType>,
    /// Tracks public parameter types so a named function can be materialized
    /// as a closure value with the hidden environment ABI.
    function_parameter_types: HashMap<String, Vec<IRType>>,
    /// Maps bare imported names to their full stdlib path for unqualified call resolution.
    /// Populated from `Module::std_import_aliases` at the start of `lower_module()`.
    /// e.g. "print" → ["std", "io", "print"]
    std_import_aliases: HashMap<String, Vec<String>>,
    /// Counter for generating unique lambda function names.
    lambda_counter: usize,
    /// Lambdas collected during lowering that will be emitted as top-level IR functions.
    pending_lambdas: Vec<IRFunction>,
    /// Maps variable names that hold closures to their generated function and captured values.
    closure_var_map: HashMap<String, ClosureInfo>,
    /// Return type annotation of the function currently being lowered.
    /// Used to resolve Generic enum type args when they can't be fully inferred from
    /// the construction expression (e.g., Result::Ok(x) in a fn -> Result<int, string>).
    current_function_return_annotation: Option<TypeAnnotation>,
    current_async_output_type: Option<IRType>,
    async_state_counter: usize,
    /// Expected expression type annotation from a local binding or other typed context.
    /// This refines generic aggregate constructors such as `Result::Ok(x)` when the
    /// expression itself only determines part of the generic argument list.
    current_expected_annotation: Option<TypeAnnotation>,
    /// Maps trait names to their methods in declaration order (for vtable slot lookup).
    trait_method_order: HashMap<String, Vec<String>>,
    /// Maps trait names to method signatures for dyn dispatch and type inference.
    trait_method_signatures: HashMap<String, HashMap<String, (Vec<IRType>, IRType)>>,
    /// Stores parsed trait declarations so default methods can be materialized for impls.
    trait_declarations: HashMap<String, TraitDeclaration>,
    /// Accumulated lowering errors that replace previous panics.
    errors: Vec<MidendError>,
    /// Compile-time constants lowered as literals at each use site.
    const_values: HashMap<String, LoweredConstValue>,
    /// Module-level mutable globals lowered to IR globals and addressed by name.
    static_globals: HashMap<String, (String, IRType)>,
    /// Borrowed receiver parameters are visible in `struct_var_map` but are
    /// not owned by the current method and must not be destroyed on return.
    drop_excluded_names: HashSet<String>,
}

include!("lowering_impl_core.rs");
include!("lowering_impl_module.rs");
include!("lowering_impl_monomorphization.rs");
include!("lowering_impl_types.rs");
include!("lowering_impl_type_inference.rs");
include!("lowering_impl_functions.rs");
include!("lowering_impl_methods.rs");
include!("lowering_impl_closures.rs");
include!("lowering_impl_blocks.rs");
include!("lowering_impl_loops.rs");
include!("lowering_impl_statements.rs");
include!("lowering_impl_hosts.rs");
include!("lowering_impl_expression.rs");
include!("lowering_expr_literals.rs");
include!("lowering_expr_binary.rs");
include!("lowering_expr_calls.rs");
include!("lowering_expr_aggregates.rs");
include!("lowering_expr_struct.rs");
include!("lowering_expr_fields.rs");
include!("lowering_expr_enum.rs");
include!("lowering_expr_match.rs");
include!("lowering_expr_method.rs");
include!("lowering_expr_tail.rs");
include!("lowering_expr_cast.rs");
include!("lowering_impl_cast_dyn.rs");
include!("lowering_impl_patterns.rs");
include!("lowering_impl_types_tail.rs");
include!("lowering_impl_substitution.rs");
include!("lowering_builtins.rs");
include!("lowering_std_host.rs");
include!("lowering_std_host_numeric.rs");
include!("lowering_std_host_math_io_error.rs");
include!("lowering_std_host_tensor_ml.rs");
include!("lowering_std_host_collections_string.rs");
include!("lowering_std_host_convert_time.rs");
include!("lowering_std_host_legacy.rs");
include!("lowering_std_api.rs");
include!("lowering_handles.rs");
include!("lowering_default.rs");
include!("lowering_tests.rs");
