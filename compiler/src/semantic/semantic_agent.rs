// R-3210 — `#[agent_tool]` validation and derived tool metadata.
//
// Exactly one new attribute exists (`#[agent_tool("description")]`). The
// declaration is validated here, from the `Item::Function` arm, because
// function attributes had no validation path before this item. Everything a
// model needs except the description is derived: the tool name is the function
// name, the input schema comes from the payload parameter through the existing
// JSON derive, and effects/capabilities are read from the IR by the surface
// command (`cli_surface.rs`).
//
// Unknown attributes on functions stay silently ignored (documented
// behaviour); only `agent_tool` misuse is an error.
//
// Codes: `E3203` declaration misuse (one hint per rejection condition),
// `E3204` payload type the JSON derive cannot decode.

use super::*;
use crate::ast::{AttributeArgument, Function, TypeAnnotation, Visibility};
use std::sync::LazyLock;

/// One validated `#[agent_tool]` declaration.
///
/// Mirrors the `json_struct_derives` precedent: the semantic pass owns
/// validation and stores the derived metadata in an analyzer map that
/// `collect_module_exports` flushes into the module registry, so tooling
/// (`surface --json`, the MCP server) never re-derives it.
#[derive(Debug, Clone)]
pub(crate) struct AgentToolInfo {
    /// Derived tool name: the function name, unique module-wide.
    pub(crate) name: String,
    /// The only authored string in the declaration.
    pub(crate) description: String,
    /// Name of the payload parameter (the one that is not `run`).
    pub(crate) payload_param: String,
    /// Rendered payload parameter type (diagnostics and tooling).
    pub(crate) payload_type_text: String,
    /// JSON Schema object derived from the payload type.
    pub(crate) input_schema: String,
    /// Span of the function declaration, used for duplicate reporting.
    pub(crate) span: Span,
}

