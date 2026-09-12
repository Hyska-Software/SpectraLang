// Hand-written lowering table for the `std.agent` namespace (R-3209).
//
// The R-3207 generator covers the seven legacy tables through its LAYOUT; this
// namespace was created after that migration, so the arm lives here until it is
// absorbed into LAYOUT. The shape intentionally matches a generated group
// (`match (module, function)` with an explicit `HostFunctionDescriptor`) so the
// absorption is mechanical and `generate_stdlib_catalog.py` can read it if the
// file is added to `LOWERING_TABLES`.

use super::*;

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
        _ => None,
    }
}
