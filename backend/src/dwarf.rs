//! DWARF v4 emission for Unix object targets.
//!
//! This module deliberately emits DWARF through `gimli` rather than writing
//! section bytes by hand.  Addresses are supplied by the object symbol table
//! reconciliation step, so the generated DIE ranges are tied to real
//! functions.  The CLI remains responsible for inserting the resulting
//! sections into the object container and for validating relocations on the
//! target platform.

use gimli::write::{
    Address, AttributeValue, Dwarf, DwarfUnit, EndianVec, Expression, LineProgram, LineString,
    Range, RangeList, Sections,
};
use gimli::{constants, Encoding, Format, LineEncoding, LittleEndian};

use crate::aot::NativeValueLocation;
use crate::debug::CodeViewFunction;
use std::collections::HashMap;

use spectra_midend::ir::{FloatWidth, IntWidth, Type as IRType};

pub type DwarfSection = (String, Vec<u8>);

pub fn sections_for_functions(
    source_file: &str,
    source: &str,
    functions: &[CodeViewFunction],
) -> Result<Vec<DwarfSection>, String> {
    let encoding = Encoding {
        format: Format::Dwarf32,
        version: 4,
        address_size: 8,
    };
    let file_name = std::path::Path::new(source_file)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(source_file);
    let mut line_program = LineProgram::new(
        encoding,
        LineEncoding::default(),
        LineString::String(Vec::new()),
        None,
        LineString::String(file_name.as_bytes().to_vec()),
        None,
    );
    let directory = line_program.default_directory();
    let file = line_program.add_file(
        LineString::String(file_name.as_bytes().to_vec()),
        directory,
        None,
    );
    let source_line_count = source.lines().count().max(1) as u32;
    for function in functions {
        line_program.begin_sequence(Some(Address::Constant(function.offset as u64)));
        // Real, span-derived rows when codegen captured them; the uniform
        // heuristic remains the documented fallback otherwise (see
        // `debug::line_table_rows`).
        for (relative, line) in crate::debug::line_table_rows(
            function.size,
            source_line_count,
            &function.line_rows,
        ) {
            line_program.set_address(Address::Constant(
                function.offset as u64 + relative as u64,
            ));
            line_program.row().file = file;
            line_program.row().line = line as u64;
            line_program.generate_row();
        }
        line_program.end_sequence(function.offset as u64 + function.size.max(1) as u64);
    }

    let mut dwarf = DwarfUnit::new(encoding);
    dwarf.unit.line_program = line_program;
    let root = dwarf.unit.root();
    dwarf.unit.get_mut(root).set(
        constants::DW_AT_name,
        AttributeValue::String(file_name.as_bytes().to_vec()),
    );
    dwarf.unit.get_mut(root).set(
        constants::DW_AT_comp_dir,
        AttributeValue::String(Vec::new()),
    );
    dwarf.unit.get_mut(root).set(
        constants::DW_AT_producer,
        AttributeValue::String(b"SpectraLang".to_vec()),
    );
    dwarf.unit.get_mut(root).set(
        constants::DW_AT_language,
        AttributeValue::Language(constants::DW_LANG_Rust),
    );
    dwarf
        .unit
        .get_mut(root)
        .set(constants::DW_AT_stmt_list, AttributeValue::LineProgramRef);
    // Interned type DIEs, shared across every function in the unit. The key
    // is the debug-format rendering of the IR type.
    let mut type_cache: HashMap<String, gimli::write::UnitEntryId> = HashMap::new();

    for function in functions {
        let subprogram = dwarf.unit.add(root, constants::DW_TAG_subprogram);
        dwarf.unit.get_mut(subprogram).set(
            constants::DW_AT_name,
            AttributeValue::String(function.name.as_bytes().to_vec()),
        );
        dwarf.unit.get_mut(subprogram).set(
            constants::DW_AT_low_pc,
            AttributeValue::Address(Address::Constant(function.offset as u64)),
        );
        dwarf.unit.get_mut(subprogram).set(
            constants::DW_AT_high_pc,
            AttributeValue::Address(Address::Constant(
                (function.offset + function.size.max(1)) as u64,
            )),
        );
        let ranges = dwarf.unit.ranges.add(RangeList(vec![Range::StartLength {
            begin: Address::Constant(function.offset as u64),
            length: function.size.max(1) as u64,
        }]));
        dwarf.unit.get_mut(subprogram).set(
            constants::DW_AT_ranges,
            AttributeValue::RangeListRef(ranges),
        );
        dwarf.unit.get_mut(subprogram).set(
            constants::DW_AT_decl_file,
            AttributeValue::FileIndex(Some(file)),
        );
        dwarf
            .unit
            .get_mut(subprogram)
            .set(constants::DW_AT_decl_line, AttributeValue::Udata(1));

        // The frame base is the canonical CFA of the call frame. Cranelift's
        // value-label pass reports local offsets as CFA-relative, so
        // `DW_OP_fbreg` expressions below resolve against this base exactly
        // like the CodeView `S_DEFRANGE_FRAMEPOINTER_REL` records do.
        let mut frame_base = Expression::new();
        frame_base.op(constants::DW_OP_call_frame_cfa);
        dwarf.unit.get_mut(subprogram).set(
            constants::DW_AT_frame_base,
            AttributeValue::Exprloc(frame_base),
        );
        if let Some(return_type) = &function.return_type {
            if !matches!(return_type, IRType::Void | IRType::Unknown) {
                let type_die = intern_type_die(&mut dwarf.unit, return_type, &mut type_cache);
                dwarf
                    .unit
                    .get_mut(subprogram)
                    .set(constants::DW_AT_type, AttributeValue::UnitRef(type_die));
            }
        }

        // A location is emitted only for a compiler-proven stack/register
        // mapping. Ranges that do not cover the complete function are left
        // absent until a DWARF location-list entry can represent their exact
        // lifetime; emitting a function-wide expression would be false.
        for (local_index, local_name) in function.locals.iter().enumerate() {
            let local = dwarf.unit.add(subprogram, constants::DW_TAG_variable);
            dwarf.unit.get_mut(local).set(
                constants::DW_AT_name,
                AttributeValue::String(local_name.as_bytes().to_vec()),
            );
            dwarf.unit.get_mut(local).set(
                constants::DW_AT_decl_file,
                AttributeValue::FileIndex(Some(file)),
            );
            dwarf
                .unit
                .get_mut(local)
                .set(constants::DW_AT_decl_line, AttributeValue::Udata(1));
            if let Some(local_type) = function.local_types.get(local_index) {
                if !matches!(local_type, IRType::Void | IRType::Unknown) {
                    let type_die = intern_type_die(&mut dwarf.unit, local_type, &mut type_cache);
                    dwarf
                        .unit
                        .get_mut(local)
                        .set(constants::DW_AT_type, AttributeValue::UnitRef(type_die));
                }
            }
            let complete_range = function
                .local_locations
                .get(local_index)
                .and_then(|ranges| {
                    ranges
                        .iter()
                        .find(|range| range.start == 0 && range.end >= function.size.max(1))
                });
            if let Some(range) = complete_range {
                let expression = match range.location {
                    NativeValueLocation::CfaOffset(offset) => Some(_location_expression(offset)),
                    NativeValueLocation::Register(hw_enc) => _register_expression(hw_enc),
                };
                if let Some(expression) = expression {
                    dwarf.unit.get_mut(local).set(
                        constants::DW_AT_location,
                        AttributeValue::Exprloc(expression),
                    );
                }
            } else if function
                .local_locations
                .get(local_index)
                .is_none_or(Vec::is_empty)
            {
                if let Some(Some(offset)) = function.local_offsets.get(local_index) {
                    dwarf.unit.get_mut(local).set(
                        constants::DW_AT_location,
                        AttributeValue::Exprloc(_location_expression(*offset)),
                    );
                }
            }
        }
    }

    let mut write_dwarf = Dwarf::new();
    write_dwarf.units.add(dwarf.unit);
    let mut sections = Sections::new(EndianVec::new(LittleEndian));
    write_dwarf
        .write(&mut sections)
        .map_err(|error| format!("DWARF write failed: {error:?}"))?;
    let mut output = Vec::new();
    sections
        .for_each(|id, data| {
            if !data.slice().is_empty() {
                output.push((id.name().to_string(), data.slice().to_vec()));
            }
            Ok::<(), ()>(())
        })
        .map_err(|error| format!("DWARF section extraction failed: {error:?}"))?;
    Ok(output)
}

