//! Native debug record generation shared by AOT producers.
//!
//! Cranelift exposes source locations on its IR, but `cranelift-object` does
//! not currently serialize them to CodeView/DWARF.  This module owns the
//! format-level CodeView records we attach to COFF objects.  The linker then
//! consumes these records and produces the PDB; the JSON sidecar is never used
//! as a substitute for the native artifact.

use crate::aot::{NativeValueLocation, NativeValueLocationRange};
use std::collections::{HashMap, HashSet};

use spectra_midend::ir::{FloatWidth, IntWidth, Type as IRType};

/// CodeView simple (primitive) type indices, from the canonical `cvconst.h`
/// table. `T_INT4 == 0x74` is the real 32-bit signed integer primitive and
/// is only emitted for compiler-proven `i32` values.
/// `T_UNKNOWN == 0x0000` is the reserved typeless index, emitted only when
/// no type information exists at all. It is an explicit unknown marker, not
/// a claimed type: debuggers must not interpret it as `void`, `int`, or any
/// other concrete type.
pub const T_UNKNOWN: u32 = 0x0000;
pub const T_VOID: u32 = 0x0003;
pub const T_CHAR: u32 = 0x0010;
pub const T_UCHAR: u32 = 0x0020;
pub const T_BOOL08: u32 = 0x0030;
pub const T_REAL32: u32 = 0x0040;
pub const T_REAL64: u32 = 0x0042;
pub const T_RCHAR: u32 = 0x0070;
pub const T_WCHAR: u32 = 0x0071;
pub const T_INT2: u32 = 0x0072;
pub const T_UINT2: u32 = 0x0073;
pub const T_INT4: u32 = 0x0074;
pub const T_UINT4: u32 = 0x0075;
pub const T_INT8: u32 = 0x0076;
pub const T_UINT8: u32 = 0x0077;

const LF_POINTER: u16 = 0x1002;
const LF_STRUCTURE: u16 = 0x1505;
const LF_FIELDLIST: u16 = 0x1203;
const LF_MEMBER: u16 = 0x150D;
const CV_IS_FWDREF: u8 = 0x80;
/// First type index available for `.debug$T` user records; everything below
/// is reserved for simple types.
const FIRST_USER_TYPE_INDEX: u32 = 0x1000;
/// CodeView subsection kinds used by the C13 debug stream.
const DEBUG_S_SYMBOLS: u32 = 0xF1;
const DEBUG_S_LINES: u32 = 0xF2;
const DEBUG_S_FILECHKSMS: u32 = 0xF4;
const DEBUG_S_STRINGTABLE: u32 = 0xF3;
const S_GPROC32: u16 = 0x110F;
const S_LOCAL: u16 = 0x113E;
const S_DEFRANGE_REGISTER: u16 = 0x1141;
const S_DEFRANGE_FRAMEPOINTER_REL: u16 = 0x1142;
const S_END: u16 = 0x114F;
const S_OBJNAME: u16 = 0x1101;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeViewFunction {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    pub section: u16,
    pub locals: Vec<String>,
    /// CFA-relative offsets resolved by Cranelift's allocator. `None` means
    /// the compiler could not prove a native location and therefore no
    /// def-range record is emitted for that local.
    pub local_offsets: Vec<Option<i64>>,
    /// Exact, compiler-proven location ranges resolved by Cranelift. Each
    /// range is relative to the beginning of this function's code range.
    /// This is the authoritative representation for native debug emission;
    /// `local_offsets` remains as a compatibility fallback for callers that
    /// only have the older CFA-only metadata.
    pub local_locations: Vec<Vec<NativeValueLocationRange>>,
    /// Real stack-frame size in bytes, captured from Cranelift's finalized
    /// layout (`Function::fixed_stack_size` after legalization covers explicit
    /// allocas plus spill slots). Zero means the function reserved no sized
    /// stack slots. Emitted as `cbFrame` in the `S_FRAMEPROC` record.
    pub frame_size: u32,
    /// IR type of each entry of `locals`, index-aligned with it. Empty (or a
    /// shorter vector) means type information was unavailable for the tail
    /// entries and they are marked `T_UNKNOWN`, never a guessed type.
    pub local_types: Vec<IRType>,
    /// Return type used for the procedure record's type index when present;
    /// without it the procedure is marked `T_UNKNOWN`.
    pub return_type: Option<IRType>,
    /// Real source-line rows captured during codegen, sorted by offset:
    /// `(machine-code offset relative to this function's start, 1-based
    /// source line)`. Each row comes from an IR instruction whose lowering
    /// populated `source_span`; Cranelift's post-allocation value-label pass
    /// supplies the machine offset. Empty means no span-derived row exists
    /// for this function and the emitted line table is empty: addresses map
    /// to no line rather than to an invented uniform distribution.
    pub line_rows: Vec<(u32, u32)>,
}

/// One entry of a native line table: machine-code offset relative to the
/// start of the function, and the 1-based source line it belongs to.
pub type LineRow = (u32, u32);

