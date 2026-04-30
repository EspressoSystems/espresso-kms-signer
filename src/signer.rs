use alloy::{
    consensus::TypedTransaction,
    network::TxSigner,
    primitives::{Address, Signature},
    signers::aws::AwsSigner,
};
use aws_config::BehaviorVersion;
use eyre::Result;
use jsonrpsee::core::async_trait;
use tracing::info;

/// Signing capability required by the RPC layer.
/// Decoupled from KMS so the RPC layer can be tested without AWS.
#[async_trait]
pub trait Signer: Send + Sync + 'static {
    fn address(&self) -> Address;
    async fn sign_transaction(
        &self,
        tx: &mut TypedTransaction,
    ) -> Result<Signature, alloy::signers::Error>;
}

/// Thin wrapper around `AwsSigner` — the only module that touches signing
/// primitives. The private key is never materialised; KMS performs all signing.
pub struct KmsSigner {
    pub address: Address,
    inner: AwsSigner,
}

impl KmsSigner {
    pub async fn new(key_id: String, chain_id: u64) -> Result<Self> {
        let config = aws_config::defaults(BehaviorVersion::latest()).load().await;
        let client = aws_sdk_kms::Client::new(&config);
        let signer = AwsSigner::new(client, key_id, Some(chain_id)).await?;
        let address = TxSigner::address(&signer);
        info!(address = %address, "derived signing address from KMS public key");
        Ok(Self {
            address,
            inner: signer,
        })
    }
}

#[async_trait]
impl Signer for KmsSigner {
    fn address(&self) -> Address {
        self.address
    }

    async fn sign_transaction(
        &self,
        tx: &mut TypedTransaction,
    ) -> Result<Signature, alloy::signers::Error> {
        self.inner.sign_transaction(tx).await
    }
}
