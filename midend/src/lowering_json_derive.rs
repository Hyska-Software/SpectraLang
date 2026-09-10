use super::*;
use spectra_compiler::ast::{Attribute, AttributeArgument, Enum as ASTEnum, Struct as ASTStruct};

/// One derived field as seen by JSON lowering.
///
/// The semantic pass owns derive validation (`JsonDerivedFieldInfo`); the
/// midend keeps this parallel mapping because the compiler-to-midend bridge
/// only carries lowered field types, not wire names or optionality. The
/// attribute grammar mirrored here is tiny (`derive(Serialize|Deserialize)`,
/// `#[json(rename = "..")]`, `#[json(optional)]`); anything the semantic
/// pass rejects never reaches lowering.
#[derive(Clone)]
pub(crate) struct JsonFieldSchema {
    pub source_name: String,
    pub json_name: String,
    pub optional: bool,
    pub field_type: IRType,
}

/// Inputs for decoding one JSON object field in `ASTLowering::lower_json_decode_field`.
struct JsonDecodeField<'a> {
    field: &'a JsonFieldSchema,
    field_type: &'a IRType,
    child: Value,
    path: &'a str,
    field_ptr: Value,
}

fn has_derive_flag(attributes: &[Attribute], flag: &str) -> bool {
    attributes.iter().any(|attr| {
        attr.name == "derive"
            && attr
                .arguments
                .iter()
                .any(|arg| matches!(arg, AttributeArgument::Name(name) if name == flag))
    })
}

fn json_name_and_optional(attributes: &[Attribute], source_name: &str) -> (String, bool) {
    let mut json_name = source_name.to_string();
    let mut optional = false;
    for attr in attributes {
        if attr.name != "json" {
            continue;
        }
        for arg in &attr.arguments {
            match arg {
                AttributeArgument::Name(name) if name == "optional" => optional = true,
                AttributeArgument::KeyValue { key, value } if key == "rename" => {
                    json_name = value.clone();
                }
                // Unknown options are semantic errors; compilation aborts
                // before lowering, so there is nothing to preserve here.
                _ => {}
            }
        }
    }
    (json_name, optional)
}

impl ASTLowering {
    /// Record the JSON schema for a struct definition. Called for imported
    /// and local structs at registration; generic structs are skipped (their
    /// instantiations carry no schema and reject derive calls explicitly).
    /// Structs without any derive flag leave no entry.
    pub(crate) fn register_json_derive_for_struct(&mut self, struct_def: &ASTStruct) {
        if !struct_def.type_params.is_empty() {
            return;
        }
        if !has_derive_flag(&struct_def.attributes, "Serialize")
            && !has_derive_flag(&struct_def.attributes, "Deserialize")
        {
            return;
        }
        let mut fields = Vec::with_capacity(struct_def.fields.len());
        for field in &struct_def.fields {
            let (json_name, optional) = json_name_and_optional(&field.attributes, &field.name);
            let field_type = self.lower_type_annotation(&field.ty);
            fields.push(JsonFieldSchema {
                source_name: field.name.clone(),
                json_name,
                optional,
                field_type,
            });
        }
        self.json_struct_schemas
            .insert(struct_def.name.clone(), fields);
    }

    /// Record variant wire names for a serializable enum, preserving tag
    /// order. Only `Serialize` matters: `from_json`/`json_error_field` on
    /// enums are already rejected by the semantic pass.
    pub(crate) fn register_json_derive_for_enum(&mut self, enum_def: &ASTEnum) {
        if !enum_def.type_params.is_empty() {
            return;
        }
        if !has_derive_flag(&enum_def.attributes, "Serialize") {
            return;
        }
        let mut variants = Vec::with_capacity(enum_def.variants.len());
        for variant in &enum_def.variants {
            let (json_name, _) = json_name_and_optional(&variant.attributes, &variant.name);
            variants.push((variant.name.clone(), json_name));
        }
        self.json_enum_schemas
            .insert(enum_def.name.clone(), variants);
    }

