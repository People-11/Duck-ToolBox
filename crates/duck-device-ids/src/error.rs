use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeviceIdsError {
    #[error("device field `{0}` is required before provisioning")]
    MissingField(&'static str),
    #[error("device ID backend is unavailable: {0}")]
    Unsupported(&'static str),
    #[error("invalid device ID value for {0}")]
    InvalidValue(String),
    #[error("TrustKernel worker failed: {0}")]
    Transport(String),
    #[error("another device ID provisioning operation is running")]
    Busy,
    #[error("KPH operation timed out; the attempted write may already have reached secure storage")]
    Timeout,
    #[error("KPH {stage} failed for {tag} (status {status}); secure storage may have changed")]
    Kph {
        stage: &'static str,
        tag: String,
        status: String,
    },
}

impl DeviceIdsError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingField(_) => "missing_device_field",
            Self::Unsupported(_) => "unsupported_device",
            Self::InvalidValue(_) => "invalid_device_field",
            Self::Transport(_) => "trustkernel_transport_failed",
            Self::Busy => "provisioning_busy",
            Self::Timeout => "provisioning_timeout",
            Self::Kph { .. } => "trustkernel_operation_failed",
        }
    }
}

pub(crate) fn code_of(error: &anyhow::Error) -> &'static str {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<DeviceIdsError>())
        .map_or("internal_error", DeviceIdsError::code)
}
