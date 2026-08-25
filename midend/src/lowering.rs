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
pub(crate) enum LoweredConstValue {
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
pub(crate) struct RangeInfo {
    start: Value,
    end: Value,
    inclusive: bool,
}

#[derive(Clone)]
pub(crate) struct ClosureCapture {
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
pub(crate) struct HostFunctionDescriptor {
    runtime_name: &'static str,
    return_type: IRType,
    returns_value: bool,
}

/// Represents a needed specialization of a generic function
#[derive(Debug, Clone)]
pub(crate) struct MonomorphizationRequest {
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
pub(crate) struct MethodMonomorphizationRequest {
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

// Former include! monolith, decomposed into real child modules. The glob
// re-exports recreate the flat namespace the includes provided; most names are
// consumed by the children themselves via `use super::*`.
#[path = "lowering_impl_core.rs"] mod lowering_impl_core;
#[path = "lowering_impl_module.rs"] mod lowering_impl_module;
#[path = "lowering_impl_monomorphization.rs"] mod lowering_impl_monomorphization;
#[path = "lowering_impl_types.rs"] mod lowering_impl_types;
#[path = "lowering_impl_type_inference.rs"] mod lowering_impl_type_inference;
#[path = "lowering_impl_functions.rs"] mod lowering_impl_functions;
#[path = "lowering_impl_methods.rs"] mod lowering_impl_methods;
#[path = "lowering_impl_closures.rs"] mod lowering_impl_closures;
#[path = "lowering_impl_blocks.rs"] mod lowering_impl_blocks;
#[path = "lowering_impl_loops.rs"] mod lowering_impl_loops;
#[path = "lowering_impl_statements.rs"] mod lowering_impl_statements;
#[path = "lowering_impl_hosts.rs"] mod lowering_impl_hosts;
#[path = "lowering_impl_expression.rs"] mod lowering_impl_expression;
#[path = "lowering_expr_literals.rs"] mod lowering_expr_literals;
#[path = "lowering_expr_binary.rs"] mod lowering_expr_binary;
#[path = "lowering_expr_calls.rs"] mod lowering_expr_calls;
#[path = "lowering_expr_aggregates.rs"] mod lowering_expr_aggregates;
#[path = "lowering_expr_struct.rs"] mod lowering_expr_struct;
#[path = "lowering_expr_fields.rs"] mod lowering_expr_fields;
#[path = "lowering_expr_enum.rs"] mod lowering_expr_enum;
#[path = "lowering_expr_match.rs"] mod lowering_expr_match;
#[path = "lowering_expr_method.rs"] mod lowering_expr_method;
#[path = "lowering_expr_tail.rs"] mod lowering_expr_tail;
#[path = "lowering_expr_cast.rs"] mod lowering_expr_cast;
#[path = "lowering_impl_cast_dyn.rs"] mod lowering_impl_cast_dyn;
#[path = "lowering_impl_patterns.rs"] mod lowering_impl_patterns;
#[path = "lowering_impl_types_tail.rs"] mod lowering_impl_types_tail;
#[path = "lowering_impl_substitution.rs"] mod lowering_impl_substitution;
#[path = "lowering_builtins.rs"] mod lowering_builtins;
#[path = "lowering_std_host.rs"] mod lowering_std_host;
#[path = "lowering_std_host_numeric.rs"] mod lowering_std_host_numeric;
#[path = "lowering_std_host_math_io_error.rs"] mod lowering_std_host_math_io_error;
#[path = "lowering_std_host_tensor_ml.rs"] mod lowering_std_host_tensor_ml;
#[path = "lowering_std_host_collections_string.rs"] mod lowering_std_host_collections_string;
#[path = "lowering_std_host_convert_time.rs"] mod lowering_std_host_convert_time;
#[path = "lowering_std_host_legacy.rs"] mod lowering_std_host_legacy;
#[path = "lowering_std_api.rs"] mod lowering_std_api;
#[path = "lowering_handles.rs"] mod lowering_handles;
#[path = "lowering_default.rs"] mod lowering_default;

/// Public re-export preserved from the pre-split layout: consumed by
/// `packages/spectra-api` (contract-drift test) as
/// `spectra_midend::lowering::std_api_host_call_target`.
pub use lowering_std_api::std_api_host_call_target;

#[allow(unused_imports)]
use {
    lowering_impl_core::*,
    lowering_impl_module::*,
    lowering_impl_monomorphization::*,
    lowering_impl_types::*,
    lowering_impl_type_inference::*,
    lowering_impl_functions::*,
    lowering_impl_methods::*,
    lowering_impl_closures::*,
    lowering_impl_blocks::*,
    lowering_impl_loops::*,
    lowering_impl_statements::*,
    lowering_impl_hosts::*,
    lowering_impl_expression::*,
    lowering_expr_literals::*,
    lowering_expr_binary::*,
    lowering_expr_calls::*,
    lowering_expr_aggregates::*,
    lowering_expr_struct::*,
    lowering_expr_fields::*,
    lowering_expr_enum::*,
    lowering_expr_match::*,
    lowering_expr_method::*,
    lowering_expr_tail::*,
    lowering_expr_cast::*,
    lowering_impl_cast_dyn::*,
    lowering_impl_patterns::*,
    lowering_impl_types_tail::*,
    lowering_impl_substitution::*,
    lowering_builtins::*,
    lowering_std_host::*,
    lowering_std_host_numeric::*,
    lowering_std_host_math_io_error::*,
    lowering_std_host_tensor_ml::*,
    lowering_std_host_collections_string::*,
    lowering_std_host_convert_time::*,
    lowering_std_host_legacy::*,
    lowering_std_api::*,
    lowering_handles::*,
    lowering_default::*,
};

#[cfg(test)]
#[path = "lowering_tests.rs"]
mod lowering_tests;