#[allow(dead_code)]
fn _location_expression(offset: i64) -> Expression {
    let mut expression = Expression::new();
    expression.op_fbreg(offset);
    expression
}

fn _register_expression(hw_enc: u8) -> Option<Expression> {
    // Cranelift's x86-64 hardware order is RAX, RCX, RDX, RBX, RSP, RBP,
    // RSI, RDI, R8..R15. DWARF orders the first eight as RAX, RDX, RCX,
    // RBX, RSI, RDI, RBP, RSP.
    const DWARF_REGISTERS: [u16; 16] = [0, 2, 1, 3, 7, 6, 4, 5, 8, 9, 10, 11, 12, 13, 14, 15];
    let register = *DWARF_REGISTERS.get(hw_enc as usize)?;
    let mut expression = Expression::new();
    expression.op_reg(gimli::Register(register));
    Some(expression)
}


/// Create (or reuse) the DIE describing one IR type.
///
/// Primitive types become `DW_TAG_base_type` entries; aggregates become a
/// declaration-only `DW_TAG_structure_type` carrying the real type name behind
/// a `DW_TAG_pointer_type`, mirroring the backend's pointer representation and
/// the CodeView forward-reference UDT emission in `debug.rs`.
fn intern_type_die(
    unit: &mut gimli::write::Unit,
    ty: &IRType,
    cache: &mut HashMap<String, gimli::write::UnitEntryId>,
) -> gimli::write::UnitEntryId {
    let key = format!("{ty:?}");
    if let Some(&existing) = cache.get(&key) {
        return existing;
    }
    let id = match ty {
        IRType::Generic { representation, .. } => intern_type_die(unit, representation, cache),
        IRType::Pointer(inner) => {
            let pointee = intern_type_die(unit, inner, cache);
            let pointer = unit.add(unit.root(), constants::DW_TAG_pointer_type);
            unit
                .get_mut(pointer)
                .set(constants::DW_AT_type, AttributeValue::UnitRef(pointee));
            pointer
        }
        IRType::Array { element_type, .. } => {
            // Arrays lower to raw element pointers on this target.
            let element = intern_type_die(unit, element_type, cache);
            let pointer = unit.add(unit.root(), constants::DW_TAG_pointer_type);
            unit
                .get_mut(pointer)
                .set(constants::DW_AT_type, AttributeValue::UnitRef(element));
            pointer
        }
        primitive @ (IRType::Int
        | IRType::Float
        | IRType::ExactInt { .. }
        | IRType::ExactFloat { .. }
        | IRType::Bool
        | IRType::Char) => {
            let entry = unit.add(unit.root(), constants::DW_TAG_base_type);
            let (name, byte_size, encoding) = primitive_base_type(primitive);
            unit
                .get_mut(entry)
                .set(constants::DW_AT_name, AttributeValue::String(name));
            unit
                .get_mut(entry)
                .set(constants::DW_AT_byte_size, AttributeValue::Udata(byte_size));
            unit
                .get_mut(entry)
                .set(constants::DW_AT_encoding, AttributeValue::Encoding(encoding));
            entry
        }
        aggregate => {
            let name = crate::debug::aggregate_udt_name(aggregate);
            let structure = unit.add(unit.root(), constants::DW_TAG_structure_type);
            unit.get_mut(structure).set(
                constants::DW_AT_name,
                AttributeValue::String(name.into_bytes()),
            );
            // Declaration-only: full member layout is runtime-owned.
            unit
                .get_mut(structure)
                .set(constants::DW_AT_declaration, AttributeValue::Flag(true));
            let pointer = unit.add(unit.root(), constants::DW_TAG_pointer_type);
            unit
                .get_mut(pointer)
                .set(constants::DW_AT_type, AttributeValue::UnitRef(structure));
            pointer
        }
    };
    cache.insert(key, id);
    id
}


