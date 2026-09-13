// R-3222 — synthesized agent-tool marshalling wrappers and registration.
//
// A `#[agent_tool]` function is an ordinary compiled function; the model can
// only reach it through the runtime `act` loop, which invokes a per-tool
// **marshalling wrapper** by address (ADR 0019). This module synthesizes those
// wrappers, the static registration that hands their addresses to the runtime,
// and the call that registers them before the first dispatch of a module.
//
// Frozen wrapper ABI (proved in JIT and AOT by R-3222-T1; recorded in ADR
// 0019):
//
//   extern "C" fn __spectra_agent_tool_<name>(run: i64, args_json: ptr,
//                                             out_slot: ptr) -> i64 status
//
// * `run` is the live run handle (also the tool's first authored parameter);
// * `args_json` is the model-provided JSON argument document, packed UTF-8
//   with a single trailing NUL in the runtime arena;
// * `out_slot` points at one `i64` word the wrapper writes the result pointer
//   into: on success (status 0) a packed JSON document, on failure (status 1)
//   a packed human-readable typed-error message.
//
// All three parameters and the return value are i64, exactly the callback
// shape `CoroutineCreate` already passes through (`poll`/`drop`), so the
// runtime invokes the wrapper with the proven mechanism.
//
// The wrapper decodes the arguments through the same derived `from_json`
// lowering user code uses, calls the async tool, drives the returned task to
// completion with `block_on`, and encodes the result through the derived
// `to_json` lowering. It carries `suspension_barrier` so ordinary
// optimization cannot inline it away or delete it: the runtime holds only its
// address.

use super::*;
use crate::ir::InstructionKind;
use spectra_compiler::ast::{AttributeArgument, Function as ASTFunction};

/// Prefix of a synthesized tool wrapper's IR name.
pub(crate) const AGENT_TOOL_WRAPPER_PREFIX: &str = "__spectra_agent_tool_";

/// Prefix of a synthesized per-module registration function's IR name. The
/// module path follows, so every module has a project-unique symbol an
/// importing module can call without knowing which tools the module declares.
pub(crate) const AGENT_REGISTER_TOOLS_PREFIX: &str = "__spectra_agent_register_tools_";

/// Host call the registration function emits per tool.
pub(crate) const AGENT_REGISTER_TOOL_HOST_CALL: &str = "spectra.std.agent.register_tool";

/// Host calls that dispatch through the governed tool path. A function that
/// contains one of these must register its module's tools before it runs.
///
/// `compensate` is included because it validates the declared name against the
/// runtime registry, and `rollback` because it dispatches the compensations;
/// both therefore need the module's tools registered before they run (R-3224).
/// The MCP hosts (R-3218) dispatch through the same path: `mcp_handle` and
/// `mcp_serve` answer `tools/call` for this project's tools, and `mcp_connect`
/// registers the remote tools alongside them.
///
/// The A2A and ACP hosts (R-3219) dispatch as well: `a2a_card` and `a2a_handle`
/// read the derived tool surface and run a delegated task through `act`, and
/// `acp_handle`'s `session/prompt` runs the same loop, so all three register
/// their module's tools first. `acp_permission` performs no dispatch; it only
/// asks for and journals a decision.
pub(crate) const AGENT_DISPATCH_HOST_CALLS: &[&str] = &[
    "spectra.std.agent.act",
    "spectra.std.agent.tool_call",
    "spectra.std.agent.compensate",
    "spectra.std.agent.rollback",
    "spectra.std.agent.mcp_connect",
    "spectra.std.agent.mcp_handle",
    "spectra.std.agent.mcp_serve",
    "spectra.std.agent.a2a_card",
    "spectra.std.agent.a2a_handle",
    "spectra.std.agent.a2a_serve",
    "spectra.std.agent.acp_handle",
];

/// IR name of the marshalling wrapper for `tool`.
pub(crate) fn agent_tool_wrapper_name(tool: &str) -> String {
    format!("{AGENT_TOOL_WRAPPER_PREFIX}{tool}")
}

/// IR name of the registration function of the module at `module_path`.
///
/// The path is sanitized to a valid symbol suffix; a module path never needs
/// more than `.`/`-` mapping to `_`.
pub(crate) fn agent_register_tools_name(module_path: &str) -> String {
    let sanitized: String = module_path
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    format!("{AGENT_REGISTER_TOOLS_PREFIX}{sanitized}")
}