    fn json_concat(&mut self, ir_func: &mut IRFunction, lhs: Value, rhs: Value) -> Value {
        self.require_value(
            self.builder.build_typed_host_call(
                ir_func,
                "spectra.std.string.concat".to_string(),
                vec![lhs, rhs],
                IRType::String,
                true,
            ),
            "JSON derive string concatenation did not produce a value",
        )
    }

    fn json_host(
        &mut self,
        ir_func: &mut IRFunction,
        host: &str,
        args: Vec<Value>,
        return_type: IRType,
        what: &str,
    ) -> Value {
        self.require_value(
            self.builder
                .build_typed_host_call(ir_func, host.to_string(), args, return_type, true),
            format!("JSON derive host call '{host}' {what}"),
        )
    }

    fn json_quote(&mut self, ir_func: &mut IRFunction, text: &str) -> Value {
        let literal = self.lower_string_literal(text, ir_func);
        self.json_host(
            ir_func,
            "spectra.api.json.quote_string",
            vec![literal],
            IRType::String,
            "did not quote a field name",
        )
    }

    /// Encode one field value as JSON text. Aggregate (struct/array) values
    /// arrive as pointers and are consumed without loading; only scalars are
    /// loaded. Returns `None` for types with no real encoding so the caller
    /// can emit a typed error naming the field.
    fn lower_json_encode_value(
        &mut self,
        field_type: &IRType,
        field_ptr: Value,
        ir_func: &mut IRFunction,
        stack: &mut Vec<String>,
    ) -> Option<Value> {
        let representation = self.ir_type_representation(field_type).clone();
        match representation {
            IRType::Int | IRType::ExactInt { .. } => {
                let value = self
                    .builder
                    .build_load_typed(ir_func, field_ptr, representation);
                Some(self.json_host(
                    ir_func,
                    "spectra.std.convert.int_to_string",
                    vec![value],
                    IRType::String,
                    "did not convert an int field",
                ))
            }
            IRType::Float | IRType::ExactFloat { .. } => {
                let value = self
                    .builder
                    .build_load_typed(ir_func, field_ptr, representation);
                Some(self.json_host(
                    ir_func,
                    "spectra.api.json.encode_number",
                    vec![value],
                    IRType::String,
                    "did not encode a float field",
                ))
            }
            IRType::Bool => {
                let value = self
                    .builder
                    .build_load_typed(ir_func, field_ptr, IRType::Bool);
                Some(self.json_host(
                    ir_func,
                    "spectra.std.convert.bool_to_string",
                    vec![value],
                    IRType::String,
                    "did not convert a bool field",
                ))
            }
            IRType::String => {
                let value = self
                    .builder
                    .build_load_typed(ir_func, field_ptr, IRType::String);
                Some(self.json_host(
                    ir_func,
                    "spectra.api.json.quote_string",
                    vec![value],
                    IRType::String,
                    "did not quote a string field",
                ))
            }
            IRType::Char => {
                let value = self
                    .builder
                    .build_load_typed(ir_func, field_ptr, IRType::Char);
                Some(self.json_host(
                    ir_func,
                    "spectra.api.json.quote_char",
                    vec![value],
                    IRType::String,
                    "did not quote a char field",
                ))
            }
            IRType::Struct { name, .. } => {
                // Struct-typed fields store an 8-byte pointer (see
                // `stored_size`); the field slot is not the struct base.
                let nested_defs = self.struct_definitions.get(&name).cloned()?;
                let nested_ty = IRType::Struct {
                    name: name.clone(),
                    fields: nested_defs,
                };
                let nested_ptr = self.builder.build_load_typed(ir_func, field_ptr, nested_ty);
                Some(self.lower_derive_encode_struct(&name, nested_ptr, ir_func, stack))
            }
            // Array annotations erase to dynamic size (`size: 0`) before
            // lowering, so element counts are unknowable at compile time and
            // runtime iteration is inexpressible here. Report the field
            // instead of encoding a wrong `[]`.
            IRType::Array { .. } => None,
            _ => None,
        }
    }