/// Build the final `(offset, line)` row table for one function.
///
/// `real_rows` are span-derived rows captured during codegen. Only
/// compiler-proven rows are emitted, sorted by offset with out-of-range
/// rows dropped. When no real row exists the table is empty: debuggers see
/// no line information instead of an invented uniform distribution over the
/// source lines. Compiler-generated instructions, optimized-out values
/// without a live range, and IR from passes that do not propagate spans
/// therefore contribute silence, not guesses.
pub fn line_table_rows(
    function_size: u32,
    _source_line_count: u32,
    real_rows: &[LineRow],
) -> Vec<LineRow> {
    let function_size = function_size.max(1);
    let mut rows: Vec<LineRow> = real_rows
        .iter()
        .copied()
        .filter(|(offset, _)| *offset < function_size)
        .collect();
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// Convert a Cranelift x86-64 hardware-register encoding to the CodeView
/// register namespace. Cranelift follows the architectural encoding order
/// (RAX, RCX, RDX, ...), while CodeView orders the first three as RAX, RDX,
/// RCX and uses the 64-bit register IDs beginning at 0x148.
pub fn codeview_x64_register(hw_enc: u8) -> Option<u16> {
    const REGISTERS: [u16; 16] = [
        0x148, // RAX
        0x14A, // RCX
        0x149, // RDX
        0x14B, // RBX
        0x14C, // RSP
        0x14D, // RBP
        0x14E, // RSI
        0x14F, // RDI
        0x150, // R8
        0x151, // R9
        0x152, // R10
        0x153, // R11
        0x154, // R12
        0x155, // R13
        0x156, // R14
        0x157, // R15
    ];
    REGISTERS.get(hw_enc as usize).copied()
}

/// Map a primitive IR type to its fixed CodeView simple index, or `None` when
/// the type requires a `.debug$T` record (pointers, aggregates). The mapping
/// mirrors `CodeGenerator::ir_type_to_cranelift`: `Int` is 64-bit, `Float` is
/// 64-bit, and every aggregate is represented by a pointer at the ABI level.
pub fn codeview_primitive_index(ty: &IRType) -> Option<u32> {
    let index = match ty {
        IRType::Int => T_INT8,
        IRType::ExactInt { signed, width } => match (*signed, *width) {
            (true, IntWidth::I8) => T_RCHAR,
            (true, IntWidth::I16) => T_INT2,
            (true, IntWidth::I32) => T_INT4,
            // I64, Isize (the platform word is 64-bit on this target).
            (true, _) => T_INT8,
            (false, IntWidth::I8) => T_UCHAR,
            (false, IntWidth::I16) => T_UINT2,
            (false, IntWidth::I32) => T_UINT4,
            (false, _) => T_UINT8,
        },
        IRType::Float => T_REAL64,
        IRType::ExactFloat { width } => match width {
            FloatWidth::F32 => T_REAL32,
            FloatWidth::F64 => T_REAL64,
        },
        IRType::Bool => T_BOOL08,
        IRType::Char => T_CHAR,
        IRType::Void | IRType::Unknown => T_VOID,
        _ => return None,
    };
    Some(index)
}

/// Name of the declaration-only user-defined type emitted for an aggregate.
/// Real struct/enum names are preserved so a debugger can resolve them
/// against the runtime's own type names; everything else gets a stable
/// Spectra-prefixed name.
pub fn aggregate_udt_name(ty: &IRType) -> String {
    match ty {
        IRType::Struct { name, .. } | IRType::Enum { name, .. } => name.clone(),
        IRType::Generic { name, .. } => name.to_string(),
        IRType::String => "spectra_string".to_string(),
        IRType::Tuple { elements } => format!("spectra_tuple{}", elements.len()),
        IRType::DynTrait { trait_name, .. } => format!("dyn_{trait_name}"),
        IRType::Task { .. } => "spectra_task".to_string(),
        IRType::Range => "spectra_range".to_string(),
        IRType::Tensor { .. } => "spectra_tensor".to_string(),
        IRType::Function { .. } => "spectra_function".to_string(),
        other => format!("spectra_aggregate_{:?}", std::mem::discriminant(other)),
    }
}

/// Builder for the `.debug$T` CodeView type stream.
/// Primitive types use their fixed simple indices and emit no records. An
/// aggregate whose IR definition carries no field list (or any non-struct
/// aggregate) interns a minimal forward-reference `LF_STRUCTURE` (carrying
/// its real name) followed by an `LF_POINTER` to it, matching the pointer
/// representation aggregates have at the ABI level. A `Struct` whose fields
/// are known in the IR instead emits a full definition: one `LF_FIELDLIST`
/// holding an `LF_MEMBER` per field (byte offsets from the mid-end's shared
/// layout module) plus the defining `LF_STRUCTURE` record, so debuggers can
/// render members directly. Indices are assigned monotonically from 0x1000.
#[derive(Debug)]
pub struct CodeViewTypeTable {
    records: Vec<u8>,
    next_index: u32,
    cache: HashMap<String, u32>,
    /// Type keys currently being defined. Guards against infinite recursion
    /// for self-referential types (`Pointer` back to the same struct): while
    /// a definition is in progress a re-entry falls back to the
    /// declaration-only form; the debugger links both by type name.
    in_progress: HashSet<String>,
}

impl Default for CodeViewTypeTable {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeViewTypeTable {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            next_index: FIRST_USER_TYPE_INDEX,
            cache: HashMap::new(),
            in_progress: HashSet::new(),
        }
    }

    /// Resolve the CodeView type index for one IR type, interning aggregate
    /// records on first use. The result is stable across calls.
    pub fn index_for(&mut self, ty: &IRType) -> u32 {
        if let Some(primitive) = codeview_primitive_index(ty) {
            return primitive;
        }
        if let IRType::Generic { representation, .. } = ty {
            // Generic applications carry their ABI form explicitly in IR;
            // the debugger sees through to the representation.
            return self.index_for(representation);
        }
        let key = format!("{ty:?}");
        if let Some(&index) = self.cache.get(&key) {
            return index;
        }
        let index = match ty {
            IRType::Pointer(inner) => {
                let pointee = self.index_for(inner);
                self.push_pointer(pointee)
            }
            IRType::Array { element_type, .. } => {
                // Arrays lower to raw element pointers on this target.
                let element = self.index_for(element_type);
                self.push_pointer(element)
            }
            IRType::Struct { name, fields } if !fields.is_empty() => {
                if self.in_progress.contains(&key) {
                    // Self-referential type reached through one of its own
                    // members: fall back to the declaration-only form. The
                    // debugger resolves the forward reference by name once
                    // the defining record is interned.
                    let forward = self.push_forward_ref_structure(name);
                    self.push_pointer(forward)
                } else {
                    self.in_progress.insert(key.clone());
                    let structure = self.push_struct_definition(name, fields);
                    self.in_progress.remove(&key);
                    self.push_pointer(structure)
                }
            }
            other => {
                let forward = self.push_forward_ref_structure(&aggregate_udt_name(other));
                self.push_pointer(forward)
            }
        };
        self.cache.insert(key, index);
        index
    }

    /// True once at least one user-defined record has been interned; when
    /// false the whole `.debug$T` section can be omitted because only simple
    /// indices were referenced.
    pub fn has_user_types(&self) -> bool {
        !self.records.is_empty()
    }

    /// Serialized `.debug$T` contents: version signature then records.
    pub fn finish(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.records.len() + 4);
        push_u32(&mut out, 4); // .debug$T version
        out.extend_from_slice(&self.records);
        out
    }

    fn push_pointer(&mut self, pointee: u32) -> u32 {
        let index = self.next_index;
        self.next_index += 1;
        let mut payload = Vec::with_capacity(10);
        payload.extend_from_slice(&pointee.to_le_bytes());
        // Attributes: pointer mode Near64 (11) in bits 6..12; near pointer
        // kind, no const/volatile qualifiers.
        payload.extend_from_slice(&0x0000_02C0u32.to_le_bytes());
        push_type_record(&mut self.records, LF_POINTER, &payload);
        index
    }

    fn push_forward_ref_structure(&mut self, name: &str) -> u32 {
        let index = self.next_index;
        self.next_index += 1;
        let mut payload = Vec::new();
        payload.extend_from_slice(&0u16.to_le_bytes()); // member count
        payload.push(CV_IS_FWDREF); // properties: forward reference
        payload.push(0); // padding
        payload.extend_from_slice(&0u32.to_le_bytes()); // field list
        payload.extend_from_slice(&0u32.to_le_bytes()); // derivation list
        payload.extend_from_slice(&0u32.to_le_bytes()); // vshape table
        payload.extend_from_slice(&0u64.to_le_bytes()); // size unknown
        payload.extend_from_slice(name.as_bytes());
        payload.push(0);
        push_type_record(&mut self.records, LF_STRUCTURE, &payload);
        index
    }
    /// Emit a full user-defined-type definition for a `Struct` whose fields
    /// are known in the IR: an `LF_FIELDLIST` with one public `LF_MEMBER` per
    /// field (byte offsets and total size from the mid-end layout module,
    /// the same source the backend's field-access lowering uses), followed by
    /// the defining `LF_STRUCTURE` record. Member types that are themselves
    /// aggregates resolve to pointer records, matching their 8-byte stored
    /// representation.
    fn push_struct_definition(&mut self, name: &str, fields: &[(String, IRType)]) -> u32 {
        let layout = spectra_midend::layout::layout_of(fields.iter().map(|(_, ty)| ty));
        let mut fieldlist_payload = Vec::new();
        for ((field_name, field_ty), &offset) in fields.iter().zip(&layout.offsets) {
            let member_type = self.index_for(field_ty);
            let mut payload = Vec::new();
            payload.extend_from_slice(&3u16.to_le_bytes()); // attr: public access
            payload.extend_from_slice(&0u16.to_le_bytes()); // padding
            payload.extend_from_slice(&member_type.to_le_bytes());
            payload.extend_from_slice(&(offset as u16).to_le_bytes());
            payload.extend_from_slice(field_name.as_bytes());
            payload.push(0);
            push_type_record(&mut fieldlist_payload, LF_MEMBER, &payload);
        }
        let fieldlist_index = self.next_index;
        self.next_index += 1;
        push_type_record(&mut self.records, LF_FIELDLIST, &fieldlist_payload);

        let structure_index = self.next_index;
        self.next_index += 1;
        let mut payload = Vec::new();
        payload.extend_from_slice(&(fields.len() as u16).to_le_bytes()); // member count
        payload.push(0); // properties: fully defined, not a forward reference
        payload.push(0); // padding
        payload.extend_from_slice(&fieldlist_index.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes()); // derivation list
        payload.extend_from_slice(&0u32.to_le_bytes()); // vshape table
        payload.extend_from_slice(&(layout.size as u64).to_le_bytes());
        payload.extend_from_slice(name.as_bytes());
        payload.push(0);
        push_type_record(&mut self.records, LF_STRUCTURE, &payload);
        structure_index
    }
}

