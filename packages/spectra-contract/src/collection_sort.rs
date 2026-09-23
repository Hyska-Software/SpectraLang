//! Internal compiler/runtime ABI tags for typed list sorting.

pub const INT: i64 = 1;
pub const FLOAT: i64 = 2;
pub const BOOL: i64 = 3;
pub const STRING: i64 = 4;
pub const CHAR: i64 = 5;

/// Exact integer tags carry their source width as `BASE + width_in_bits`.
pub const SIGNED_EXACT_BASE: i64 = 100;
pub const UNSIGNED_EXACT_BASE: i64 = 200;

/// Passed only after lowering has recorded a diagnostic for a non-orderable type.
pub const INVALID: i64 = -1;