    /// Encode a struct value (by pointer) as a JSON object string.
    pub(crate) fn lower_derive_encode_struct(
        &mut self,
        struct_name: &str,
        struct_ptr: Value,
        ir_func: &mut IRFunction,
        stack: &mut Vec<String>,
    ) -> Value {
        let Some(field_defs) = self.struct_definitions.get(struct_name).cloned() else {
            return self.invalid_value(format!("JSON to_json on unknown struct '{struct_name}'"));
        };
        let Some(schema) = self.json_struct_schemas.get(struct_name).cloned() else {
            return self.invalid_value(format!(
                "JSON to_json on struct '{struct_name}' without a derive schema"
            ));
        };
        // Recursive types would lower forever; aggregates are pointer-linked
        // so the type recursion never bottoms out at compile time.
        if stack.contains(&struct_name.to_string()) {
            return self.invalid_value(format!(
                "JSON derive on recursive struct '{struct_name}' is not supported"
            ));
        }
        stack.push(struct_name.to_string());
        let result = self.lower_derive_encode_struct_inner(
            struct_name,
            &field_defs,
            &schema,
            struct_ptr,
            ir_func,
            stack,
        );
        stack.pop();
        result
    }

    fn lower_derive_encode_struct_inner(
        &mut self,
        struct_name: &str,
        field_defs: &[(String, IRType)],
        schema: &[JsonFieldSchema],
        struct_ptr: Value,
        ir_func: &mut IRFunction,
        stack: &mut Vec<String>,
    ) -> Value {
        let layout = layout::layout_of(field_defs.iter().map(|(_, ty)| ty));
        let mut json = self.lower_string_literal("{", ir_func);
        for (idx, (source_name, field_type)) in field_defs.iter().enumerate() {
            let Some(field) = schema.iter().find(|f| &f.source_name == source_name) else {
                return self.invalid_value(format!(
                    "JSON derive schema for '{struct_name}' is missing field '{source_name}'"
                ));
            };
            let Some(offset) = layout.offsets.get(idx).copied() else {
                return self.invalid_value(format!(
                    "JSON derive layout for '{struct_name}' is missing field '{source_name}'"
                ));
            };
            let field_ptr = self
                .builder
                .build_field_ptr(ir_func, struct_ptr, offset as i64);
            let Some(encoded) = self.lower_json_encode_value(field_type, field_ptr, ir_func, stack)
            else {
                return self.invalid_value(format!(
                    "JSON to_json on '{struct_name}.{}' is not supported for field type {:?}",
                    field.source_name, field_type
                ));
            };
            let key = self.json_quote(ir_func, &field.json_name);
            let colon = self.lower_string_literal(":", ir_func);
            json = self.json_concat(ir_func, json, key);
            json = self.json_concat(ir_func, json, colon);
            json = self.json_concat(ir_func, json, encoded);
            if idx + 1 < field_defs.len() {
                let comma = self.lower_string_literal(",", ir_func);
                json = self.json_concat(ir_func, json, comma);
            }
        }
        let close = self.lower_string_literal("}", ir_func);
        self.json_concat(ir_func, json, close)
    }

    /// Decode a JSON object handle into a freshly allocated struct value.
    fn lower_derive_decode_object(
        &mut self,
        struct_name: &str,
        obj: Value,
        base_path: &str,
        ir_func: &mut IRFunction,
        stack: &mut Vec<String>,
    ) -> Value {
        let Some(field_defs) = self.struct_definitions.get(struct_name).cloned() else {
            return self.invalid_value(format!("JSON from_json on unknown struct '{struct_name}'"));
        };
        let Some(schema) = self.json_struct_schemas.get(struct_name).cloned() else {
            return self.invalid_value(format!(
                "JSON from_json on struct '{struct_name}' without a derive schema"
            ));
        };
        if stack.contains(&struct_name.to_string()) {
            return self.invalid_value(format!(
                "JSON derive on recursive struct '{struct_name}' is not supported"
            ));
        }
        stack.push(struct_name.to_string());
        let struct_type = IRType::Struct {
            name: struct_name.to_string(),
            fields: field_defs.clone(),
        };
        let struct_ptr = self.builder.build_alloca(ir_func, struct_type);
        let layout = layout::layout_of(field_defs.iter().map(|(_, ty)| ty));
        for (idx, (source_name, field_type)) in field_defs.iter().enumerate() {
            let Some(field) = schema.iter().find(|f| &f.source_name == source_name) else {
                return self.invalid_value(format!(
                    "JSON derive schema for '{struct_name}' is missing field '{source_name}'"
                ));
            };
            let Some(offset) = layout.offsets.get(idx).copied() else {
                return self.invalid_value(format!(
                    "JSON derive layout for '{struct_name}' is missing field '{source_name}'"
                ));
            };
            let field_ptr = self
                .builder
                .build_field_ptr(ir_func, struct_ptr, offset as i64);
            let path = if base_path.is_empty() {
                field.json_name.clone()
            } else {
                format!("{base_path}.{}", field.json_name)
            };
            let key = self.lower_string_literal(&field.json_name, ir_func);
            let child = self.json_host(
                ir_func,
                "spectra.api.json.value_get",
                vec![obj, key],
                IRType::Int,
                "did not look up a JSON field",
            );
            let decoded = self.lower_json_decode_field(
                JsonDecodeField {
                    field,
                    field_type,
                    child,
                    path: &path,
                    field_ptr,
                },
                ir_func,
                stack,
            );
            // stacking a secondary error.
            if decoded.is_none() {
                stack.pop();
                return struct_ptr;
            }
        }
        stack.pop();
        struct_ptr
    }

