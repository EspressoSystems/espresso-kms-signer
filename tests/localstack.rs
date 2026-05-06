/// End-to-end signing test against a real KMS (localstack).
///
/// Requires localstack running with a secp256k1 key provisioned.
/// Set the following env vars before running:
///
///   AWS_KMS_KEY_ID=<key-id>
///   AWS_ENDPOINT_URL=http://localhost:4566
///   AWS_DEFAULT_REGION=us-east-2
///   AWS_ACCESS_KEY_ID=test
///   AWS_SECRET_ACCESS_KEY=test
///
/// Then run with:
///
///   cargo test --test localstack -- --include-ignored
///
use std::sync::Arc;

use alloy::{
    eips::eip2718::Decodable2718,
    primitives::{address, U256},
};
use espresso_kms_signer::{
    rpc::{SignerRpcServer, SignerServer},
    signer::KmsSigner,
    tx_builder::TransactionArgs,
    Config,
};

fn localstack_config() -> Option<(String, u64)> {
    let key_id = std::env::var("AWS_KMS_KEY_ID").ok()?;
    // Only run if explicitly pointing at a local endpoint.
    std::env::var("AWS_ENDPOINT_URL").ok()?;
    Some((key_id, 11155111))
}

#[tokio::test]
#[ignore = "requires localstack with a secp256k1 KMS key (see file header)"]
async fn localstack_signs_eip1559_transaction() {
    let (key_id, chain_id) =
        localstack_config().expect("AWS_KMS_KEY_ID and AWS_ENDPOINT_URL must be set");

    let signer = Arc::new(
        KmsSigner::new(key_id, chain_id)
            .await
            .expect("failed to connect to localstack KMS"),
    );

    let config = Arc::new(Config {
        kms_key_id: "localstack".into(),
        chain_id,
        listen_addr: "127.0.0.1:0".parse().unwrap(),
        allowed_to: None,
        tls: None,
    });

    let srv = SignerServer::new(signer.clone(), config);

    let args = TransactionArgs {
        from: Some(signer.address),
        to: Some(address!("ff00000000000000000000000000000011155111")),
        gas: Some(U256::from(21000u64)),
        gas_price: None,
        max_fee_per_gas: Some(U256::from(1_000_000_000u64)),
        max_priority_fee_per_gas: Some(U256::from(1_000_000u64)),
        value: Some(U256::from(1u64)),
        nonce: Some(U256::from(0u64)),
        data: None,
        chain_id: Some(U256::from(chain_id)),
        access_list: None,
        blob_fee_cap: None,
        blob_hashes: None,
    };

    let result = srv
        .eth_sign_transaction(args)
        .await
        .expect("signing should succeed");

    let bytes = alloy::primitives::hex::decode(&result[2..]).unwrap();
    let tx =
        alloy::consensus::TxEnvelope::decode_2718_exact(&bytes).expect("invalid EIP-2718 encoding");

    let alloy::consensus::TxEnvelope::Eip1559(signed) = tx else {
        panic!("expected EIP-1559 transaction");
    };

    // Verify fields round-trip correctly.
    let inner = signed.tx();
    assert_eq!(inner.chain_id, chain_id);
    assert_eq!(inner.nonce, 0);
    assert_eq!(inner.gas_limit, 21000);
    assert_eq!(inner.value, alloy::primitives::U256::from(1u64));

    // Verify the recovered signer matches the KMS-derived address.
    use alloy::consensus::SignableTransaction;
    let recovered = signed
        .signature()
        .recover_address_from_prehash(&signed.tx().signature_hash())
        .expect("failed to recover signer");
    assert_eq!(recovered, signer.address);
}
