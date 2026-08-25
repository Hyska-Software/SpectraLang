// Spectra Intermediate Representation (IR)
// SSA-based representation with explicit control flow

pub mod pretty;

/// Stable source location carried from the Spectra AST into native debug
/// generation. Lines and columns are one-based, matching compiler spans.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub file: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDebugInfo {
    pub name: String,
    pub ty: Type,
    pub value_id: Option<usize>,
    pub declaration: Option<SourceSpan>,
    pub scope_start: Option<SourceSpan>,
    pub scope_end: Option<SourceSpan>,
}

/// IR Module - top level container
#[derive(Debug, Clone)]
pub struct Module {
    pub name: String,
    pub functions: Vec<Function>,
    /// Function declarations supplied by another source module. These are
    /// emitted as native linker imports by AOT code generation and are not
    /// lowered as bodies in this module.
    pub external_functions: Vec<ExternalFunction>,
    pub globals: Vec<Global>,
    pub source_file: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExternalFunction {
    pub name: String,
    pub params: Vec<Type>,
    pub return_type: Type,
}

/// Global variable
#[derive(Debug, Clone)]
pub struct Global {
    pub id: usize,
    pub name: String,
    pub ty: Type,
    pub is_mutable: bool,
    pub initializer: Option<Constant>,
}

/// Function in IR
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<Parameter>,
    pub return_type: Type,
    pub blocks: Vec<BasicBlock>,
    pub next_value_id: usize,
    pub next_block_id: usize,
    pub source_span: Option<SourceSpan>,
    pub locals: Vec<LocalDebugInfo>,
}

/// Function parameter
#[derive(Debug, Clone)]
pub struct Parameter {
    pub id: usize,
    pub name: String,
    pub ty: Type,
}

/// Basic block in SSA form
#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: usize,
    pub label: String,
    pub instructions: Vec<Instruction>,
    pub terminator: Option<Terminator>,
}

/// SSA Instruction
#[derive(Debug, Clone)]
pub struct Instruction {
    pub id: usize,
    pub kind: InstructionKind,
    pub source_span: Option<SourceSpan>,
}

#[derive(Debug, Clone)]
pub enum InstructionKind {
    // Arithmetic
    Add {
        result: Value,
        lhs: Value,
        rhs: Value,
    },
    Sub {
        result: Value,
        lhs: Value,
        rhs: Value,
    },
    Mul {
        result: Value,
        lhs: Value,
        rhs: Value,
    },
    Div {
        result: Value,
        lhs: Value,
        rhs: Value,
    },
    Rem {
        result: Value,
        lhs: Value,
        rhs: Value,
    },

    // Comparisons
    Eq {
        result: Value,
        lhs: Value,
        rhs: Value,
    },
    Ne {
        result: Value,
        lhs: Value,
        rhs: Value,
    },
    Lt {
        result: Value,
        lhs: Value,
        rhs: Value,
    },
    Le {
        result: Value,
        lhs: Value,
        rhs: Value,
    },
    Gt {
        result: Value,
        lhs: Value,
        rhs: Value,
    },
    Ge {
        result: Value,
        lhs: Value,
        rhs: Value,
    },

    // Logical
    And {
        result: Value,
        lhs: Value,
        rhs: Value,
    },
    Or {
        result: Value,
        lhs: Value,
        rhs: Value,
    },
    Not {
        result: Value,
        operand: Value,
    },

    // Memory
    Alloca {
        result: Value,
        ty: Type,
    },
    /// Address of a module-level mutable global.
    GlobalAddr {
        result: Value,
        name: String,
        ty: Type,
    },
    /// Runtime manual heap allocation (tracked, frame-scoped). Used for values
    /// that must outlive the current frame, such as dyn Trait vtables (R-210).
    ManualAlloc {
        result: Value,
        size: i64,
    },
    /// Escapes a manual allocation to the base frame so it survives
    /// `frame_exit` of the current function (R-210).
    EscapeManualAlloc {
        ptr: Value,
    },
    Load {
        result: Value,
        ptr: Value,
        ty: Type,
    },
    Store {
        ptr: Value,
        value: Value,
    },
    GetElementPtr {
        result: Value,
        ptr: Value,
        index: Value,
        element_type: Type,
    },
    /// Pointer arithmetic with a constant byte offset for aggregate fields.
    /// Used for struct/tuple/enum fields whose layout requires padding and
    /// cumulative offsets (see `crate::layout`).
    FieldPtr {
        result: Value,
        ptr: Value,
        offset: i64,
    },

