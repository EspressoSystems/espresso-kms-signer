use std::sync::Arc;

use alloy::{
    eips::eip2718::Encodable2718,
    primitives::hex,
};
use jsonrpsee::{
    core::{async_trait, RpcResult},
    proc_macros::rpc,
    types::ErrorObjectOwned,
};
use tracing::{error, info, warn};

use crate::{
    error::SignerError,
    signer::KmsSigner,
    tx_builder::TransactionArgs,
    Config,
};

#[rpc(server)]
pub trait SignerRpc {
    #[method(name = "health_status")]
    async fn health_status(&self) -> RpcResult<serde_json::Value>;

    #[method(name = "eth_signTransaction")]
    async fn eth_sign_transaction(&self, args: TransactionArgs) -> RpcResult<String>;
}

pub struct SignerServer {
    signer: Arc<KmsSigner>,
    config: Arc<Config>,
}

impl SignerServer {
    pub fn new(signer: Arc<KmsSigner>, config: Arc<Config>) -> Self {
        Self { signer, config }
    }
}

#[async_trait]
impl SignerRpcServer for SignerServer {
    async fn health_status(&self) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "status": "ok"
        }))
    }

    async fn eth_sign_transaction(&self, args: TransactionArgs) -> RpcResult<String> {
        let log_to = args.to.map(|a| format!("{a:#x}")).unwrap_or_else(|| "none".into());
        let log_nonce = args.nonce.unwrap_or_default();

        let chain_id: u64 = args
            .chain_id
            .ok_or_else(|| ErrorObjectOwned::from(SignerError::MissingField("chainId")))?
            .try_into()
            .map_err(|_| {
                ErrorObjectOwned::from(SignerError::InvalidField("chainId overflow".into()))
            })?;

        if chain_id != self.config.chain_id {
            let e = SignerError::ChainIdMismatch { got: chain_id, expected: self.config.chain_id };
            warn!(error = %e, to = %log_to, nonce = %log_nonce);
            return Err(ErrorObjectOwned::from(e));
        }

        let from = args
            .from
            .ok_or_else(|| ErrorObjectOwned::from(SignerError::MissingField("from")))?;
        if from != self.signer.address {
            let e = SignerError::FromMismatch {
                got: format!("{from:#x}"),
                expected: format!("{:#x}", self.signer.address),
            };
            warn!(error = %e, to = %log_to, nonce = %log_nonce);
            return Err(ErrorObjectOwned::from(e));
        }

        if let Some(allowed) = &self.config.allowed_to {
            let to = args
                .to
                .ok_or_else(|| ErrorObjectOwned::from(SignerError::MissingField("to")))?;
            if !allowed.contains(&to) {
                let e = SignerError::ToNotAllowed(format!("{to:#x}"));
                warn!(error = %e, nonce = %log_nonce);
                return Err(ErrorObjectOwned::from(e));
            }
        }

        info!(to = %log_to, nonce = %log_nonce, "signing request received, calling KMS");

        let mut typed_tx = args
            .into_typed_transaction()
            .map_err(ErrorObjectOwned::from)?;

        let sig = self
            .signer
            .sign_transaction(&mut typed_tx)
            .await
            .map_err(|err| {
                error!(error = %err, to = %log_to, nonce = %log_nonce, "KMS signing failed");
                ErrorObjectOwned::owned(crate::error::SERVER_ERROR, "KMS signing failed", None::<()>)
            })?;

        let envelope = typed_tx.into_envelope(sig);
        info!(tx_hash = %envelope.tx_hash(), to = %log_to, nonce = %log_nonce, "transaction signed successfully");

        Ok(format!("0x{}", hex::encode(envelope.encoded_2718())))
    }
}
