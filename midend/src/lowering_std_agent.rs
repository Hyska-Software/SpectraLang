// Hand-written lowering table for the `std.agent` namespace (R-3209/R-3211/R-3212/R-3216).
//
// The R-3207 generator covers the seven legacy tables through its LAYOUT; this
// namespace was created after that migration, so the arm lives here until it is
// absorbed into LAYOUT. The shape intentionally matches a generated group
// (`match (module, function)` with an explicit `HostFunctionDescriptor`) so the
// absorption is mechanical and `generate_stdlib_catalog.py` can read it if the
// file is added to `LOWERING_TABLES`.
//
// R-3211 adds the model gateway: `Result<T, Error>` returns use the same
// two-word tagged representation as `std.fs`/`std.env`, and IO functions return
// `Task<Result<T, Error>>` so compiled `await` (and `block_on`) drives them
// through the runtime task protocol. R-3212 adds the synchronous
// `remember`/`recall` pair with the same tagged `Result` shape.

use super::*;

/// `Run`/`ChunkStream` are opaque pointer-sized handles; the IR only needs the
/// nominal struct name so `Ok(payload)` keeps the handle value intact.
fn agent_handle_ir_type(name: &str) -> IRType {
    IRType::Struct {
        name: name.to_string(),
        fields: Vec::new(),
    }
}

/// `Result<Tensor, Error>`: the shared `builtin_result_ir_type` helper cannot
/// name a tensor payload, so the descriptor spells the enum explicitly. The
/// variant order and payload layout still match every other `Result`.
fn agent_result_tensor_ir_type() -> IRType {
    IRType::Enum {
        name: "Result_Tensor_Error".to_string(),
        variants: vec![
            (
                "Ok".to_string(),
                Some(vec![IRType::Tensor {
                    dtype: Box::new(IRType::Float),
                    rank: Some(1),
                    dims: None,
                    layout: None,
                    device: None,
                }]),
            ),
            ("Err".to_string(), Some(vec![builtin_error_ir_type()])),
        ],
    }
}

fn agent_task(output: IRType) -> IRType {
    IRType::Task {
        output: Box::new(output),
    }
}

fn agent_descriptor(runtime_name: &'static str, return_type: IRType) -> HostFunctionDescriptor {
    HostFunctionDescriptor {
        runtime_name,
        return_type,
        returns_value: true,
    }
}

pub(crate) fn lookup_std_host_group_agent(
    module: &str,
    function: &str,
) -> Option<HostFunctionDescriptor> {
    match (module, function) {
        ("agent", "token_count") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.agent.token_count",
            return_type: IRType::Int,
            returns_value: true,
        }),
        // ── R-3211 run lifecycle ─────────────────────────────────────────
        ("agent", "agent_start") => Some(agent_descriptor(
            "spectra.std.agent.agent_start",
            builtin_result_ir_type(agent_handle_ir_type("Run")),
        )),
        ("agent", "agent_end") => Some(agent_descriptor(
            "spectra.std.agent.agent_end",
            builtin_result_ir_type(IRType::String),
        )),
        // ── model gateway (async) ────────────────────────────────────────
        ("agent", "ask") => Some(agent_descriptor(
            "spectra.std.agent.ask",
            agent_task(builtin_result_ir_type(IRType::String)),
        )),
        ("agent", "ask_json") => Some(agent_descriptor(
            "spectra.std.agent.ask_json",
            agent_task(builtin_result_ir_type(IRType::String)),
        )),
        ("agent", "ask_stream") => Some(agent_descriptor(
            "spectra.std.agent.ask_stream",
            agent_task(builtin_result_ir_type(agent_handle_ir_type("ChunkStream"))),
        )),
        ("agent", "stream_next") => Some(agent_descriptor(
            "spectra.std.agent.stream_next",
            agent_task(builtin_result_ir_type(IRType::String)),
        )),
        ("agent", "stream_close") => Some(agent_descriptor(
            "spectra.std.agent.stream_close",
            agent_task(builtin_result_ir_type(IRType::Bool)),
        )),
        ("agent", "embed") => Some(agent_descriptor(
            "spectra.std.agent.embed",
            agent_task(agent_result_tensor_ir_type()),
        )),
        // ── budget (sync; R-3216) ────────────────────────────────────────
        ("agent", "budget_remaining") => Some(agent_descriptor(
            "spectra.std.agent.budget_remaining",
            builtin_result_ir_type(IRType::Int),
        )),
        // ── memory (sync; R-3212) ────────────────────────────────────────
        ("agent", "remember") => Some(agent_descriptor(
            "spectra.std.agent.remember",
            builtin_result_ir_type(IRType::Bool),
        )),
        ("agent", "recall") => Some(agent_descriptor(
            "spectra.std.agent.recall",
            builtin_result_ir_type(IRType::String),
        )),
        // ── tool dispatch (R-3222) ───────────────────────────────────────
        ("agent", "act") => Some(agent_descriptor(
            "spectra.std.agent.act",
            agent_task(builtin_result_ir_type(IRType::String)),
        )),
        ("agent", "tool_call") => Some(agent_descriptor(
            "spectra.std.agent.tool_call",
            agent_task(builtin_result_ir_type(IRType::String)),
        )),
        // Internal: emitted by the synthesized registration function, never by
        // user code. The IR return type must match the compiler entry so the
        // catalog and the runtime agree.
        ("agent", "register_tool") => Some(agent_descriptor(
            "spectra.std.agent.register_tool",
            builtin_result_ir_type(IRType::Bool),
        )),
        // ── governance (sync; R-3217) ────────────────────────────────────
        ("agent", "approve") => Some(agent_descriptor(
            "spectra.std.agent.approve",
            builtin_result_ir_type(IRType::Bool),
        )),
        ("agent", "require") => Some(agent_descriptor(
            "spectra.std.agent.require",
            builtin_result_ir_type(IRType::Bool),
        )),
        _ => None,
    }
}