impl SemanticAnalyzer {
    /// Validate `#[agent_tool]` on a free function.
    ///
    /// Returns without side effects when the attribute is absent, so the
    /// existing lenient treatment of every other function attribute is
    /// preserved.
    pub(crate) fn validate_function_attributes(&mut self, func: &Function) {
        let Some(attribute) = func
            .attributes
            .iter()
            .find(|attribute| attribute.name == "agent_tool")
        else {
            return;
        };

        let description = match attribute.arguments.as_slice() {
            [AttributeArgument::StringLiteral(text)] => text.clone(),
            [AttributeArgument::Name(name)] => {
                self.agent_tool_error(
                    func,
                    format!(
                        "The #[agent_tool] description of '{}' must be a string literal, found identifier '{}'",
                        func.name, name
                    ),
                    "Replace the identifier with a quoted string literal.",
                    attribute.span,
                );
                return;
            }
            [AttributeArgument::KeyValue { key, .. }] => {
                self.agent_tool_error(
                    func,
                    format!(
                        "The #[agent_tool] description of '{}' must be a positional string literal, found named argument '{}'",
                        func.name, key
                    ),
                    "Write #[agent_tool(\"...\")] with exactly one positional string literal.",
                    attribute.span,
                );
                return;
            }
            arguments => {
                self.agent_tool_error(
                    func,
                    format!(
                        "#[agent_tool] on '{}' takes exactly one string-literal description, found {} argument(s)",
                        func.name,
                        arguments.len()
                    ),
                    "Write #[agent_tool(\"...\")] with exactly one positional string literal.",
                    attribute.span,
                );
                return;
            }
        };

        // A tool is model-invocable, so it must be part of the module's public
        // surface and awaitable.
        if func.visibility != Visibility::Public {
            self.agent_tool_error(
                func,
                format!("Tool function '{}' must be public", func.name),
                "Add 'public' before the function declaration.",
                func.span,
            );
            return;
        }
        if !func.is_async {
            self.agent_tool_error(
                func,
                format!("Tool function '{}' must be async", func.name),
                "Declare it as 'public async func'.",
                func.span,
            );
            return;
        }
        if !func.type_params.is_empty() {
            self.agent_tool_error(
                func,
                format!("Tool function '{}' cannot be generic", func.name),
                "Remove the type parameters; a tool is dispatched through a fixed marshalling wrapper.",
                func.type_params[0].span,
            );
            return;
        }

        for param in &func.params {
            if matches!(
                self.type_annotation_to_type(&param.ty),
                Type::DynTrait { .. }
            ) {
                self.agent_tool_error(
                    func,
                    format!(
                        "Tool parameter '{}' of '{}' must not be a dyn trait object",
                        param.name, func.name
                    ),
                    "Use a concrete type the marshalling wrapper can decode.",
                    param.span,
                );
                return;
            }
        }

        // The run handle is the model-invocability marker: present and first.
        let run_param = func.params.first().filter(|param| param.name == "run");
        if run_param.is_none() {
            let run_elsewhere = func.params.iter().any(|param| param.name == "run");
            let (message, hint) = if run_elsewhere {
                (
                    format!(
                        "Tool function '{}' must take 'run' as its first parameter",
                        func.name
                    ),
                    "Move the 'run' parameter before the payload parameter.",
                )
            } else {
                (
                    format!(
                        "Tool function '{}' needs a 'run' parameter",
                        func.name
                    ),
                    "Declare the run handle as the first parameter: 'func name(run: Run, payload: Payload)'.",
                )
            };
            self.agent_tool_error(func, message, hint, func.span);
            return;
        }

        if func.params.len() != 2 {
            self.agent_tool_error(
                func,
                format!(
                    "Tool function '{}' must take exactly 'run' plus one payload parameter, found {} parameter(s)",
                    func.name,
                    func.params.len()
                ),
                "Declare 'func name(run: Run, payload: Payload)'.",
                func.span,
            );
            return;
        }

        let payload = &func.params[1];
        let Some(payload_annotation) = payload.ty.clone() else {
            self.push_semantic_error_coded(
                "E3204",
                format!(
                    "Tool parameter '{}' of '{}' needs an explicit decodable type annotation",
                    payload.name, func.name
                ),
                payload.span,
                Some("E3204: tool inputs cross the host ABI as JSON documents.".to_string()),
                Some(
                    "Use int, float, bool, string, char, a record with #[derive(Serialize)] or #[derive(Deserialize)], or a unit-only enum.".to_string(),
                ),
            );
            return;
        };
        let input_schema = match self.render_input_schema(&payload_annotation) {
            Ok(schema) => schema,
            Err(reason) => {
                self.push_semantic_error_coded(
                    "E3204",
                    format!(
                        "Tool parameter '{}' of '{}' has a type the JSON derive cannot decode: {}",
                        payload.name, func.name, reason
                    ),
                    payload.span,
                    Some("E3204: tool inputs cross the host ABI as JSON documents.".to_string()),
                    Some(
                        "Use int, float, bool, string, char, a record with #[derive(Serialize)] or #[derive(Deserialize)], or a unit-only enum.".to_string(),
                    ),
                );
                return;
            }
        };

        let info = AgentToolInfo {
            name: func.name.clone(),
            description,
            payload_param: payload.name.clone(),
            payload_type_text: crate::semantic::surface::render_annotation(&payload_annotation),
            input_schema,
            span: func.span,
        };

        if let Some(existing) = self.agent_tools.get(&func.name) {
            self.push_semantic_error_coded(
                "E3203",
                format!("Duplicate tool name '{}'", func.name),
                func.span,
                Some(format!(
                    "The name was already declared at byte offset {} in this module.",
                    existing.span.start
                )),
                Some("Tool names are the function names; make each tool function name unique within the module.".to_string()),
            );
            return;
        }
        self.agent_tools.insert(func.name.clone(), info);
    }

    fn agent_tool_error(
        &mut self,
        func: &Function,
        message: String,
        hint: &str,
        span: Span,
    ) {
        self.push_semantic_error_coded(
            "E3203",
            message,
            span,
            Some(format!(
                "E3203: invalid #[agent_tool] declaration on '{}'.",
                func.name
            )),
            Some(hint.to_string()),
        );
    }