/// Every `#[agent_tool]` function declared by `module`, in declaration order.
pub(crate) fn module_agent_tools(module: &ASTModule) -> Vec<&ASTFunction> {
    module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Function(func)
                if func
                    .attributes
                    .iter()
                    .any(|attribute| attribute.name == "agent_tool") =>
            {
                Some(func)
            }
            _ => None,
        })
        .collect()
}

/// The `IRType::Struct` name of an aggregate payload/result, if it is one.
fn aggregate_name(ty: &IRType) -> Option<&str> {
    match ty {
        IRType::Struct { name, .. } | IRType::Enum { name, .. } => Some(name),
        _ => None,
    }
}

impl ASTLowering {
    /// Synthesize this module's tool wrappers and its registration function,
    /// then wire registration into every dispatch entry point.
    ///
    /// Each module registers the tools it **declares**: the wrapper address,
    /// the model-facing description/schema and the derived effect set are all
    /// known where the tool is defined. A module that dispatches through
    /// `act`/`tool_call` calls its own registration function plus the
    /// (deterministically named) registration function of every imported
    /// module that declares tools, so cross-module tools are reachable by
    /// address without a second dispatch path.
    ///
    /// Runs after every user function has been lowered, so the tool's
    /// parameter/return IR types, its JSON derive schema and its coroutine
    /// ramp all exist.
    pub(crate) fn synthesize_agent_tools(
        &mut self,
        ast_module: &ASTModule,
        ir_module: &mut IRModule,
    ) {
        let tools = module_agent_tools(ast_module);
        for tool in &tools {
            if let Some(wrapper) = self.synthesize_tool_wrapper(tool) {
                ir_module.add_function(wrapper);
            }
        }

        let own_registration = if tools.is_empty() {
            None
        } else {
            let register = self.synthesize_agent_registration(ast_module, ir_module, &tools);
            let name = register.name.clone();
            ir_module.add_function(register);
            Some(name)
        };

        let dispatch_functions: Vec<String> = ir_module
            .functions
            .iter()
            .filter(|function| function_dispatches_tools(function))
            .map(|function| function.name.clone())
            .collect();
        if dispatch_functions.is_empty() {
            return;
        }

        // Imported modules that declare tools: their registration function is
        // named from the module path, so the caller can declare and call it
        // without knowing anything about the tools themselves.
        let mut imported: Vec<String> = Vec::new();
        for tool in &ast_module.imported_agent_tools {
            let symbol = agent_register_tools_name(&tool.module_path);
            if !imported.contains(&symbol) {
                imported.push(symbol);
            }
        }
        for symbol in &imported {
            if !ir_module
                .external_functions
                .iter()
                .any(|external| &external.name == symbol)
            {
                ir_module.external_functions.push(ExternalFunction {
                    name: symbol.clone(),
                    params: Vec::new(),
                    return_type: IRType::Int,
                });
            }
        }

        let mut callees: Vec<String> = Vec::new();
        if let Some(own) = &own_registration {
            callees.push(own.clone());
        }
        callees.extend(imported);
        for function in &mut ir_module.functions {
            if dispatch_functions.contains(&function.name) {
                insert_registration_calls(function, &callees);
            }
        }
    }