    /// Decode one child handle and store it at `field_ptr`. Returns `None`
    /// after recording a typed error for unsupported shapes.
    fn lower_json_decode_field(
        &mut self,
        decode: JsonDecodeField<'_>,
        ir_func: &mut IRFunction,
        stack: &mut Vec<String>,
    ) -> Option<()> {
        let JsonDecodeField {
            field,
            field_type,
            child,
            path,
            field_ptr,
        } = decode;
        if field.optional && !is_json_scalar(field_type) {
            self.error(format!(
                "JSON from_json on '{}.{}': optional is supported only for int, float, bool, string, and char fields",
                path, field.source_name
            ));
            return None;
        }
        let representation = self.ir_type_representation(field_type).clone();
        match representation {
            IRType::Int | IRType::Float | IRType::Bool | IRType::String | IRType::Char => {
                let type_name = match representation {
                    IRType::Int => "int",
                    IRType::Float => "float",
                    IRType::Bool => "bool",
                    IRType::String => "string",
                    _ => "char",
                };
                let default = self.lower_default_value_for_type(&representation, ir_func);
                let path_lit = self.lower_string_literal(path, ir_func);
                let type_lit = self.lower_string_literal(type_name, ir_func);
                let optional = self
                    .builder
                    .build_const_int(ir_func, i64::from(field.optional));
                let value = self.json_host(
                    ir_func,
                    "spectra.api.json.decode_field",
                    vec![child, path_lit, type_lit, optional, default],
                    representation.clone(),
                    "did not decode a JSON field",
                );
                self.builder.build_store(ir_func, field_ptr, value);
                Some(())
            }
            IRType::ExactInt { .. } | IRType::ExactFloat { .. } => {
                self.error(format!(
                    "JSON from_json on '{}.{}': exact-width integer and float fields are not supported",
                    path, field.source_name
                ));
                None
            }
            IRType::Struct { name, .. } => {
                let type_lit = self.lower_string_literal(&name, ir_func);
                let path_lit = self.lower_string_literal(path, ir_func);
                let required = self.builder.build_const_int(ir_func, 0);
                let nested = self.json_host(
                    ir_func,
                    "spectra.api.json.decode_field",
                    vec![child, path_lit, type_lit, required, required],
                    IRType::Int,
                    "did not decode a nested JSON object",
                );
                let nested_ptr =
                    self.lower_derive_decode_object(&name, nested, path, ir_func, stack);
                self.builder.build_store(ir_func, field_ptr, nested_ptr);
                Some(())
            }
            IRType::Array { .. } => {
                self.error(format!(
                    "JSON from_json on '{}.{}': array fields are not supported (array annotations erase to dynamic size)",
                    path, field.source_name
                ));
                None
            }
            _ => {
                self.error(format!(
                    "JSON from_json on '{}.{}': field type {:?} is not supported",
                    path, field.source_name, field_type
                ));
                None
            }
        }
    }

