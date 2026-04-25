#[derive(Debug, thiserror::Error)]
pub enum UnderlayError {
    #[error("device already exists: {0}")]
    DeviceAlreadyExists(String),

    #[error("device not found: {0}")]
    DeviceNotFound(String),

    #[error("invalid device state: {0}")]
    InvalidDeviceState(String),

    #[error("adapter error: {0}")]
    Adapter(String),

    #[error("unsupported transaction strategy")]
    UnsupportedTransactionStrategy,

    #[error("internal error: {0}")]
    Internal(String),
}

pub type UnderlayResult<T> = Result<T, UnderlayError>;