/// Environment variable that requests the JIT debug sidecar on `run`.
pub const JIT_DEBUG_ENV: &str = "SPECTRA_JIT_DEBUG";

/// One variable entry of the JIT debug sidecar.
///
/// Every field is compiler-proven: `name`/`type_name` come from the IR
/// local's debug info, `line` from its declaration span, and each
/// `(start, end)` range is a machine-code offset pair (relative to the start
/// of the compiled function) resolved by Cranelift's post-allocation
/// value-label pass — the exact collection also feeding AOT native debug
/// emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JitDebugVariable {
    pub name: String,
    pub type_name: String,
    pub line: Option<u32>,
    pub ranges: Vec<(u32, u32)>,
}

/// True when `SPECTRA_JIT_DEBUG=1` requests the sidecar. Callers with an
/// explicit `--timings` flag pass `force = true` to [`write_jit_debug_sidecar`]
/// instead of relying on this check.
pub fn jit_debug_requested() -> bool {
    std::env::var(JIT_DEBUG_ENV).is_ok_and(|value| value == "1")
}

/// Sidecar path convention: `<source>.spectra-jit-debug.json` next to the
/// source file.
pub fn jit_debug_sidecar_path(source_file: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{source_file}.spectra-jit-debug.json"))
}

/// Render the sidecar JSON: `{ "<function>": [ {"name", "type", "line",
/// "ranges": [[start, end], ...]}, ... ] }`. Functions are sorted by name so
/// output is deterministic across runs. Hand-rolled serialization keeps the
/// backend free of a JSON dependency; strings are minimally escaped.
pub fn jit_debug_sidecar_json(functions: &[(String, Vec<JitDebugVariable>)]) -> String {
    fn escape(value: &str) -> String {
        let mut out = String::with_capacity(value.len() + 2);
        for ch in value.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                }
                c => out.push(c),
            }
        }
        out
    }

    let mut ordered: Vec<&(String, Vec<JitDebugVariable>)> = functions.iter().collect();
    ordered.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::from("{");
    for (index, (function, variables)) in ordered.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(&escape(function));
        out.push_str("\":[");
        for (var_index, var) in variables.iter().enumerate() {
            if var_index > 0 {
                out.push(',');
            }
            let line = match var.line {
                Some(line) => line.to_string(),
                None => "null".to_string(),
            };
            let ranges = var
                .ranges
                .iter()
                .map(|(start, end)| format!("[{start},{end}]"))
                .collect::<Vec<_>>()
                .join(",");
            out.push_str(&format!(
                "{{\"name\":\"{}\",\"type\":\"{}\",\"line\":{line},\"ranges\":[{ranges}]}}",
                escape(&var.name),
                escape(&var.type_name),
            ));
        }
        out.push(']');
    }
    out.push('}');
    out
}

/// Write `<source>.spectra-jit-debug.json` when requested.
///
/// The sidecar is produced only from Cranelift's proven value-label data
/// collected during JIT compilation (see [`JitDebugVariable`]); nothing here
/// reconstructs records from source text or guesses offsets. It supplements
/// but never replaces native CodeView/DWARF artifacts.
///
/// Writes when `force` is set (`--timings`) or `SPECTRA_JIT_DEBUG=1`;
/// otherwise returns `Ok(None)` without touching the filesystem. A write
/// failure returns `Err`, which callers treat as a non-fatal warning.
pub fn write_jit_debug_sidecar(
    source_file: &str,
    functions: &[(String, Vec<JitDebugVariable>)],
    force: bool,
) -> Result<Option<std::path::PathBuf>, String> {
    if !force && !jit_debug_requested() {
        return Ok(None);
    }
    if functions.is_empty() {
        return Ok(None);
    }
    let path = jit_debug_sidecar_path(source_file);
    std::fs::write(&path, jit_debug_sidecar_json(functions)).map_err(|error| {
        format!(
            "failed to write JIT debug sidecar {}: {error}",
            path.display()
        )
    })?;
    Ok(Some(path))
}

/// Human-readable name of one IR type, used in the sidecar's `type` field.
/// Primitives use their DWARF base-type names, aggregates keep their real
/// IR names (see [`aggregate_udt_name`]).
pub fn ir_type_debug_name(ty: &IRType) -> String {
    if let IRType::Generic { representation, .. } = ty {
        return ir_type_debug_name(representation);
    }
    match crate::dwarf::primitive_base_type(ty) {
        Some((name, _, _)) => String::from_utf8_lossy(&name).into_owned(),
        None => aggregate_udt_name(ty),
    }
}