    /// Decode a whole JSON document into a struct value.
    pub(crate) fn lower_derive_from_json(
        &mut self,
        struct_name: &str,
        json: Value,
        ir_func: &mut IRFunction,
    ) -> Value {
        let root = self.json_host(
            ir_func,
            "spectra.api.json.parse",
            vec![json],
            IRType::Int,
            "did not parse a JSON document",
        );
        // Object-mode check: any non-scalar type name selects object
        // validation and returns the handle for per-field decoding.
        let type_lit = self.lower_string_literal(struct_name, ir_func);
        let root_path = self.lower_string_literal("$", ir_func);
        let required = self.builder.build_const_int(ir_func, 0);
        let obj = self.json_host(
            ir_func,
            "spectra.api.json.decode_field",
            vec![root, root_path, type_lit, required, required],
            IRType::Int,
            "did not validate a JSON object root",
        );
        let mut stack = Vec::new();
        self.lower_derive_decode_object(struct_name, obj, "", ir_func, &mut stack)
    }

    /// Render the schema DSL consumed by `spectra.api.json.typed_error_field`.
    /// The `Err` message already names the struct and field; the caller
    /// reports it verbatim so each problem yields exactly one diagnostic.
    fn derive_schema_string(
        &mut self,
        struct_name: &str,
        stack: &mut Vec<String>,
    ) -> Result<String, String> {
        if stack.contains(&struct_name.to_string()) {
            return Err(format!(
                "JSON json_error_field on recursive struct '{struct_name}' is not supported"
            ));
        }
        let Some(schema) = self.json_struct_schemas.get(struct_name).cloned() else {
            return Err(format!(
                "JSON json_error_field on unknown struct '{struct_name}'"
            ));
        };
        stack.push(struct_name.to_string());
        let mut out = format!("{struct_name}{{");
        for field in &schema {
            let Some(ty) = self.derive_schema_type(field, stack) else {
                return Err(format!(
                    "JSON json_error_field on '{struct_name}.{}': field type {:?} is not supported",
                    field.source_name, field.field_type
                ));
            };
            out.push_str(&field.json_name);
            out.push(':');
            out.push_str(&ty);
            out.push(if field.optional { '?' } else { '!' });
            out.push(';');
        }
        out.push('}');
        stack.pop();
        Ok(out)
    }

    fn derive_schema_type(
        &mut self,
        field: &JsonFieldSchema,
        stack: &mut Vec<String>,
    ) -> Option<String> {
        let representation = self.ir_type_representation(&field.field_type).clone();
        match representation {
            IRType::Int => Some("int".to_string()),
            IRType::Float => Some("float".to_string()),
            IRType::Bool => Some("bool".to_string()),
            IRType::String => Some("string".to_string()),
            IRType::Char => Some("char".to_string()),
            IRType::Struct { name, .. } => {
                let inner = self.derive_schema_string(&name, stack).ok()?;
                Some(inner.replacen(&name.to_string(), "", 1))
            }
            IRType::Array { element_type, .. } => {
                let element_rep = self.ir_type_representation(element_type.as_ref()).clone();
                let element = match element_rep {
                    IRType::Int => "int".to_string(),
                    IRType::Float => "float".to_string(),
                    IRType::Bool => "bool".to_string(),
                    IRType::String => "string".to_string(),
                    IRType::Char => "char".to_string(),
                    IRType::Struct { name, .. } => {
                        let inner = self.derive_schema_string(&name, stack).ok()?;
                        inner.replacen(&name.to_string(), "", 1)
                    }
                    _ => return None,
                };
                Some(format!("[{element}]"))
            }
            _ => None,
        }
    }

    /// Lower `Type::json_error_field(json)` to a single host call reporting
    /// the first violating path, or `""` when the document is valid.
    pub(crate) fn lower_derive_error_field(
        &mut self,
        struct_name: &str,
        json: Value,
        ir_func: &mut IRFunction,
    ) -> Value {
        let mut stack = Vec::new();
        let schema = match self.derive_schema_string(struct_name, &mut stack) {
            Ok(schema) => schema,
            Err(message) => return self.invalid_value(message),
        };
        let schema_lit = self.lower_string_literal(&schema, ir_func);
        self.json_host(
            ir_func,
            "spectra.api.json.typed_error_field",
            vec![schema_lit, json],
            IRType::String,
            "did not validate a JSON document",
        )
    }

