use std::sync::Arc;

use alloy::{eips::eip2718::Encodable2718, primitives::hex};
use jsonrpsee::{
    core::{async_trait, RpcResult},
    proc_macros::rpc,
    types::ErrorObjectOwned,
};
use tracing::{debug, error, info, warn};

use crate::{error::SignerError, signer::Signer, tx_builder::TransactionArgs, Config};

#[rpc(server)]
pub trait SignerRpc {
    #[method(name = "health_status")]
    async fn health_status(&self) -> RpcResult<serde_json::Value>;

    #[method(name = "eth_signTransaction")]
    async fn eth_sign_transaction(&self, args: TransactionArgs) -> RpcResult<String>;
}

pub struct SignerServer<S> {
    signer: Arc<S>,
    config: Arc<Config>,
}

impl<S: Signer> SignerServer<S> {
    pub fn new(signer: Arc<S>, config: Arc<Config>) -> Self {
        Self { signer, config }
    }
}

#[async_trait]
impl<S: Signer> SignerRpcServer for SignerServer<S> {
    async fn health_status(&self) -> RpcResult<serde_json::Value> {
        debug!("health check");
        Ok(serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "status": "ok"
        }))
    }

    async fn eth_sign_transaction(&self, args: TransactionArgs) -> RpcResult<String> {
        let log_to = args
            .to
            .map(|a| format!("{a:#x}"))
            .unwrap_or_else(|| "none".into());
        let log_nonce = args.nonce.unwrap_or_default();

        let chain_id: u64 = args
            .chain_id
            .ok_or_else(|| ErrorObjectOwned::from(SignerError::MissingField("chainId")))?
            .try_into()
            .map_err(|_| {
                ErrorObjectOwned::from(SignerError::Internal("chainId overflow".into()))
            })?;

        if chain_id != self.config.chain_id {
            let e = SignerError::ChainIdMismatch {
                got: chain_id,
                expected: self.config.chain_id,
            };
            warn!(error = %e, to = %log_to, nonce = %log_nonce);
            return Err(ErrorObjectOwned::from(e));
        }

        let from = args
            .from
            .ok_or_else(|| ErrorObjectOwned::from(SignerError::MissingField("from")))?;
        if from != self.signer.address() {
            let e = SignerError::FromMismatch {
                got: format!("{from:#x}"),
                expected: format!("{:#x}", self.signer.address()),
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
                ErrorObjectOwned::owned(-32000, "KMS signing failed", None::<()>)
            })?;

        let envelope = typed_tx.into_envelope(sig);
        info!(tx_hash = %envelope.tx_hash(), to = %log_to, nonce = %log_nonce, "transaction signed successfully");

        Ok(format!("0x{}", hex::encode(envelope.encoded_2718())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::{
        consensus::TypedTransaction,
        primitives::{address, U256},
    };
    use std::net::SocketAddr;

    const CHAIN_ID: u64 = 11155111;
    const TEST_KEY: &str = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    // A signer whose address is derived from the well-known Hardhat test key.
    // sign_transaction delegates to PrivateKeySigner so signing tests produce real RLP.
    struct TestSigner(alloy::signers::local::PrivateKeySigner);

    impl TestSigner {
        fn new() -> Self {
            Self(TEST_KEY.parse().unwrap())
        }
    }

    #[async_trait]
    impl Signer for TestSigner {
        fn address(&self) -> alloy::primitives::Address {
            alloy::network::TxSigner::address(&self.0)
        }
        async fn sign_transaction(
            &self,
            tx: &mut TypedTransaction,
        ) -> Result<alloy::primitives::Signature, alloy::signers::Error> {
            alloy::network::TxSigner::sign_transaction(&self.0, tx).await
        }
    }

    fn test_config(allowed_to: Option<Vec<alloy::primitives::Address>>) -> Arc<Config> {
        Arc::new(Config {
            kms_key_id: "test".into(),
            chain_id: CHAIN_ID,
            listen_addr: "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
            allowed_to,
            tls: None,
        })
    }

    fn server() -> SignerServer<TestSigner> {
        let signer = Arc::new(TestSigner::new());
        SignerServer::new(signer, test_config(None))
    }

    fn base_args(from: alloy::primitives::Address) -> TransactionArgs {
        TransactionArgs {
            from: Some(from),
            to: Some(address!("ff00000000000000000000000000000011155111")),
            gas: Some(U256::from(21000u64)),
            gas_price: None,
            max_fee_per_gas: Some(U256::from(1_000_000_000u64)),
            max_priority_fee_per_gas: Some(U256::from(1_000_000u64)),
            value: Some(U256::from(1u64)),
            nonce: Some(U256::from(0u64)),
            data: None,
            chain_id: Some(U256::from(CHAIN_ID)),
            access_list: None,
            blob_fee_cap: None,
            blob_hashes: None,
        }
    }

    #[tokio::test]
    async fn missing_chain_id_rejected() {
        let srv = server();
        let mut args = base_args(srv.signer.address());
        args.chain_id = None;
        let err = srv.eth_sign_transaction(args).await.unwrap_err();
        assert_eq!(err.code(), -32602);
        assert!(err.message().contains("chainId"));
    }

    #[tokio::test]
    async fn chain_id_mismatch_rejected() {
        let srv = server();
        let mut args = base_args(srv.signer.address());
        args.chain_id = Some(U256::from(1u64)); // mainnet, not Sepolia
        let err = srv.eth_sign_transaction(args).await.unwrap_err();
        assert_eq!(err.code(), -32000);
        assert!(err.message().contains("chain ID mismatch"));
    }

    #[tokio::test]
    async fn missing_from_rejected() {
        let srv = server();
        let mut args = base_args(srv.signer.address());
        args.from = None;
        let err = srv.eth_sign_transaction(args).await.unwrap_err();
        assert_eq!(err.code(), -32602);
        assert!(err.message().contains("from"));
    }

    #[tokio::test]
    async fn from_mismatch_rejected() {
        let srv = server();
        let args = base_args(address!("0000000000000000000000000000000000000001"));
        let err = srv.eth_sign_transaction(args).await.unwrap_err();
        assert_eq!(err.code(), -32000);
        assert!(err.message().contains("`from` mismatch"));
    }

    #[tokio::test]
    async fn to_not_in_allowlist_rejected() {
        let signer = Arc::new(TestSigner::new());
        let allowed = address!("ff00000000000000000000000000000011155111");
        let other = address!("0000000000000000000000000000000000000002");
        let config = test_config(Some(vec![allowed]));
        let srv = SignerServer::new(signer.clone(), config);
        let mut args = base_args(signer.address());
        args.to = Some(other);
        let err = srv.eth_sign_transaction(args).await.unwrap_err();
        assert_eq!(err.code(), -32000);
        assert!(err.message().contains("not in allowlist"));
    }

    #[tokio::test]
    async fn missing_to_when_allowlist_configured_rejected() {
        let signer = Arc::new(TestSigner::new());
        let allowed = address!("ff00000000000000000000000000000011155111");
        let config = test_config(Some(vec![allowed]));
        let srv = SignerServer::new(signer.clone(), config);
        let mut args = base_args(signer.address());
        args.to = None;
        let err = srv.eth_sign_transaction(args).await.unwrap_err();
        assert_eq!(err.code(), -32602);
        assert!(err.message().contains("to"));
    }

    #[tokio::test]
    async fn valid_request_returns_signed_rlp() {
        let srv = server();
        let args = base_args(srv.signer.address());
        let result = srv.eth_sign_transaction(args).await.unwrap();
        assert!(result.starts_with("0x"));
        // Must decode as a valid EIP-2718 transaction.
        let bytes = hex::decode(&result[2..]).unwrap();
        assert!(!bytes.is_empty());
        assert_eq!(bytes[0], 0x02); // EIP-1559 type byte
    }

    #[tokio::test]
    async fn allowlist_passes_when_to_matches() {
        let signer = Arc::new(TestSigner::new());
        let allowed = address!("ff00000000000000000000000000000011155111");
        let config = test_config(Some(vec![allowed]));
        let srv = SignerServer::new(signer.clone(), config);
        let args = base_args(signer.address());
        assert!(srv.eth_sign_transaction(args).await.is_ok());
    }
}