/// Append one `.debug$T` leaf record: 16-bit length (excluding the length
/// field), leaf id, payload, padded to a 4-byte stream boundary.
fn push_type_record(out: &mut Vec<u8>, leaf: u16, payload: &[u8]) {
    let length = 2usize.saturating_add(payload.len());
    push_u16(out, length as u16);
    push_u16(out, leaf);
    out.extend_from_slice(payload);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Minimal RFC 1321 MD5, used only for CodeView `DEBUG_S_FILECHKSMS` file
/// checksums. The backend has no hashing dependency beyond this; the digest
/// is over the exact source text that was compiled.
fn md5(input: &[u8]) -> [u8; 16] {
    const S: [usize; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let mut message = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_le_bytes());

    let mut a0: u32 = 0x6745_2301;
    let mut b0: u32 = 0xefcd_ab89;
    let mut c0: u32 = 0x98ba_dcfe;
    let mut d0: u32 = 0x1032_5476;

    for chunk in message.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (index, word) in m.iter_mut().enumerate() {
            *word = u32::from_le_bytes(chunk[index * 4..index * 4 + 4].try_into().unwrap());
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for index in 0..64 {
            let (f, g) = match index {
                0..=15 => ((b & c) | (!b & d), index),
                16..=31 => ((d & b) | (!d & c), (5 * index + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * index + 5) % 16),
                _ => (c ^ (b | !d), (7 * index) % 16),
            };
            let temp = d;
            d = c;
            c = b;
            let sum = a.wrapping_add(f).wrapping_add(K[index]).wrapping_add(m[g]);
            b = b.wrapping_add(sum.rotate_left(S[index] as u32));
            a = temp;
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut digest = [0u8; 16];
    digest[0..4].copy_from_slice(&a0.to_le_bytes());
    digest[4..8].copy_from_slice(&b0.to_le_bytes());
    digest[8..12].copy_from_slice(&c0.to_le_bytes());
    digest[12..16].copy_from_slice(&d0.to_le_bytes());
    digest
}

fn subsection(out: &mut Vec<u8>, kind: u32, payload: &[u8]) {
    push_u32(out, kind);
    push_u32(out, payload.len() as u32);
    out.extend_from_slice(payload);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn symbol_record(out: &mut Vec<u8>, kind: u16, payload: &[u8]) {
    // The length includes the kind and any alignment padding.  Omitting the
    // padding from this field makes the next padding bytes look like another
    // symbol record and causes MSVC to report LNK4209/LNK1103.
    let padding = (4 - ((4 + payload.len()) % 4)) % 4;
    let record_len = 2usize.saturating_add(payload.len()).saturating_add(padding);
    push_u16(out, record_len as u16);
    push_u16(out, kind);
    out.extend_from_slice(payload);
    out.resize(out.len() + padding, 0);
}

fn local_location_ranges(
    function: &CodeViewFunction,
    local_index: usize,
) -> Vec<NativeValueLocationRange> {
    if let Some(ranges) = function.local_locations.get(local_index) {
        if !ranges.is_empty() {
            return ranges.clone();
        }
    }
    function
        .local_offsets
        .get(local_index)
        .and_then(|offset| {
            offset.map(|offset| NativeValueLocationRange {
                start: 0,
                end: function.size.max(1),
                location: NativeValueLocation::CfaOffset(offset),
            })
        })
        .into_iter()
        .collect()
}
/// `proc_type_index` is the resolved CodeView type index for the return type
/// and `local_type_indices` holds the per-local indices; both come from the
/// shared [`CodeViewTypeTable`] built by [`codeview_sections`]. Missing entries
/// are marked `T_UNKNOWN`, the explicit unknown marker, never a guessed type.
fn function_symbols(
    function: &CodeViewFunction,
    proc_type_index: u32,
    local_type_indices: &[u32],
) -> Vec<u8> {
    let mut symbols = Vec::new();
    let mut proc = Vec::new();
    push_u32(&mut proc, 0); // parent
    push_u32(&mut proc, 0); // end
    push_u32(&mut proc, 0); // next
    push_u32(&mut proc, function.size.max(1));
    push_u32(&mut proc, 0); // debug start
    push_u32(&mut proc, function.size.max(1)); // debug end
    push_u32(&mut proc, proc_type_index);
    push_u32(&mut proc, function.offset); // section-relative offset
    push_u16(&mut proc, function.section);
    proc.push(0xC0); // noinline | optimized debug info
    proc.extend_from_slice(function.name.as_bytes());
    proc.push(0);
    symbol_record(&mut symbols, S_GPROC32, &proc);

    let mut frame = Vec::new();
    // cbFrame: real stack-frame size captured from Cranelift's finalized
    // layout (explicit allocas + spill slots).
    push_u32(&mut frame, function.frame_size);
    push_u32(&mut frame, 0); // padding size
    push_u32(&mut frame, 0); // padding offset
    push_u32(&mut frame, 0); // reserved
    push_u32(&mut frame, 0); // exception handler
    push_u32(&mut frame, 0); // handler data
    push_u32(&mut frame, 0x0000_0140); // uses RSP as frame/parameter base
    symbol_record(&mut symbols, 0x1012, &frame); // S_FRAMEPROC

    for (local_index, local_name) in function.locals.iter().enumerate() {
        let mut local = Vec::new();
        let type_index = local_type_indices
            .get(local_index)
            .copied()
            .unwrap_or(T_UNKNOWN);
        push_u32(&mut local, type_index);
        push_u16(&mut local, 0);
        local.extend_from_slice(local_name.as_bytes());
        local.push(0);
        symbol_record(&mut symbols, S_LOCAL, &local);

        for range in local_location_ranges(function, local_index) {
            let start = range.start.min(function.size);
            let end = range.end.min(function.size);
            let length = end.saturating_sub(start).min(u16::MAX as u32) as u16;
            if length == 0 {
                continue;
            }
            let mut defrange = Vec::new();
            match range.location {
                NativeValueLocation::CfaOffset(offset) => {
                    push_u32(&mut defrange, offset as u32);
                    push_u32(&mut defrange, start);
                    push_u16(&mut defrange, length);
                    push_u16(&mut defrange, 0); // no gaps
                    symbol_record(&mut symbols, S_DEFRANGE_FRAMEPOINTER_REL, &defrange);
                }
                NativeValueLocation::Register(hw_enc) => {
                    let Some(register) = codeview_x64_register(hw_enc) else {
                        continue;
                    };
                    push_u16(&mut defrange, register);
                    push_u16(&mut defrange, 0); // mayHaveNoName
                    push_u32(&mut defrange, start);
                    push_u16(&mut defrange, function.section);
                    push_u16(&mut defrange, length);
                    symbol_record(&mut symbols, S_DEFRANGE_REGISTER, &defrange);
                }
            }
        }
    }
    symbol_record(&mut symbols, S_END, &[]);
    symbols
}

/// The records use relocatable ranges beginning at section offset zero.  The
/// linker resolves the section/range in the final image.  The function records
/// are compiler-owned and are never reconstructed from the JSON sidecar.
pub fn codeview_section(source_file: &str, functions: &[String], source: &str) -> Vec<u8> {
    let ranges = functions
        .iter()
        .map(|name| CodeViewFunction {
            name: name.clone(),
            offset: 0,
            size: 1,
            section: 1,
            locals: vec!["debug_value".to_string()],
            local_offsets: vec![None],
            local_locations: vec![Vec::new()],
            frame_size: 0,
            local_types: Vec::new(),
            return_type: None,
            line_rows: Vec::new(),
        })
        .collect::<Vec<_>>();
    codeview_section_with_ranges(source_file, &ranges, source)
}

/// Full native CodeView emission for one object.
///
/// Returns the C13 `.debug$S` stream and — when any function carries real
/// type information — the matching `.debug$T` type stream. The two sections
/// must be attached to the same COFF object so `S_LOCAL`/`S_GPROC32` type
/// indices resolve. With no typed locals only `.debug$S` is produced, which
/// is byte-identical to the historical primitives-only output.
pub fn codeview_sections(
    source_file: &str,
    functions: &[CodeViewFunction],
    source: &str,
) -> Vec<(&'static str, Vec<u8>)> {
    let mut type_table = CodeViewTypeTable::new();
    for function in functions {
        if let Some(return_type) = &function.return_type {
            type_table.index_for(return_type);
        }
        for local_type in &function.local_types {
            type_table.index_for(local_type);
        }
    }
    let mut sections = vec![(
        ".debug$S",
        codeview_debug_s(source_file, functions, source, &mut type_table),
    )];
    if type_table.has_user_types() {
        sections.push((".debug$T", type_table.finish()));
    }
    sections
}

fn codeview_debug_s(
    source_file: &str,
    functions: &[CodeViewFunction],
    source: &str,
    type_table: &mut CodeViewTypeTable,
) -> Vec<u8> {
    // The checksum is the real MD5 of the source text. The source path is
    // always resolvable by the AOT attach step (the CLI reads the file
    // before calling this function), so a real checksum is emitted instead
    // of the historical all-zero placeholder.
    let mut checksums = Vec::new();
    push_u32(&mut checksums, 0); // offset of source_file in the string table
    checksums.push(16); // checksum size
    checksums.push(0); // MD5 checksum kind
    checksums.extend_from_slice(&md5(source.as_bytes()));
    while checksums.len() % 4 != 0 {
        checksums.push(0);
    }

    let source_line_count = source.lines().count().max(1) as u32;
    let mut result = Vec::new();
    // C13 streams start with the version signature 4.
    push_u32(&mut result, 4);
    for function in functions {
        let mut symbols = Vec::new();
        let mut objname = Vec::new();
        push_u32(&mut objname, 0);
        objname.extend_from_slice(b"spectralang");
        objname.push(0);
        symbol_record(&mut symbols, S_OBJNAME, &objname);
        let proc_type_index = function
            .return_type
            .as_ref()
            .map(|ty| type_table.index_for(ty))
            .unwrap_or(T_UNKNOWN);
        let local_type_indices = (0..function.locals.len())
            .map(|index| {
                function
                    .local_types
                    .get(index)
                    .map(|ty| type_table.index_for(ty))
                    .unwrap_or(T_UNKNOWN)
            })
            .collect::<Vec<_>>();
        symbols.extend_from_slice(&function_symbols(
            function,
            proc_type_index,
            &local_type_indices,
        ));
        subsection(&mut result, DEBUG_S_SYMBOLS, &symbols);
    }
    for function in functions {
        let rows = line_table_rows(function.size, source_line_count, &function.line_rows);
        let mut lines = Vec::new();
        push_u32(&mut lines, function.offset);
        push_u16(&mut lines, function.section);
        push_u16(&mut lines, 0);
        push_u32(&mut lines, function.size.max(1));
        push_u32(&mut lines, 0); // file checksum record offset
        push_u32(&mut lines, 12 + rows.len() as u32 * 8);
        for (relative, line) in &rows {
            push_u32(&mut lines, *relative);
            push_u32(&mut lines, line & 0x00FF_FFFF);
        }
        if source.is_empty() {
            push_u32(&mut lines, 0);
            push_u32(&mut lines, 1);
        }
        subsection(&mut result, DEBUG_S_LINES, &lines);
    }
    let mut strings = source_file.as_bytes().to_vec();
    strings.push(0);
    subsection(&mut result, DEBUG_S_STRINGTABLE, &strings);
    subsection(&mut result, DEBUG_S_FILECHKSMS, &checksums);
    // Keep the source file name in the stream even before the linker merges
    // the C13 string table. This also makes independent structural auditing
    // possible without consulting the sidecar.
    result
}

/// Backward-compatible single-section variant of [`codeview_sections`]:
/// returns only the C13 `.debug$S` stream bytes.
pub fn codeview_section_with_ranges(
    source_file: &str,
    functions: &[CodeViewFunction],
    source: &str,
) -> Vec<u8> {
    codeview_sections(source_file, functions, source)
        .into_iter()
        .find(|(name, _)| *name == ".debug$S")
        .map(|(_, bytes)| bytes)
        .expect("codeview_sections always emits .debug$S")
}

/// Read function symbols from a COFF object without relying on a platform
/// debugger or linker.  Public symbols are the only authoritative ranges
/// available before linking; unknown symbols are ignored rather than given a
/// synthetic range.
pub fn coff_function_ranges(object: &[u8]) -> Vec<CodeViewFunction> {
    if object.len() < 20 || &object[0..2] == b"MZ" {
        return Vec::new();
    }
    let section_count = u16::from_le_bytes([object[2], object[3]]) as usize;
    let symbol_offset = u32::from_le_bytes([object[8], object[9], object[10], object[11]]) as usize;
    let symbol_count =
        u32::from_le_bytes([object[12], object[13], object[14], object[15]]) as usize;
    if symbol_offset == 0 || symbol_count == 0 || symbol_offset + symbol_count * 18 > object.len() {
        return Vec::new();
    }
    let string_base = symbol_offset + symbol_count * 18;
    if string_base + 4 > object.len() {
        return Vec::new();
    }
    let strings = &object[string_base + 4..];
    let mut section_sizes = vec![0u32; section_count + 1];
    for index in 0..section_count {
        let header = 20 + index * 40;
        if header + 40 > object.len() {
            return Vec::new();
        }
        section_sizes[index + 1] = u32::from_le_bytes([
            object[header + 16],
            object[header + 17],
            object[header + 18],
            object[header + 19],
        ]);
    }
    let mut result = Vec::new();
    for index in 0..symbol_count {
        let entry = symbol_offset + index * 18;
        let name_bytes = &object[entry..entry + 8];
        let value = u32::from_le_bytes([
            object[entry + 8],
            object[entry + 9],
            object[entry + 10],
            object[entry + 11],
        ]);
        let section = u16::from_le_bytes([object[entry + 12], object[entry + 13]]);
        let storage = object[entry + 16];
        if storage != 2 || section == 0 || section as usize > section_count {
            continue;
        }
        let name = if name_bytes[..4] == [0, 0, 0, 0] {
            let offset =
                u32::from_le_bytes([name_bytes[4], name_bytes[5], name_bytes[6], name_bytes[7]])
                    as usize;
            if offset < 4 || offset - 4 >= strings.len() {
                continue;
            }
            &strings[offset - 4..]
        } else {
            name_bytes
        };
        let name = name.split(|byte| *byte == 0).next().unwrap_or_default();
        let Ok(name) = std::str::from_utf8(name) else {
            continue;
        };
        if name.is_empty() || name.starts_with('.') {
            continue;
        }
        result.push(CodeViewFunction {
            name: name.to_string(),
            offset: value,
            size: section_sizes[section as usize].saturating_sub(value).max(1),
            section,
            locals: Vec::new(),
            local_offsets: Vec::new(),
            local_locations: Vec::new(),
            frame_size: 0,
            local_types: Vec::new(),
            return_type: None,
            line_rows: Vec::new(),
        });
    }
    // COFF function symbols carry starts but not sizes.  The next symbol in
    // the same section is the authoritative end of the current range; only
    // the final symbol uses the section end.  This avoids assigning the whole
    // text section to every procedure.
    result.sort_by_key(|function| (function.section, function.offset));
    for index in 0..result.len() {
        if let Some(next) = result.get(index + 1) {
            if next.section == result[index].section && next.offset > result[index].offset {
                result[index].size = next.offset - result[index].offset;
            }
        }
    }
    result
}

/// Cross-format symbol ranges used by DWARF emission.  `object` resolves ELF,
/// Mach-O and COFF symbol tables independently of the linker.
pub fn native_function_ranges(bytes: &[u8]) -> Vec<CodeViewFunction> {
    use object::{Object, ObjectSymbol, SymbolKind, SymbolSection};
    let Ok(file) = object::File::parse(bytes) else {
        return Vec::new();
    };
    let mut functions = file
        .symbols()
        .filter(|symbol| {
            symbol.is_definition()
                && symbol.kind() == SymbolKind::Text
                && !symbol.name().unwrap_or_default().starts_with('.')
        })
        .filter_map(|symbol| {
            let name = symbol.name().ok()?.to_string();
            let section = match symbol.section() {
                SymbolSection::Section(index) => index.0.min(u16::MAX as usize) as u16,
                _ => return None,
            };
            Some(CodeViewFunction {
                name,
                offset: symbol.address().min(u32::MAX as u64) as u32,
                size: symbol.size().min(u32::MAX as u64) as u32,
                section,
                locals: Vec::new(),
                local_offsets: Vec::new(),
                local_locations: Vec::new(),
                frame_size: 0,
                local_types: Vec::new(),
                return_type: None,
                line_rows: Vec::new(),
            })
        })
        .collect::<Vec<_>>();
    functions.sort_by_key(|function| (function.section, function.offset));
    functions
}

/// Append a debug section to a COFF object while preserving the existing
/// sections, relocations, symbols and string table.  This is intentionally a
/// container rewrite only; the CodeView bytes are produced by this module.
pub fn append_coff_section(
    object: &[u8],
    name: &str,
    data: &[u8],
    characteristics: u32,
) -> Result<Vec<u8>, String> {
    if object.len() < 20 || &object[0..2] == b"MZ" {
        return Err("truncated or non-COFF object".to_string());
    }
    let section_count = u16::from_le_bytes([object[2], object[3]]) as usize;
    let old_table_end = 20usize
        .checked_add(
            section_count
                .checked_mul(40)
                .ok_or("COFF section table overflow")?,
        )
        .ok_or("COFF section table overflow")?;
    if old_table_end > object.len() || name.len() > 8 {
        return Err("invalid COFF section table or section name".to_string());
    }
    let symbol_ptr = u32::from_le_bytes([object[8], object[9], object[10], object[11]]) as usize;
    let pointer_fields = old_table_end;
    let mut insert_at = object.len();
    for index in 0..section_count {
        let header = 20 + index * 40;
        for field in [20usize, 24usize, 28usize] {
            let pointer = u32::from_le_bytes([
                object[header + field],
                object[header + field + 1],
                object[header + field + 2],
                object[header + field + 3],
            ]) as usize;
            if pointer != 0 {
                insert_at = insert_at.min(pointer);
            }
        }
    }
    if symbol_ptr != 0 {
        insert_at = insert_at.min(symbol_ptr);
    }
    if insert_at < pointer_fields {
        return Err("COFF data begins inside the section table".to_string());
    }
    let mut rewritten = Vec::with_capacity(object.len() + 40 + data.len());
    rewritten.extend_from_slice(&object[..insert_at]);
    rewritten.resize(rewritten.len() + 40, 0);
    rewritten.extend_from_slice(&object[insert_at..]);
    let shift = 40u32;
    let adjust = |value: u32| {
        if value == 0 {
            Ok(0)
        } else {
            value.checked_add(shift).ok_or("COFF pointer overflow")
        }
    };
    for index in 0..section_count {
        let old_header = 20 + index * 40;
        let header = old_header;
        for field in [20usize, 24usize, 28usize] {
            let value = u32::from_le_bytes([
                object[old_header + field],
                object[old_header + field + 1],
                object[old_header + field + 2],
                object[old_header + field + 3],
            ]) as usize;
            let adjusted = if value != 0 && value >= insert_at {
                value.checked_add(40).ok_or("COFF pointer overflow")?
            } else {
                value
            } as u32;
            rewritten[header + field..header + field + 4].copy_from_slice(&adjusted.to_le_bytes());
        }
    }
    if symbol_ptr != 0 {
        let adjusted = adjust(symbol_ptr as u32)?;
        rewritten[8..12].copy_from_slice(&adjusted.to_le_bytes());
    }
    rewritten[2..4].copy_from_slice(&((section_count + 1) as u16).to_le_bytes());
    let new_header = 20 + section_count * 40;
    rewritten[new_header..new_header + name.len()].copy_from_slice(name.as_bytes());
    let data_offset = rewritten.len() as u32;
    rewritten[new_header + 16..new_header + 20].copy_from_slice(&(data.len() as u32).to_le_bytes());
    rewritten[new_header + 20..new_header + 24].copy_from_slice(&data_offset.to_le_bytes());
    rewritten[new_header + 36..new_header + 40].copy_from_slice(&characteristics.to_le_bytes());
    rewritten.extend_from_slice(data);
    Ok(rewritten)
}

#[cfg(test)]
mod tests {
    use super::codeview_section;
    use super::{
        codeview_primitive_index, codeview_sections, CodeViewFunction, CodeViewTypeTable,
        DEBUG_S_SYMBOLS,
    };
    use spectra_midend::ir::{FloatWidth, IntWidth, Type};

    fn function_with_types() -> CodeViewFunction {
        CodeViewFunction {
            name: "typed".to_string(),
            offset: 0,
            size: 32,
            section: 1,
            locals: vec![
                "ratio".to_string(),
                "label".to_string(),
                "count".to_string(),
            ],
            local_offsets: vec![Some(-8), Some(-16), None],
            local_locations: vec![Vec::new(); 3],
            frame_size: 24,
            local_types: vec![
                Type::ExactFloat {
                    width: FloatWidth::F64,
                },
                Type::String,
                Type::ExactInt {
                    signed: true,
                    width: IntWidth::I32,
                },
            ],
            return_type: Some(Type::Int),
            line_rows: Vec::new(),
        }
    }

    /// Walk the C13 stream and return every symbol record as `(kind, payload)`
    /// across all `DEBUG_S_SYMBOLS` subsections.
    fn c13_symbol_records(bytes: &[u8]) -> Vec<(u16, Vec<u8>)> {
        let mut records = Vec::new();
        let mut cursor = 4usize; // C13 version signature
        while cursor + 8 <= bytes.len() {
            let kind = u32::from_le_bytes([
                bytes[cursor],
                bytes[cursor + 1],
                bytes[cursor + 2],
                bytes[cursor + 3],
            ]);

            let length = u32::from_le_bytes([
                bytes[cursor + 4],
                bytes[cursor + 5],
                bytes[cursor + 6],
                bytes[cursor + 7],
            ]) as usize;
            let payload_start = cursor + 8;
            if payload_start + length <= bytes.len() {
                let payload = &bytes[payload_start..payload_start + length];
                if kind == DEBUG_S_SYMBOLS {
                    let mut inner = 0usize;
                    while inner + 4 <= payload.len() {
                        let record_len =
                            u16::from_le_bytes([payload[inner], payload[inner + 1]]) as usize;
                        // The recorded length excludes the 2-byte length
                        // field itself.
                        let total = record_len + 2;
                        if record_len < 2 || inner + total > payload.len() {
                            break;
                        }
                        let kind = u16::from_le_bytes([payload[inner + 2], payload[inner + 3]]);
                        records.push((kind, payload[inner + 4..inner + total].to_vec()));
                        // Symbol records are 4-byte aligned within the stream.
                        inner = (inner + total).div_ceil(4) * 4;
                    }
                }
            }
            cursor = payload_start + length;
            cursor = cursor.div_ceil(4) * 4;
        }
        records
    }
    /// Walk the `.debug$T` stream (after its version signature) and return
    /// every type record as `(leaf id, payload)`.
    fn debug_t_records(bytes: &[u8]) -> Vec<(u16, Vec<u8>)> {
        let mut records = Vec::new();
        let mut cursor = 4usize; // version signature
        while cursor + 4 <= bytes.len() {
            let length = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
            if length < 2 || cursor + 2 + length > bytes.len() {
                break;
            }
            let leaf = u16::from_le_bytes([bytes[cursor + 2], bytes[cursor + 3]]);
            records.push((leaf, bytes[cursor + 4..cursor + 2 + length].to_vec()));
            cursor = (cursor + 2 + length).div_ceil(4) * 4;
        }
        records
    }

    fn point_struct_ir() -> Type {
        Type::Struct {
            name: "Point".to_string(),
            fields: vec![
                ("x".to_string(), Type::Int),
                ("y".to_string(), Type::Float),
                (
                    "z".to_string(),
                    Type::ExactInt {
                        signed: true,
                        width: IntWidth::I32,
                    },
                ),
            ],
        }
    }

    #[test]
    fn struct_with_fields_emits_full_codeview_udt_definition() {
        // Replace one local's type with the 3-field Point struct.
        let mut function = function_with_types();
        function.locals = vec!["origin".to_string()];
        function.local_offsets = vec![Some(-8)];
        function.local_locations = vec![Vec::new(); 1];
        function.local_types = vec![point_struct_ir()];
        let sections = codeview_sections("fixture.spectra", &[function], "fn typed() {}");
        let type_stream = &sections
            .iter()
            .find(|(name, _)| *name == ".debug$T")
            .expect("struct local must produce a .debug$T stream")
            .1;

        let records = debug_t_records(type_stream);

        // Defining LF_STRUCTURE named "Point": 3 members, defined properties,
        // real field-list back-reference, padded size 8+8+4 -> 24.
        let point_structure = records
            .iter()
            .find(|(leaf, payload)| *leaf == super::LF_STRUCTURE && payload.ends_with(b"Point\0"))
            .map(|(_, payload)| payload)
            .expect("defining LF_STRUCTURE for Point must be present");
        assert_eq!(
            u16::from_le_bytes([point_structure[0], point_structure[1]]),
            3
        );
        assert_eq!(point_structure[2] & super::CV_IS_FWDREF, 0);
        let fieldlist_index = u32::from_le_bytes([
            point_structure[4],
            point_structure[5],
            point_structure[6],
            point_structure[7],
        ]);
        assert_ne!(fieldlist_index, 0);
        let size = u64::from_le_bytes(point_structure[16..24].try_into().unwrap());
        assert_eq!(size, 24);

        // The referenced LF_FIELDLIST carries one LF_MEMBER per field with
        // mid-end layout offsets 0/8/16 and stored-representation types.
        let fieldlist = records
            .iter()
            .find(|(leaf, _)| *leaf == super::LF_FIELDLIST)
            .map(|(_, payload)| payload)
            .expect("LF_FIELDLIST must be present");
        const EXPECTED: [(&str, u32, u16); 3] = [
            ("x", super::T_INT8, 0),
            ("y", super::T_REAL64, 8),
            ("z", super::T_INT4, 16),
        ];
        let mut members = Vec::new();
        let mut cursor = 0usize;
        while cursor + 4 <= fieldlist.len() {
            let length = u16::from_le_bytes([fieldlist[cursor], fieldlist[cursor + 1]]) as usize;
            if length < 2 || cursor + 2 + length > fieldlist.len() {
                break;
            }
            let leaf = u16::from_le_bytes([fieldlist[cursor + 2], fieldlist[cursor + 3]]);
            let payload = fieldlist[cursor + 4..cursor + 2 + length].to_vec();
            cursor = (cursor + 2 + length).div_ceil(4) * 4;
            if leaf != super::LF_MEMBER {
                continue;
            }
            let name = String::from_utf8_lossy(&payload[10..])
                .trim_end_matches('\0')
                .to_string();
            let member_type = u32::from_le_bytes(payload[4..8].try_into().unwrap());
            let offset = u16::from_le_bytes([payload[8], payload[9]]);
            members.push((name, member_type, offset));
        }
        assert_eq!(
            members,
            EXPECTED
                .iter()
                .map(|(name, ty, off)| ((*name).to_string(), *ty, *off))
                .collect::<Vec<_>>()
        );
        // Member attributes: public access.
        assert_eq!(u16::from_le_bytes([fieldlist[4], fieldlist[5]]), 3);

        // The `.debug$S` S_LOCAL for `origin` references the pointer record
        // that points at the defining structure.
        let symbol_records = c13_symbol_records(
            sections
                .iter()
                .find(|(name, _)| *name == ".debug$S")
                .map(|(_, bytes)| bytes)
                .unwrap(),
        );
        const S_LOCAL_KIND: u16 = 0x113E;
        let origin_index = symbol_records
            .iter()
            .filter(|(kind, _)| *kind == S_LOCAL_KIND)
            .find(|(_, payload)| payload[6..].starts_with(b"origin"))
            .map(|(_, payload)| {
                u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]])
            })
            .expect("S_LOCAL for origin must exist");
        assert!(origin_index >= super::FIRST_USER_TYPE_INDEX);
        let pointer_payload = records
            .iter()
            .find(|(leaf, _)| *leaf == super::LF_POINTER)
            .map(|(_, payload)| payload)
            .expect("pointer record for the aggregate must exist");
        assert_eq!(
            u32::from_le_bytes(pointer_payload[..4].try_into().unwrap()),
            fieldlist_index.wrapping_add(1),
            "the pointer must target the defining LF_STRUCTURE"
        );
    }

    #[test]
    fn jit_debug_sidecar_json_is_deterministic_and_escaped() {
        let functions = vec![
            (
                "b_fn".to_string(),
                vec![super::JitDebugVariable {
                    name: "va\"l".to_string(),
                    type_name: "int".to_string(),
                    line: Some(7),
                    ranges: vec![(0, 12), (20, 48)],
                }],
            ),
            ("a_fn".to_string(), Vec::new()),
        ];
        let json = super::jit_debug_sidecar_json(&functions);
        assert_eq!(
            json,
            "{\"a_fn\":[],\"b_fn\":[{\"name\":\"va\\\"l\",\"type\":\"int\",\"line\":7,\"ranges\":[[0,12],[20,48]]}]}"
        );
    }

    #[test]
    fn typed_locals_emit_distinct_codeview_type_indices() {
        let sections =
            codeview_sections("fixture.spectra", &[function_with_types()], "fn typed() {}");
        assert!(
            sections.iter().any(|(name, _)| *name == ".debug$S"),
            "C13 symbols stream must always be present"
        );
        let type_stream = &sections
            .iter()
            .find(|(name, _)| *name == ".debug$T")
            .expect("typed functions must produce a .debug$T stream")
            .1;
        assert!(type_stream.windows(15).any(|w| w == b"spectra_string\0"));

        let records = c13_symbol_records(
            sections
                .iter()
                .find(|(name, _)| *name == ".debug$S")
                .map(|(_, bytes)| bytes)
                .unwrap(),
        );
        const S_LOCAL_KIND: u16 = 0x113E;
        let mut locals = records
            .iter()
            .filter(|(kind, _)| *kind == S_LOCAL_KIND)
            .map(|(_, payload)| {
                (
                    String::from_utf8_lossy(&payload[6..])
                        .trim_end_matches('\0')
                        .to_string(),
                    u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]),
                )
            })
            .collect::<Vec<_>>();
        locals.sort();
        // float local → T_REAL64; string local → pointer into .debug$T (not a
        // simple index); i32 local → T_INT4. Three distinct indices, so the
        // output is no longer "everything is T_INT4".
        assert_eq!(locals[0], ("count".to_string(), super::T_INT4));
        assert_eq!(locals[1], ("label".to_string(), 0x1001));
        assert!(locals[1].1 >= 0x1000);
        assert_eq!(locals[2], ("ratio".to_string(), super::T_REAL64));
        let distinct = locals
            .iter()
            .map(|(_, ti)| *ti)
            .collect::<std::collections::HashSet<_>>();
        assert!(distinct.len() >= 3);
    }

    #[test]
    fn frame_size_is_emitted_in_s_frameproc() {
        let sections =
            codeview_sections("fixture.spectra", &[function_with_types()], "fn typed() {}");
        let records = c13_symbol_records(&sections[0].1);
        const S_FRAMEPROC: u16 = 0x1012;
        let frame = records
            .iter()
            .find(|(kind, _)| *kind == S_FRAMEPROC)
            .expect("procedure must carry an S_FRAMEPROC record");
        let cb_frame = u32::from_le_bytes([frame.1[0], frame.1[1], frame.1[2], frame.1[3]]);
        assert_eq!(cb_frame, 24);
    }

    #[test]
    fn primitive_ir_types_map_to_fixed_codeview_indices() {
        assert_eq!(codeview_primitive_index(&Type::Int), Some(super::T_INT8));
        assert_eq!(
            codeview_primitive_index(&Type::Float),
            Some(super::T_REAL64)
        );
        assert_eq!(
            codeview_primitive_index(&Type::ExactFloat {
                width: FloatWidth::F32
            }),
            Some(super::T_REAL32)
        );
        assert_eq!(
            codeview_primitive_index(&Type::ExactInt {
                signed: false,
                width: IntWidth::Usize
            }),
            Some(super::T_UINT8)
        );
        assert_eq!(codeview_primitive_index(&Type::Bool), Some(super::T_BOOL08));
        assert_eq!(codeview_primitive_index(&Type::Char), Some(super::T_CHAR));
        assert_eq!(codeview_primitive_index(&Type::String), None);
    }

    #[test]
    fn type_table_interns_aggregates_once_and_keeps_primitives_simple() {
        let mut table = CodeViewTypeTable::new();
        assert_eq!(table.index_for(&Type::Int), super::T_INT8);
        assert!(!table.has_user_types());
        let first = table.index_for(&Type::String);
        let second = table.index_for(&Type::String);
        assert_eq!(first, second);
        assert!(first >= 0x1000);
        assert!(table.has_user_types());
        let pointer = table.index_for(&Type::Pointer(Box::new(Type::Int)));
        assert_ne!(pointer, first);
        let finished = table.finish();
        assert_eq!(&finished[..4], &4u32.to_le_bytes());
    }

    #[test]
    fn untyped_functions_still_emit_without_debug_t() {
        let mut f = function_with_types();
        f.local_types.clear();
        f.return_type = None;
        let sections = codeview_sections("fixture.spectra", &[f], "fn typed() {}");
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].0, ".debug$S");
    }

    #[test]
    fn emits_non_empty_c13_records_with_function_and_local_names() {
        let bytes = codeview_section("fixture.spectra", &["main".into()], "fn main() {}");
        assert!(bytes.windows(4).any(|w| w == b"main"));
        assert!(bytes.windows(11).any(|w| w == b"debug_value"));
        assert!(bytes.len() > 32);
    }

    #[test]
    fn line_table_rows_returns_empty_without_real_rows() {
        assert!(super::line_table_rows(32, 5, &[]).is_empty());
        assert!(super::line_table_rows(1, 1, &[]).is_empty());
    }

    #[test]
    fn line_table_rows_emits_only_real_rows() {
        // Real, compiler-proven rows at offsets 4 and 20.
        let real = [(4u32, 7u32), (20, 12)];
        let rows = super::line_table_rows(32, 5, &real);
        assert_eq!(rows, vec![(4, 7), (20, 12)]);
    }

    #[test]
    fn line_table_rows_drops_real_rows_outside_the_function_range() {
        let real = [(100u32, 3u32)];
        assert!(super::line_table_rows(16, 2, &real).is_empty());
    }

    #[test]
    fn md5_matches_reference_digests() {
        assert_eq!(hex(&super::md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&super::md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