    /// Lower `value.to_json()` for unit-only derived enums as a quoted wire
    /// name selected by the runtime tag. Data-carrying variants have no real
    /// encoding and are rejected explicitly.
    pub(crate) fn lower_enum_to_json(
        &mut self,
        enum_name: &str,
        enum_value: Value,
        ir_func: &mut IRFunction,
    ) -> Value {
        let Some(variants) = self.enum_definitions.get(enum_name).cloned() else {
            return self.invalid_value(format!("JSON to_json on unknown enum '{enum_name}'"));
        };
        if variants.iter().any(|(_, _, data)| data.is_some()) {
            return self.invalid_value(format!(
                "JSON to_json on enum '{enum_name}' is not supported for data-carrying variants"
            ));
        }
        let renames = self
            .json_enum_schemas
            .get(enum_name)
            .cloned()
            .unwrap_or_default();
        let wire_names: Vec<String> = variants
            .iter()
            .map(|(variant_name, _, _)| {
                renames
                    .iter()
                    .find(|(name, _)| name == variant_name)
                    .map(|(_, json_name)| json_name.clone())
                    .unwrap_or_else(|| variant_name.clone())
            })
            .collect();
        // `ErrorCode` crosses the host ABI as a closed scalar tag rather than
        // a heap pointer (see EnumVariant lowering); every other unit variant
        // stores its tag at offset zero of a one-tuple alloca.
        let tag = if enum_name == "ErrorCode" {
            enum_value
        } else {
            let tag_ptr = self.builder.build_field_ptr(ir_func, enum_value, 0);
            self.builder.build_load_typed(ir_func, tag_ptr, IRType::Int)
        };
        if variants.is_empty() {
            return self.invalid_value(format!(
                "JSON to_json on enum '{enum_name}' without variants"
            ));
        }
        if variants.len() == 1 {
            return self.json_quote(ir_func, &wire_names[0]);
        }
        let merge_bb = ir_func.add_block("enum_json.merge");
        let else_bb = ir_func.add_block("enum_json.else");
        let mut phi_inputs: Vec<(Value, usize)> = Vec::new();
        let mut cond_bb = self.builder.get_current_block();
        for (index, wire_name) in wire_names.iter().enumerate() {
            let body_bb = ir_func.add_block(format!("enum_json.{index}"));
            let next_bb = if index + 1 < wire_names.len() {
                ir_func.add_block(format!("enum_json.cond.{index}"))
            } else {
                else_bb
            };
            if let Some(current) = cond_bb {
                self.builder.set_current_block(current);
            }
            let expected = self.builder.build_const_int(ir_func, index as i64);
            let is_variant = self.builder.build_eq(ir_func, tag, expected);
            self.builder
                .build_cond_branch(ir_func, is_variant, body_bb, next_bb);
            self.builder.set_current_block(body_bb);
            let text = self.json_quote(ir_func, wire_name);
            self.builder.build_branch(ir_func, merge_bb);
            phi_inputs.push((text, body_bb));
            cond_bb = Some(next_bb);
        }
        if let Some(current) = cond_bb {
            self.builder.set_current_block(current);
        }
        // Unreachable: tags only come from variant construction, which always
        // yields a valid index. The empty literal keeps the IR verifier whole
        // without inventing data on any reachable path.
        let fallback = self.lower_string_literal("", ir_func);
        self.builder.build_branch(ir_func, merge_bb);
        phi_inputs.push((fallback, else_bb));
        self.builder.set_current_block(merge_bb);
        self.builder.build_phi(ir_func, phi_inputs)
    }
}

/// Scalar JSON field types. `optional` is supported only for these: struct
/// and array fields always decode from a present value or fail loudly.
fn is_json_scalar(field_type: &IRType) -> bool {
    matches!(
        field_type,
        IRType::Int | IRType::Float | IRType::Bool | IRType::String | IRType::Char
    )
}