    /// Validate the `allow` capabilities of a literal `agent_start` spec.
    ///
    /// `AgentSpec` crosses the host ABI as a JSON document (plan adaptation
    /// 11), so the compiler can inspect the spec only when the argument is a
    /// string literal — the same limitation `validate_derived_from_json_literal`
    /// accepts for `from_json`. A computed spec is validated at runtime by
    /// `spectra-agent` (`AgentSpec::parse`), which rejects exactly the same
    /// shapes; the compiler never guesses at a computed string.
    ///
    /// Malformed literals reuse the JSON-derive codes (`EJSON001` syntax,
    /// `EJSON002` root shape, `EJSON004` member type): this is the same
    /// string-literal validation path, and a crash or a silent skip would both
    /// be worse than a diagnostic.
    pub(crate) fn validate_agent_start_capabilities(&mut self, arguments: &[Expression]) {
        let Some(spec_literal) = arguments.first() else {
            return;
        };
        let ExpressionKind::StringLiteral(spec_json) = &spec_literal.kind else {
            return;
        };

        let value = match serde_json::from_str::<serde_json::Value>(spec_json) {
            Ok(value) => value,
            Err(error) => {
                self.push_semantic_error_coded(
                    "EJSON001",
                    format!("Invalid JSON for the agent_start spec: {error}"),
                    spec_literal.span,
                    Some(
                        "A string-literal AgentSpec argument is decoded at compile time."
                            .to_string(),
                    ),
                    Some(
                        "Fix the JSON syntax, or pass a computed string (validated at runtime by the agent crate)."
                            .to_string(),
                    ),
                );
                return;
            }
        };

        let Some(object) = value.as_object() else {
            self.push_semantic_error_coded(
                "EJSON002",
                "Invalid JSON for the agent_start spec: expected an object at the root",
                spec_literal.span,
                Some("An AgentSpec document decodes to a JSON object.".to_string()),
                Some("Pass an object with the AgentSpec fields, or a computed string.".to_string()),
            );
            return;
        };

        let Some(allow) = object.get("allow") else {
            // `allow` is optional; an absent grant list is the default-deny
            // configuration and needs no vocabulary check.
            return;
        };
        let Some(capabilities) = allow.as_array() else {
            self.push_semantic_error_coded(
                "EJSON004",
                "Invalid JSON for the agent_start spec: field 'allow' must be an array of strings",
                spec_literal.span,
                Some("AgentSpec.allow is a list of capability grants.".to_string()),
                Some("Write \"allow\": [\"spectra.std.fs\"] or omit the field.".to_string()),
            );
            return;
        };

        for capability in capabilities {
            let Some(capability) = capability.as_str() else {
                self.push_semantic_error_coded(
                    "EJSON004",
                    "Invalid JSON for the agent_start spec: every 'allow' entry must be a string",
                    spec_literal.span,
                    Some("AgentSpec.allow is a list of capability grants.".to_string()),
                    Some("Use quoted capability strings.".to_string()),
                );
                continue;
            };
            self.validate_capability(capability, spec_literal.span);
        }
    }

    /// Validate one capability grant against the contract catalog.
    ///
    /// A grant names a registered host call by its runtime name
    /// (`spectra.std.fs.fs_read`) or a namespace prefix of one
    /// (`spectra.std.fs`). The runtime evaluates exactly these names, so a
    /// grant that cannot match is rejected here instead of silently doing
    /// nothing. A scoped grant (`name:host=api.example.com`) is accepted only
    /// where the host call declares that key in its catalog `scope_keys`.
    fn validate_capability(&mut self, capability: &str, span: Span) {
        let (name, scope) = match capability.split_once(':') {
            Some((name, scope)) => (name, Some(scope)),
            None => (capability, None),
        };

        if !capability_matches_host_call(name) {
            let hint = match closest_host_call(name) {
                Some(suggestion) => format!(
                    "Did you mean '{suggestion}'? A grant names a registered host call or a namespace prefix of one."
                ),
                None => "A grant names a registered host call (for example 'spectra.std.fs.fs_read') or a namespace prefix of one (for example 'spectra.std.fs').".to_string(),
            };
            self.push_semantic_error_coded(
                "E3201",
                format!(
                    "Unknown capability '{name}': no registered host call or namespace matches it"
                ),
                span,
                Some(
                    "Capability grants are validated against the contract catalog at compile time, so a grant can never silently match nothing."
                        .to_string(),
                ),
                Some(hint),
            );
            return;
        }

        let Some(scope) = scope else {
            return;
        };

        let supported = capability_scope_keys(name);
        for pair in scope.split(',') {
            let pair = pair.trim();
            let Some((key, _value)) = pair.split_once('=') else {
                self.push_semantic_error_coded(
                    "E3202",
                    format!(
                        "Unsupported scope form in capability '{capability}': expected 'key=value'"
                    ),
                    span,
                    Some("A scoped grant is written 'name:key=value'.".to_string()),
                    Some(format!(
                        "Use a 'key=value' pair; '{}' supports: {}.",
                        name,
                        describe_scope_keys(&supported)
                    )),
                );
                continue;
            };
            if key.is_empty() {
                self.push_semantic_error_coded(
                    "E3202",
                    format!("Unsupported scope form in capability '{capability}': empty scope key"),
                    span,
                    Some("A scoped grant is written 'name:key=value'.".to_string()),
                    Some(format!(
                        "Name the scope key; '{}' supports: {}.",
                        name,
                        describe_scope_keys(&supported)
                    )),
                );
                continue;
            }
            if !supported.iter().any(|supported_key| *supported_key == key) {
                self.push_semantic_error_coded(
                    "E3202",
                    format!(
                        "Capability '{name}' does not support scope key '{key}'"
                    ),
                    span,
                    Some(
                        "A scope predicate is accepted only where the host call registers an extractor for that key (catalog `scope_keys`)."
                            .to_string(),
                    ),
                    Some(format!(
                        "Remove the ':{key}=…' predicate or use a supported key; '{}' supports: {}.",
                        name,
                        describe_scope_keys(&supported)
                    )),
                );
            }
        }
    }