    /// Build one marshalling wrapper for `tool`, or report why it cannot be
    /// synthesized (a lowering error fails the module rather than emitting a
    /// wrapper the runtime would crash on).
    fn synthesize_tool_wrapper(&mut self, tool: &ASTFunction) -> Option<IRFunction> {
        let name = tool.name.clone();
        let Some(params) = self.function_parameter_types.get(&name).cloned() else {
            return None;
        };
        let Some(declared_return) = self.function_return_types.get(&name).cloned() else {
            self.error(format!(
                "agent tool '{name}' has no lowered return type; the marshalling wrapper cannot encode its result"
            ));
            return None;
        };
        let output = match declared_return {
            IRType::Task { output } => *output,
            other => other,
        };
        if params.len() != 2 {
            self.error(format!(
                "agent tool '{name}' must lower to (run, payload); found {} parameter(s)",
                params.len()
            ));
            return None;
        }
        let run_type = params[0].clone();
        let payload_type = params[1].clone();
        let Some(payload_name) = aggregate_name(&payload_type).map(str::to_string) else {
            self.error(format!(
                "agent tool '{name}' has a payload type that is not a record or unit-only enum; the marshalling wrapper can only decode documents into a derived aggregate"
            ));
            return None;
        };
        if !self.json_struct_schemas.contains_key(&payload_name) {
            self.error(format!(
                "agent tool '{name}' payload '{payload_name}' has no JSON derive schema; add #[derive(Serialize, Deserialize)] to the payload record"
            ));
            return None;
        }

        let wrapper_name = agent_tool_wrapper_name(&name);
        let mut wrapper = IRFunction::new(
            wrapper_name,
            vec![
                Parameter {
                    id: 0,
                    name: "run".to_string(),
                    ty: run_type,
                },
                Parameter {
                    id: 1,
                    name: "args_json".to_string(),
                    ty: IRType::String,
                },
                Parameter {
                    id: 2,
                    name: "out_slot".to_string(),
                    ty: IRType::Int,
                },
            ],
            IRType::Int,
        );
        // The runtime holds only the address, so the wrapper must survive
        // optimization. It does not need `suspension_barrier` (which is the
        // coroutine-frame shield): dead-code elimination never removes whole
        // functions, and the function-inlining pass refuses any function that
        // contains a `Call`/`HostCall`, which the wrapper body always does.
        // The `FuncAddr` in the registration function is what makes it
        // reachable; see ADR 0019 ("Equivalent shield").

        let entry = wrapper.add_block("entry");
        let invalid = wrapper.add_block("invalid_arguments");
        let decode = wrapper.add_block("decode_and_call");
        let saved_block = self.builder.get_current_block();
        self.builder.set_current_block(entry);

        let run = Value { id: 0 };
        let args_json = Value { id: 1 };
        let out_slot = Value { id: 2 };

        // 1. Validate the document against the payload's derived schema first:
        //    a violation must reach the model as a typed message, never as a
        //    partially decoded payload.
        let violation = self.lower_derive_error_field(&payload_name, args_json, &mut wrapper);
        let empty = self.lower_string_literal("", &mut wrapper);
        // String comparison must go through the runtime codec: the raw `ne`
        // instruction compares pointers, and two equal strings are almost
        // never the same pointer.
        let is_valid = self.require_value(
            self.builder.build_typed_host_call(
                &mut wrapper,
                "spectra.std.string.eq".to_string(),
                vec![violation, empty],
                IRType::Bool,
                true,
            ),
            format!("agent tool '{name}': validating the argument document produced no verdict"),
        );
        let has_violation = self.builder.build_not(&mut wrapper, is_valid);
        self.builder.build_cond_branch(&mut wrapper, has_violation, invalid, decode);

        self.builder.set_current_block(invalid);
        self.builder.build_store(&mut wrapper, out_slot, violation);
        let failure = self.builder.build_const_int(&mut wrapper, 1);
        self.builder.build_return(&mut wrapper, Some(failure));

        // 2. Decode, call the async tool and drive its task to completion.
        self.builder.set_current_block(decode);
        let payload = self.lower_derive_from_json(&payload_name, args_json, &mut wrapper);
        let task = self.require_value(
            self.builder
                .build_call(&mut wrapper, name.clone(), vec![run, payload], true),
            format!("agent tool '{name}' call did not produce its task"),
        );
        if output != IRType::Void {
            let block_on = self.builder.build_typed_host_call(
                &mut wrapper,
                "spectra.async.task.block_on".to_string(),
                vec![task],
                output.clone(),
                true,
            );
            if block_on.is_none() {
                self.error(format!(
                    "agent tool '{name}': driving the tool task did not produce a value"
                ));
            }
        }
        let value = self.require_value(
            self.builder.build_typed_host_call(
                &mut wrapper,
                "spectra.async.task.result".to_string(),
                vec![task],
                output.clone(),
                output != IRType::Void,
            ),
            format!("agent tool '{name}' task produced no result"),
        );

        // 3. Encode the result as the document the model sees.
        let encoded = self.encode_tool_output(&name, &output, value, &mut wrapper);
        self.builder.build_store(&mut wrapper, out_slot, encoded);
        let success = self.builder.build_const_int(&mut wrapper, 0);
        self.builder.build_return(&mut wrapper, Some(success));

        if let Some(saved_block) = saved_block {
            self.builder.set_current_block(saved_block);
        }
        Some(wrapper)
    }

