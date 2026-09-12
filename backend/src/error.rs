use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendErrorKind {
    Cranelift,
    InvalidIr,
    MissingBlock,
    MissingFunction,
    MissingPhiIncoming,
    MissingValue,
    UnsupportedExecutionReturnType,
    UnsupportedHostArgumentType,
    UnsupportedType,
    TensorIr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendCodegenError {
    kind: BackendErrorKind,
    message: String,
}

impl BackendCodegenError {
    pub fn new(kind: BackendErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> &BackendErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn cranelift(message: impl Into<String>) -> Self {
        Self::new(BackendErrorKind::Cranelift, message)
    }

    pub(crate) fn invalid_ir(message: impl Into<String>) -> Self {
        Self::new(BackendErrorKind::InvalidIr, message)
    }

    pub(crate) fn missing_block(block_id: usize) -> Self {
        Self::new(
            BackendErrorKind::MissingBlock,
            format!(
                "Block {} not found during backend code generation",
                block_id
            ),
        )
    }

    pub(crate) fn missing_function(function: impl AsRef<str>) -> Self {
        Self::new(
            BackendErrorKind::MissingFunction,
            format!(
                "Function '{}' not found during backend code generation",
                function.as_ref()
            ),
        )
    }

    pub(crate) fn missing_phi_incoming(current_block: usize, target_block: usize) -> Self {
        Self::new(
            BackendErrorKind::MissingPhiIncoming,
            format!(
                "PHI for target block {} is missing incoming value from block {}",
                target_block, current_block
            ),
        )
    }

    pub(crate) fn missing_value(value_id: usize) -> Self {
        Self::new(
            BackendErrorKind::MissingValue,
            format!(
                "Value {} not found during backend code generation",
                value_id
            ),
        )
    }

    pub(crate) fn unsupported_host_argument_type(ty: impl fmt::Debug) -> Self {
        Self::new(
            BackendErrorKind::UnsupportedHostArgumentType,
            format!("Unsupported host call argument type {:?}", ty),
        )
    }

    pub(crate) fn unsupported_type(ty: impl fmt::Debug) -> Self {
        Self::new(
            BackendErrorKind::UnsupportedType,
            format!("Unresolved or unsupported IR type {:?}", ty),
        )
    }

    pub(crate) fn unsupported_execution_return_type(ty: impl fmt::Debug) -> Self {
        Self::new(
            BackendErrorKind::UnsupportedExecutionReturnType,
            format!("Execution for return type {:?} is not yet supported", ty),
        )
    }

    pub(crate) fn tensor_ir(message: impl Into<String>) -> Self {
        Self::new(BackendErrorKind::TensorIr, message)
    }
}

impl fmt::Display for BackendCodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for BackendCodegenError {}

pub type BackendResult<T> = Result<T, BackendCodegenError>;