fn primitive_base_type(ty: &IRType) -> (Vec<u8>, u64, constants::DwAte) {
    match ty {
        IRType::Int => (b"int".to_vec(), 8, constants::DW_ATE_signed),
        IRType::Float => (b"double".to_vec(), 8, constants::DW_ATE_float),
        IRType::ExactInt { signed, width } => {
            let size = match width {
                IntWidth::I8 => 1u64,
                IntWidth::I16 => 2,
                IntWidth::I32 => 4,
                IntWidth::I64 | IntWidth::Isize | IntWidth::Usize => 8,
            };
            let name: &[u8] = match (*signed, width) {
                (true, IntWidth::I8) => b"i8",
                (true, IntWidth::I16) => b"i16",
                (true, IntWidth::I32) => b"i32",
                (true, IntWidth::I64) => b"i64",
                (true, IntWidth::Isize) => b"isize",
                (false, IntWidth::I8) => b"u8",
                (false, IntWidth::I16) => b"u16",
                (false, IntWidth::I32) => b"u32",
                (false, IntWidth::I64) => b"u64",
                (false, IntWidth::Usize) => b"usize",
                _ => b"isize",
            };
            (
                name.to_vec(),
                size,
                if *signed {
                    constants::DW_ATE_signed
                } else {
                    constants::DW_ATE_unsigned
                },
            )
        }
        IRType::ExactFloat { width } => match width {
            FloatWidth::F32 => (b"f32".to_vec(), 4, constants::DW_ATE_float),
            FloatWidth::F64 => (b"f64".to_vec(), 8, constants::DW_ATE_float),
        },
        IRType::Bool => (b"bool".to_vec(), 1, constants::DW_ATE_boolean),
        IRType::Char => (b"char".to_vec(), 4, constants::DW_ATE_UTF),
        _ => (b"unknown".to_vec(), 1, constants::DW_ATE_unsigned),
    }
}
#[cfg(test)]
mod tests {
    use super::sections_for_functions;
    use crate::debug::CodeViewFunction;
    use spectra_midend::ir::{FloatWidth, Type};