    /// Encode a tool's return value as a JSON document.
    ///
    /// Derived aggregates go through the same `to_json` lowering user code
    /// uses; scalars go through the canonical encoders, so the document the
    /// model reads is byte-identical to one the same value would produce in
    /// source.
    fn encode_tool_output(
        &mut self,
        tool: &str,
        ty: &IRType,
        value: Value,
        ir_func: &mut IRFunction,
    ) -> Value {
        let host = |this: &mut Self,
                    name: &str,
                    args: Vec<Value>,
                    ir_func: &mut IRFunction|
         -> Value {
            this.require_value(
                this.builder.build_typed_host_call(
                    ir_func,
                    name.to_string(),
                    args,
                    IRType::String,
                    true,
                ),
                format!("agent tool '{tool}' result encoder '{name}' produced no value"),
            )
        };
        match ty {
            IRType::Void => self.lower_string_literal("null", ir_func),
            IRType::String => host(self, "spectra.api.json.quote_string", vec![value], ir_func),
            IRType::Int => host(
                self,
                "spectra.std.convert.int_to_string",
                vec![value],
                ir_func,
            ),
            IRType::Bool => host(
                self,
                "spectra.std.convert.bool_to_string",
                vec![value],
                ir_func,
            ),
            IRType::Float => host(
                self,
                "spectra.std.convert.float_to_string",
                vec![value],
                ir_func,
            ),
            IRType::Struct { name, .. } if self.json_struct_schemas.contains_key(name) => {
                let mut stack = Vec::new();
                self.lower_derive_encode_struct(name, value, ir_func, &mut stack)
            }
            IRType::Enum { name, .. } if self.json_enum_schemas.contains_key(name) => {
                self.lower_enum_to_json(name, value, ir_func)
            }
            // A `List<element>` result is a JSON array: one host call reads the
            // elements and formats the whole array.
            IRType::Generic { name, .. } if name == "List" => {
                let Some(element_kind) = list_element_json_kind(ty) else {
                    self.error(format!(
                        "agent tool '{tool}' returns {ty:?}: a tool result list must hold int, \
                         float, bool, string, or char elements"
                    ));
                    return self.lower_string_literal("null", ir_func);
                };
                let kind = self.lower_string_literal(element_kind, ir_func);
                host(self, "spectra.api.json.encode_list", vec![value, kind], ir_func)
            }
            other => {
                self.error(format!(
                    "agent tool '{tool}' returns {other:?}, which has no JSON encoding; \
                     return a String, an int, a bool, a float, a derived record, or a List of scalars"
                ));
                self.lower_string_literal("null", ir_func)
            }
        }
    }

    /// Build this module's idempotent registration function: one
    /// `register_tool(name, wrapper_address, description, input_schema,
    /// effects)` host call per declared tool.
    ///
    /// The runtime registry is keyed by tool name and idempotent, so calling
    /// this function more than once (one call site per dispatch entry point)
    /// registers each tool exactly once with the same address.
    fn synthesize_agent_registration(
        &mut self,
        ast_module: &ASTModule,
        ir_module: &IRModule,
        tools: &[&ASTFunction],
    ) -> IRFunction {
        let mut function = IRFunction::new(
            agent_register_tools_name(&ast_module.name),
            Vec::new(),
            IRType::Int,
        );
        // Reachability comes from the `Call` inserted at each dispatch entry
        // point. `suspension_barrier` is deliberately not set: it selects the
        // coroutine-frame lowering for allocas, and this function is a plain
        // function. See the wrapper synthesis above for why the shield is not
        // needed.
        let entry = function.add_block("entry");
        let saved_block = self.builder.get_current_block();
        self.builder.set_current_block(entry);
        for tool in tools {
            let wrapper = agent_tool_wrapper_name(&tool.name);
            let name = self.lower_string_literal(&tool.name, &mut function);
            let address = self.builder.build_func_addr(&mut function, wrapper);
            let description = self.lower_string_literal(&tool_description(tool), &mut function);
            let schema = match self.tool_payload_struct_name(tool) {
                Some(payload) => self.lower_derive_json_schema(&payload, &mut function),
                None => self.lower_string_literal("{}", &mut function),
            };
            let effects = self.lower_string_literal(
                &tool_effects_json(ir_module, &tool.name),
                &mut function,
            );
            let registration = self.builder.build_typed_host_call(
                &mut function,
                AGENT_REGISTER_TOOL_HOST_CALL.to_string(),
                vec![name, address, description, schema, effects],
                builtin_result_ir_type(IRType::Bool),
                true,
            );
            self.require_value(
                registration,
                format!("registering agent tool '{}' produced no result", tool.name),
            );
        }
        let zero = self.builder.build_const_int(&mut function, 0);
        self.builder.build_return(&mut function, Some(zero));
        if let Some(saved_block) = saved_block {
            self.builder.set_current_block(saved_block);
        }
        function
    }