    // Function calls
    Call {
        result: Option<Value>,
        function: String,
        args: Vec<Value>,
        /// True when this direct self-call sits in tail position: the only
        /// remaining action of its block is returning this call's result.
        /// The backend fuses it (plus the trailing `Return`) into a native
        /// Cranelift tail call. Only *self*-recursion is marked; cross-function
        /// tail calls are out of scope.
        is_tail: bool,
    },
    // Host function invocation
    HostCall {
        result: Option<Value>,
        host: String,
        args: Vec<Value>,
        result_type: Option<Type>,
    },
    /// Compiler-generated reverse-mode autodiff step. Unlike `HostCall`, this
    /// node has a closed operation contract and is dispatched by the backend
    /// to a dedicated reverse kernel.
    AutodiffStep {
        result: Option<Value>,
        operation: String,
        /// Forward output whose saved creator/value is consumed by the reverse kernel.
        output: Value,
        upstream: Option<Value>,
        inputs: Vec<Value>,
        targets: Vec<Value>,
    },
    /// Get the address of a named function as an opaque i64 pointer (for closures/HOF).
    FuncAddr {
        result: Value,
        function: String,
    },
    /// Indirect call through a function pointer (closures passed as arguments).
    CallIndirect {
        result: Option<Value>,
        fn_ptr: Value,
        args: Vec<Value>,
        /// Parameter types of the callee signature (used to build SigRef in the backend).
        signature_params: Vec<Type>,
        /// Return type of the callee signature.
        signature_return: Box<Type>,
    },
    /// Produce a ready task handle from a completed async result.
    AsyncReady {
        result: Value,
        value: Option<Value>,
        output_type: Type,
    },

    // PHI node for SSA
    Phi {
        result: Value,
        incoming: Vec<(Value, usize)>,
    },

    // Copy/Move
    Copy {
        result: Value,
        source: Value,
    },

    // Constants (for literal values)
    ConstInt {
        result: Value,
        value: i64,
    },
    ConstIntTyped {
        result: Value,
        value: i64,
        ty: Type,
    },
    ConstFloat {
        result: Value,
        value: f64,
    },
    ConstFloatTyped {
        result: Value,
        value: f64,
        ty: Type,
    },
    ConstBool {
        result: Value,
        value: bool,
    },
    /// String literal value. Codegen resolves this to a stable pointer
    /// (global data section in AOT, heap-allocated immutable buffer in
    /// JIT). Length is always known at compile time and the bytes are stored
    /// packed UTF-8, terminated by a single NUL byte (`len + 1` bytes).
    ConstString {
        result: Value,
        value: String,
    },
    /// Numeric type conversion: int↔float, int↔char
    Cast {
        result: Value,
        operand: Value,
        from_ty: Type,
        to_ty: Type,
    },
    /// Build a fat pointer (data_ptr, vtable_ptr) for `T as dyn Trait`.
    MakeDynFatPtr {
        result: Value,
        data_ptr: Value,
        vtable_ptr: Value,
    },
    /// Load the data pointer from a fat pointer (dyn Trait object).
    LoadDynDataPtr {
        result: Value,
        fat_ptr: Value,
    },
    /// Load the vtable pointer from a fat pointer (dyn Trait object).
    LoadDynVtablePtr {
        result: Value,
        fat_ptr: Value,
    },
    /// Load a function pointer from a vtable at a given slot index.
    LoadVtableSlot {
        result: Value,
        vtable_ptr: Value,
        slot_index: usize,
    },
}

/// Block terminator (control flow)
#[derive(Debug, Clone)]
pub enum Terminator {
    Return {
        value: Option<Value>,
    },
    Branch {
        target: usize,
    },
    CondBranch {
        condition: Value,
        true_block: usize,
        false_block: usize,
    },
    Switch {
        value: Value,
        cases: Vec<(i64, usize)>,
        default: usize,
    },
    Unreachable,
}

