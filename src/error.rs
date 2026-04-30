use jsonrpsee::types::ErrorObjectOwned;

#[derive(Debug, thiserror::Error)]
pub enum SignerError {
    // caller errors → JSON-RPC -32602 invalid params
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("unsupported transaction type: {0}")]
    UnsupportedTxType(String),

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
            SignerError::MissingField(_) | SignerError::UnsupportedTxType(_) => -32602,
            _ => -32000,
        };
        ErrorObjectOwned::owned(code, e.to_string(), None::<()>)
    }
}