    /// Render the JSON Schema for a tool payload parameter.
    ///
    /// The accepted shapes mirror the decode side of the existing JSON derive
    /// (`midend/src/lowering_json_derive.rs`): primitives (with `optional`
    /// allowed only on those), nested derived records, and unit-only enums.
    /// Exact-width ints/floats and arrays are rejected because `from_json`
    /// cannot decode them.
    fn render_input_schema(&self, annotation: &TypeAnnotation) -> Result<String, String> {
        let ty = self.type_annotation_to_type(&Some(annotation.clone()));
        self.render_type_schema(&ty, &mut Vec::new())
    }

    fn render_type_schema(&self, ty: &Type, stack: &mut Vec<String>) -> Result<String, String> {
        match ty {
            Type::Int => Ok("{\"type\":\"integer\"}".to_string()),
            Type::Float => Ok("{\"type\":\"number\"}".to_string()),
            Type::Bool => Ok("{\"type\":\"boolean\"}".to_string()),
            Type::String | Type::Char => Ok("{\"type\":\"string\"}".to_string()),
            Type::Struct { name } => self.render_struct_schema(name, stack),
            Type::Enum { name } => match self.enum_infos.get(name) {
                Some(info)
                    if info
                        .variants
                        .values()
                        .all(|variant| variant.data.is_none() && variant.struct_data.is_none()) =>
                {
                    Ok("{\"type\":\"string\"}".to_string())
                }
                Some(_) => Err(format!(
                    "enum '{name}' carries variant data; only unit-only enums are decodable"
                )),
                None => Err(format!("enum '{name}' is not decodable")),
            },
            Type::ExactInt { .. } => {
                Err(format!("exact-width integer type '{}' is not decodable", type_name(ty)))
            }
            Type::ExactFloat { .. } => {
                Err(format!("exact-width float type '{}' is not decodable", type_name(ty)))
            }
            Type::Array { .. } => {
                Err("array parameters are not decodable (array annotations erase to dynamic size)"
                    .to_string())
            }
            other => Err(format!("type '{}' is not decodable", type_name(other))),
        }
    }

    fn render_struct_schema(
        &self,
        name: &str,
        stack: &mut Vec<String>,
    ) -> Result<String, String> {
        let Some(info) = self.json_struct_derives.get(name).cloned() else {
            return Err(format!(
                "record '{name}' has no #[derive(Serialize)] or #[derive(Deserialize)]"
            ));
        };
        if stack.iter().any(|entry| entry == name) {
            return Err(format!("record '{name}' is recursive"));
        }
        stack.push(name.to_string());

        let mut properties: Vec<String> = Vec::with_capacity(info.fields.len());
        let mut required: Vec<String> = Vec::new();
        for field in &info.fields {
            let field_type = self.type_annotation_to_type(&Some(field.ty.clone()));
            if field.optional && !is_json_scalar_annotation(&field_type) {
                stack.pop();
                return Err(format!(
                    "optional is supported only on int, float, bool, string, and char fields, not on '{name}.{}'",
                    field.source_name
                ));
            }
            let schema = match self.render_type_schema(&field_type, stack) {
                Ok(schema) => schema,
                Err(reason) => {
                    stack.pop();
                    return Err(format!("'{name}.{}': {reason}", field.source_name));
                }
            };
            properties.push(format!(
                "{}:{}",
                json_quote(&field.json_name),
                schema
            ));
            if !field.optional {
                required.push(json_quote(&field.json_name));
            }
        }
        stack.pop();

        Ok(format!(
            "{{\"type\":\"object\",\"properties\":{{{}}},\"required\":[{}]}}",
            properties.join(","),
            required.join(",")
        ))
    }
}