    #[test]
    fn emits_structural_dwarf_sections_for_real_ranges() {
        let functions = vec![CodeViewFunction {
            name: "helper".to_string(),
            offset: 16,
            size: 32,
            section: 1,
            locals: vec!["debug_value".to_string()],
            local_offsets: vec![Some(-8)],
            local_locations: vec![Vec::new()],
            frame_size: 0,
            local_types: Vec::new(),
            return_type: None,
            line_rows: Vec::new(),
        }];
        let sections = sections_for_functions("fixture.spectra", "fn helper() {}", &functions)
            .expect("DWARF writer should accept a valid unit");
        assert!(sections
            .iter()
            .any(|(name, data)| name == ".debug_info" && !data.is_empty()));
        assert!(sections
            .iter()
            .any(|(name, data)| name == ".debug_line" && !data.is_empty()));
        assert!(sections
            .iter()
            .any(|(name, data)| name == ".debug_abbrev" && !data.is_empty()));
    }

    #[test]
    fn typed_locals_produce_named_type_dies() {
        let functions = vec![CodeViewFunction {
            name: "typed".to_string(),
            offset: 0,
            size: 16,
            section: 1,
            locals: vec!["ratio".to_string(), "label".to_string()],
            local_offsets: vec![Some(-8), Some(-16)],
            local_locations: vec![Vec::new(); 2],
            frame_size: 16,
            local_types: vec![
                Type::ExactFloat { width: FloatWidth::F64 },
                Type::String,
            ],
            return_type: None,
            line_rows: Vec::new(),
        }];
        let sections = sections_for_functions("fixture.spectra", "fn typed() {}", &functions)
            .expect("DWARF writer should accept a valid unit");
        // The declaration-only structure DIE for the string local keeps the
        // real aggregate name and the base-type DIE keeps its primitive name.
        let debug_info = &sections
            .iter()
            .find(|(name, _)| name == ".debug_info")
            .expect("DWARF unit must emit .debug_info")
            .1;
        assert!(debug_info.windows(14).any(|w| w == b"spectra_string"));
        assert!(debug_info.windows(3).any(|w| w == b"f64"));
    }
}
