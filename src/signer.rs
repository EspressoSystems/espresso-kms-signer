use alloy::{
    consensus::TypedTransaction,
    network::TxSigner,
    primitives::{Address, Signature},
    signers::aws::AwsSigner,
};
use aws_config::BehaviorVersion;
use eyre::Result;
use tracing::info;

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
        Ok(Self { address, inner: signer })
    }

    pub async fn sign_transaction(
        &self,
        tx: &mut TypedTransaction,
    ) -> Result<Signature, alloy::signers::Error> {
        self.inner.sign_transaction(tx).await
    }
}