/// Scalar shapes that support `#[json(optional)]`, matching the midend decode
/// restriction (`is_json_scalar` in `lowering_json_derive.rs`).
fn is_json_scalar_annotation(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char
    )
}

/// Quote a JSON member name with the same escaping the derive uses.
fn json_quote(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\""))
}

// ── R-3215 — capability vocabulary ────────────────────────────────────────

/// One registered host call from the contract catalog.
struct HostCallVocabularyEntry {
    /// Runtime host-call name (`spectra.std.fs.fs_read`). This is the name the
    /// dispatch seam evaluates and the only form a grant can match, so it is
    /// the vocabulary.
    binding: String,
    /// Scope predicate keys the host call registers an extractor for.
    scope_keys: Vec<String>,
}

/// The catalog host calls, loaded once per process.
///
/// `spectra-contract` embeds the generated catalog, so this is the same data
/// the R-3206 schema validator audits; a capability can never drift from the
/// surface the runtime dispatches. Only host-call entries are part of the
/// vocabulary — a module or type binding is not something a run can grant.
fn capability_vocabulary() -> &'static [HostCallVocabularyEntry] {
    static VOCABULARY: LazyLock<Vec<HostCallVocabularyEntry>> = LazyLock::new(|| {
        spectra_contract::catalog()
            .entry
            .into_iter()
            // A host call is exactly what the ABI says it is; the catalog's
            // `effects` classify the domain (`filesystem`, `network`, ...),
            // not the dispatch kind.
            .filter(|entry| entry.abi.starts_with("host("))
            .map(|entry| HostCallVocabularyEntry {
                binding: entry.binding,
                scope_keys: entry.scope_keys,
            })
            .collect()
    });
    &VOCABULARY
}

/// Exact host call or a dotted namespace prefix of one.
fn host_call_name_matches(entry: &HostCallVocabularyEntry, name: &str) -> bool {
    let candidate = entry.binding.as_str();
    candidate == name
        || (candidate.len() > name.len()
            && candidate.starts_with(name)
            && candidate.as_bytes()[name.len()] == b'.')
}

/// Whether `name` grants at least one registered host call.
fn capability_matches_host_call(name: &str) -> bool {
    capability_vocabulary()
        .iter()
        .any(|entry| host_call_name_matches(entry, name))
}

/// Scope keys supported by every host call the grant covers, sorted.
fn capability_scope_keys(name: &str) -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = Vec::new();
    for entry in capability_vocabulary() {
        if !host_call_name_matches(entry, name) {
            continue;
        }
        for key in &entry.scope_keys {
            let key = key.as_str();
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    keys.sort_unstable();
    keys
}

/// Closest host call or namespace to an unknown grant, for did-you-mean.
///
/// A grant that spells the catalog path (`std.api.client.request`) is the one
/// predictable mistake: the runtime name is the catalog binding
/// (`spectra.api.client.request`). Suggesting the prefixed name is exact, so
/// it is tried before the Levenshtein window, which mirrors `E033`'s module
/// suggestion (`semantic_item_import.rs`).
fn closest_host_call(name: &str) -> Option<String> {
    let normalized = format!("spectra.{name}");
    if capability_matches_host_call(&normalized) {
        return Some(normalized);
    }

    let mut best: Option<(usize, &str)> = None;
    for entry in capability_vocabulary() {
        let namespace = entry
            .binding
            .rsplit_once('.')
            .map(|(ns, _)| ns)
            .unwrap_or("");
        for candidate in [entry.binding.as_str(), namespace] {
            if candidate.is_empty() || candidate == name {
                continue;
            }
            let distance = levenshtein_distance(name, candidate);
            if best.is_none() || distance < best.expect("checked above").0 {
                best = Some((distance, candidate));
            }
        }
    }
    let (distance, candidate) = best?;
    (distance <= 3).then(|| candidate.to_string())
}

/// Human-readable list of supported scope keys for a hint.
fn describe_scope_keys(keys: &[&str]) -> String {
    if keys.is_empty() {
        "no scope keys".to_string()
    } else {
        keys.join(", ")
    }
}
