use jsonrpsee::types::ErrorObjectOwned;

pub const INVALID_PARAMS: i32 = -32602;
pub const SERVER_ERROR: i32 = -32000;

#[derive(Debug, thiserror::Error)]
pub enum SignerError {
    // caller errors → JSON-RPC -32602 invalid params
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("unsupported transaction type: {0}")]
    UnsupportedTxType(String),
    #[error("invalid field value: {0}")]
    InvalidField(String),

    // server errors → JSON-RPC -32000
    #[error("chain ID mismatch: got {got}, expected {expected}")]
    ChainIdMismatch { got: u64, expected: u64 },
    #[error("`from` mismatch: got {got}, expected {expected}")]
    FromMismatch { got: String, expected: String },
    #[error("`to` address not in allowlist: {0}")]
    ToNotAllowed(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<SignerError> for ErrorObjectOwned {
    fn from(e: SignerError) -> Self {
        let code = match &e {
            SignerError::MissingField(_)
            | SignerError::UnsupportedTxType(_)
            | SignerError::InvalidField(_) => INVALID_PARAMS,
            _ => SERVER_ERROR,
        };
        ErrorObjectOwned::owned(code, e.to_string(), None::<()>)
    }
}