    /// The `IRType::Struct` name of a tool's payload parameter.
    fn tool_payload_struct_name(&self, tool: &ASTFunction) -> Option<String> {
        let params = self.function_parameter_types.get(&tool.name)?;
        let payload = params.get(1)?;
        aggregate_name(payload).map(str::to_string)
    }
}

/// The authored description of an `#[agent_tool(...)]` declaration.
fn tool_description(tool: &ASTFunction) -> String {
    for attribute in &tool.attributes {
        if attribute.name != "agent_tool" {
            continue;
        }
        for argument in &attribute.arguments {
            if let AttributeArgument::StringLiteral(text) = argument {
                return text.clone();
            }
        }
    }
    String::new()
}

/// Whether `function` contains a call into the governed tool dispatch path.
fn function_dispatches_tools(function: &IRFunction) -> bool {
    function.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                &instruction.kind,
                InstructionKind::HostCall { host, .. }
                    if AGENT_DISPATCH_HOST_CALLS.contains(&host.as_str())
            )
        })
    })
}

/// Insert the registration calls at the top of the entry block, after any
/// leading stack allocations, so they run before the first dispatch.
fn insert_registration_calls(function: &mut IRFunction, callees: &[String]) {
    let Some(entry_id) = function.blocks.first().map(|block| block.id) else {
        return;
    };
    let mut instructions: Vec<Instruction> = Vec::with_capacity(callees.len());
    for callee in callees {
        let result = function.next_value();
        instructions.push(Instruction {
            id: 0,
            kind: InstructionKind::Call {
                result: Some(result),
                function: callee.clone(),
                args: Vec::new(),
                is_tail: false,
            },
            source_span: None,
        });
    }
    let Some(block) = function.get_block_mut(entry_id) else {
        return;
    };
    let position = block
        .instructions
        .iter()
        .position(|instruction| !matches!(instruction.kind, InstructionKind::Alloca { .. }))
        .unwrap_or(block.instructions.len());
    for (offset, instruction) in instructions.into_iter().enumerate() {
        block.instructions.insert(position + offset, instruction);
    }
    for (index, instruction) in block.instructions.iter_mut().enumerate() {
        instruction.id = index;
    }
}

/// JSON array of the host calls reachable from `tool`'s body through the
/// module's static call graph.
///
/// This is the same effect set `spectralang surface --json` derives; it is
/// computed here so registration can hand the runtime the effects it must
/// validate against the run's grant. Cross-module callees are not visible
/// while a module is lowered in isolation, so an effect reached only through
/// another user module is under-approximated here and is caught by that
/// module's own registration when the callee is itself a tool.
fn tool_effects_json(module: &IRModule, tool: &str) -> String {
    let mut effects: Vec<String> = Vec::new();
    let mut visited: Vec<String> = vec![tool.to_string()];
    let mut pending: Vec<String> = vec![tool.to_string()];
    while let Some(function_name) = pending.pop() {
        let Some(function) = module.get_function(&function_name) else {
            continue;
        };
        for block in &function.blocks {
            for instruction in &block.instructions {
                let callee = match &instruction.kind {
                    InstructionKind::HostCall { host, .. } => {
                        if (!effects.iter().any(|effect| effect == host))
                            && !host.starts_with("spectra.std.agent.")
                        {
                            effects.push(host.clone());
                        }
                        None
                    }
                    InstructionKind::Call { function, .. } => Some(function.clone()),
                    InstructionKind::CoroutineCreate { poll, drop, .. } => {
                        let mut queued = vec![poll.clone(), drop.clone()];
                        pending.append(&mut queued);
                        None
                    }
                    InstructionKind::FuncAddr { function, .. } => Some(function.clone()),
                    _ => None,
                };
                if let Some(callee) = callee {
                    if !visited.contains(&callee) {
                        visited.push(callee.clone());
                        pending.push(callee);
                    }
                }
            }
        }
    }
    effects.sort();
    let mut rendered = String::from("[");
    for (index, effect) in effects.iter().enumerate() {
        if index > 0 {
            rendered.push(',');
        }
        rendered.push('"');
        for character in effect.chars() {
            match character {
                '"' => rendered.push_str("\\\""),
                '\\' => rendered.push_str("\\\\"),
                other => rendered.push(other),
            }
        }
        rendered.push('"');
    }
    rendered.push(']');
    rendered
}