/// SSA Value
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Value {
    pub id: usize,
}

/// IR Type system
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// A type that could not be resolved by semantic analysis or lowering.
    ///
    /// This is deliberately not a backend representation.  It exists so the
    /// midend can preserve the distinction between a real unit value (`Void`)
    /// and an unresolved value.  Verification must reject modules containing
    /// this variant before any backend is invoked.
    Unknown,
    Void,
    Int,
    Float,
    ExactInt {
        signed: bool,
        width: IntWidth,
    },
    ExactFloat {
        width: FloatWidth,
    },
    Bool,
    String,
    Char,
    Pointer(Box<Type>),
    Array {
        element_type: Box<Type>,
        size: usize,
    },
    Tuple {
        elements: Vec<Type>,
    },
    Struct {
        name: String,
        fields: Vec<(String, Type)>,
    },
    Enum {
        name: String,
        variants: Vec<(String, Option<Vec<Type>>)>, // (name, data_types)
    },
    /// A concrete generic application whose arguments remain explicit in IR.
    /// `representation` is the ABI/layout form used by existing lowering and
    /// backend code while the application is migrated away from name-only
    /// mangling.
    Generic {
        name: String,
        args: Vec<Type>,
        representation: Box<Type>,
    },
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },
    /// Async task/future handle. Lowered as an opaque i64 handle by the current backend.
    Task {
        output: Box<Type>,
    },
    /// Opaque integer range handle.
    Range,
    /// Runtime tensor handle carrying compiler-visible dtype/rank/shape/layout/device metadata.
    Tensor {
        dtype: Box<Type>,
        rank: Option<usize>,
        dims: Option<Vec<Option<usize>>>,
        layout: Option<String>,
        device: Option<String>,
    },
    /// Fat pointer for dyn Trait objects: (data_ptr: i64, vtable_ptr: i64).
    DynTrait {
        trait_name: String,
        auto_traits: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntWidth {
    I8,
    I16,
    I32,
    I64,
    Isize,
    Usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FloatWidth {
    F32,
    F64,
}

/// Constant values
#[derive(Debug, Clone)]
pub enum Constant {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Char(char),
    Null,
}

impl Module {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            functions: Vec::new(),
            external_functions: Vec::new(),
            globals: Vec::new(),
            source_file: None,
        }
    }

    pub fn add_function(&mut self, function: Function) {
        self.functions.push(function);
    }

    pub fn get_function(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|f| f.name == name)
    }
}

impl Function {
    pub fn new(name: impl Into<String>, params: Vec<Parameter>, return_type: Type) -> Self {
        let param_count = params.len();
        Self {
            name: name.into(),
            params,
            return_type,
            blocks: Vec::new(),
            next_value_id: param_count, // Start after parameters
            next_block_id: 0,
            source_span: None,
            locals: Vec::new(),
        }
    }

    pub fn add_block(&mut self, label: impl Into<String>) -> usize {
        let id = self.next_block_id;
        self.next_block_id += 1;

        self.blocks.push(BasicBlock {
            id,
            label: label.into(),
            instructions: Vec::new(),
            terminator: None,
        });

        id
    }

    pub fn get_block(&self, id: usize) -> Option<&BasicBlock> {
        self.blocks.iter().find(|b| b.id == id)
    }

    pub fn get_block_mut(&mut self, id: usize) -> Option<&mut BasicBlock> {
        self.blocks.iter_mut().find(|b| b.id == id)
    }

    pub fn next_value(&mut self) -> Value {
        let id = self.next_value_id;
        self.next_value_id += 1;
        Value { id }
    }
}

impl BasicBlock {
    pub fn add_instruction(&mut self, kind: InstructionKind) -> usize {
        let id = self.instructions.len();
        self.instructions.push(Instruction {
            id,
            kind,
            source_span: None,
        });
        id
    }

    pub fn set_terminator(&mut self, terminator: Terminator) {
        self.terminator = Some(terminator);
    }
}

impl Type {
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Type::Int | Type::Float | Type::ExactInt { .. } | Type::ExactFloat { .. }
        )
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Type::Int | Type::ExactInt { .. })
    }

    pub fn is_bool(&self) -> bool {
        matches!(self, Type::Bool)
    }
}
