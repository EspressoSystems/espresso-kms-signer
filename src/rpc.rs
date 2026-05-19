//! JSON-RPC surface consumed by op-batcher's `SignerClient`.
//!
//! Sources (optimism-espresso-integration):
//! - op-service/signer/client.go      → `health_status`, `eth_signTransaction`
//! - op-service/signer/espresso.go    → `eth_sign` (Espresso batch auth)
//!
//! `opsigner_signBlockPayload[V2]` also exist on `SignerClient` but are only
//! invoked by op-node/op-proposer, so they are intentionally out of scope here.

use std::sync::Arc;

use alloy::{
    eips::eip2718::Encodable2718,
    primitives::{hex, Address, B256},
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use jsonrpsee::{
    core::{async_trait, RpcResult},
    proc_macros::rpc,
    types::ErrorObjectOwned,
};
use tracing::{debug, error, info, warn};

use crate::{error::SignerError, signer::Signer, tx_builder::TransactionArgs, Config};

#[rpc(server)]
pub trait SignerRpc {
    /// Returns the signer version. Matches the op-signer convention so that
    /// op-batcher's `pingVersion` ( `var v string` ) can unmarshal the response.
    #[method(name = "health_status")]
    async fn health_status(&self) -> RpcResult<String>;

    /// Returns the Ethereum address managed by this signer.
    #[method(name = "signer_address")]
    async fn signer_address(&self) -> RpcResult<String>;

    #[method(name = "eth_signTransaction")]
    async fn eth_sign_transaction(&self, args: TransactionArgs) -> RpcResult<String>;

    /// Sign a 32-byte digest. Non-standard `eth_sign`: signs the input directly
    /// (no Ethereum message prefix), matching op-batcher's Espresso batch-auth path.
    /// `data` is base64 — Go's default JSON encoding of `[]byte`, what
    /// op-service/signer puts on the wire.
    #[method(name = "eth_sign")]
    async fn eth_sign(&self, address: Address, data: String) -> RpcResult<String>;
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
    async fn health_status(&self) -> RpcResult<String> {
        debug!("health check");
        Ok(env!("CARGO_PKG_VERSION").to_string())
    }

    async fn signer_address(&self) -> RpcResult<String> {
        Ok(self.signer.address().to_string())
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
                ErrorObjectOwned::from(SignerError::InvalidField("chainId overflow".into()))
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
                ErrorObjectOwned::owned(
                    crate::error::SERVER_ERROR,
                    "KMS signing failed",
                    None::<()>,
                )
            })?;

        let envelope = typed_tx.into_envelope(sig);
        info!(tx_hash = %envelope.tx_hash(), to = %log_to, nonce = %log_nonce, "transaction signed successfully");

        Ok(format!("0x{}", hex::encode(envelope.encoded_2718())))
    }

    async fn eth_sign(&self, address: Address, data: String) -> RpcResult<String> {
        // `address` is on the wire per op-batcher's signer protocol; validated
        // here to catch --signer.address misconfiguration before any KMS call.
        if address != self.signer.address() {
            return Err(ErrorObjectOwned::from(SignerError::FromMismatch {
                got: format!("{address:#x}"),
                expected: format!("{:#x}", self.signer.address()),
            }));
        }
        let raw = BASE64.decode(data.as_bytes()).map_err(|e| {
            ErrorObjectOwned::from(SignerError::InvalidField(format!(
                "eth_sign: invalid base64: {e}"
            )))
        })?;
        let digest = B256::try_from(raw.as_slice()).map_err(|_| {
            ErrorObjectOwned::from(SignerError::InvalidField(format!(
                "eth_sign expects a 32-byte digest, got {} bytes",
                raw.len()
            )))
        })?;
        info!(address = %address, digest = %digest, "signing request received, calling KMS");
        let sig = self.signer.sign_hash(&digest).await.map_err(|err| {
            error!(error = %err, "KMS digest signing failed");
            ErrorObjectOwned::owned(crate::error::SERVER_ERROR, "KMS signing failed", None::<()>)
        })?;
        // `as_rsy` emits r||s||y_parity (0/1), matching go-ethereum's `crypto.Sign`
        // — the format op-batcher's `crypto.SigToPub` verify path expects.
        info!(address = %address, digest = %digest, "digest signed successfully");
        Ok(format!("0x{}", hex::encode(sig.as_rsy())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::{
        consensus::TypedTransaction,
        primitives::{address, TxKind, U256},
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
        async fn sign_hash(
            &self,
            hash: &B256,
        ) -> Result<alloy::primitives::Signature, alloy::signers::Error> {
            alloy::signers::Signer::sign_hash(&self.0, hash).await
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
            input: None,
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
        use alloy::{
            consensus::{SignableTransaction, TxEnvelope},
            eips::{
                eip2718::Decodable2718,
                eip2930::{AccessList, AccessListItem},
            },
        };

        let allowed = address!("ff00000000000000000000000000000011155111");
        let signer = Arc::new(TestSigner::new());
        let config = test_config(Some(vec![allowed]));
        let srv = SignerServer::new(signer.clone(), config);
        let from = signer.address();

        let mut args = base_args(from);
        args.access_list = Some(AccessList(vec![AccessListItem {
            address: address!("0000000000000000000000000000000000000003"),
            storage_keys: vec![alloy::primitives::B256::ZERO],
        }]));

        let result = srv.eth_sign_transaction(args).await.unwrap();
        let bytes = hex::decode(&result[2..]).unwrap();
        let tx = TxEnvelope::decode_2718_exact(&bytes).expect("invalid EIP-2718 encoding");

        let TxEnvelope::Eip1559(signed) = tx else {
            panic!("expected EIP-1559 transaction");
        };

        let inner = signed.tx();
        assert_eq!(inner.chain_id, CHAIN_ID);
        assert_eq!(inner.nonce, 0);
        assert_eq!(inner.gas_limit, 21000);
        assert_eq!(inner.max_fee_per_gas, 1_000_000_000);
        assert_eq!(inner.max_priority_fee_per_gas, 1_000_000);
        assert_eq!(inner.value, U256::from(1u64));
        assert_eq!(inner.to, TxKind::Call(allowed));
        assert_eq!(inner.access_list.0.len(), 1);
        assert_eq!(
            inner.access_list.0[0].address,
            address!("0000000000000000000000000000000000000003")
        );
        assert_eq!(
            inner.access_list.0[0].storage_keys,
            vec![alloy::primitives::B256::ZERO]
        );

        let recovered = signed
            .signature()
            .recover_address_from_prehash(&signed.tx().signature_hash())
            .expect("failed to recover signer");
        assert_eq!(recovered, from);
    }

    #[tokio::test]
    async fn eth_sign_recovers_signer_address() {
        let srv = server();
        let from = srv.signer.address();
        let digest = B256::from_slice(&alloy::primitives::keccak256(b"hello").0);

        let sig_hex = srv
            .eth_sign(from, BASE64.encode(digest.as_slice()))
            .await
            .unwrap();

        let sig_bytes = hex::decode(&sig_hex[2..]).unwrap();
        assert_eq!(sig_bytes.len(), 65);
        let sig = alloy::primitives::Signature::try_from(sig_bytes.as_slice()).unwrap();
        let recovered = sig.recover_address_from_prehash(&digest).unwrap();
        assert_eq!(recovered, from);
    }

    #[tokio::test]
    async fn eth_sign_rejects_wrong_address() {
        let srv = server();
        let digest = B256::ZERO;
        let err = srv
            .eth_sign(
                address!("0000000000000000000000000000000000000001"),
                BASE64.encode(digest.as_slice()),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code(), -32000);
        assert!(err.message().contains("`from` mismatch"));
    }

    #[tokio::test]
    async fn eth_sign_rejects_non_32_byte_input() {
        let srv = server();
        let err = srv
            .eth_sign(srv.signer.address(), BASE64.encode(b"too short"))
            .await
            .unwrap_err();
        assert_eq!(err.code(), -32602);
        assert!(err.message().contains("32-byte"));
    }

    #[tokio::test]
    async fn eth_sign_rejects_invalid_base64() {
        let srv = server();
        let err = srv
            .eth_sign(srv.signer.address(), "!!!not base64!!!".to_string())
            .await
            .unwrap_err();
        assert_eq!(err.code(), -32602);
        assert!(err.message().contains("invalid base64"));
    }
}
