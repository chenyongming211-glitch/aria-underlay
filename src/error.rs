#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterErrorDetail {
    pub code: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum UnderlayError {
    #[error("device already exists: {0}")]
    DeviceAlreadyExists(String),

    #[error("device not found: {0}")]
    DeviceNotFound(String),

    #[error("invalid device state: {0}")]
    InvalidDeviceState(String),

    #[error("adapter transport error: {0}")]
    AdapterTransport(String),

    #[error("adapter operation error: {code}: {message}")]
    AdapterOperation {
        code: String,
        message: String,
        retryable: bool,
        errors: Vec<AdapterErrorDetail>,
    },

    #[error("invalid intent: {0}")]
    InvalidIntent(String),

    #[error("authorization denied: {0}")]
    AuthorizationDenied(String),

    #[error("product audit write failed: {0}")]
    ProductAuditWriteFailed(String),

    #[error("unsupported transaction strategy")]
    UnsupportedTransactionStrategy,

    #[error("internal error: {0}")]
    Internal(String),
}

pub type UnderlayResult<T> = Result<T, UnderlayError>;
