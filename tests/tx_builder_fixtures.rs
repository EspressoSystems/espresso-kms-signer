use alloy::{
    consensus::TypedTransaction,
    eips::eip2718::Encodable2718,
    network::TxSigner,
    primitives::hex,
    signers::local::PrivateKeySigner,
};
use espresso_kms_signer::tx_builder::TransactionArgs;
use std::path::Path;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    description: String,
    private_key: String,
    input: TransactionArgs,
    expected_rlp: String,
}

async fn run_fixture(path: &Path) {
    let raw =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let fixture: Fixture =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));

    let signer: PrivateKeySigner = fixture
        .private_key
        .parse()
        .unwrap_or_else(|e| panic!("{}: bad private key: {e}", fixture.description));

    let mut typed_tx: TypedTransaction = fixture
        .input
        .into_typed_transaction()
        .unwrap_or_else(|e| panic!("{}: into_typed_transaction: {e}", fixture.description));

    let sig = signer
        .sign_transaction(&mut typed_tx)
        .await
        .unwrap_or_else(|e| panic!("{}: sign: {e}", fixture.description));

    let got = format!("0x{}", hex::encode(typed_tx.into_envelope(sig).encoded_2718()));
    assert_eq!(got, fixture.expected_rlp, "{}: RLP mismatch", fixture.description);
}

macro_rules! fixture_test {
    ($name:ident) => {
        #[tokio::test]
        async fn $name() {
            run_fixture(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures")
                    .join(concat!(stringify!($name), ".json")),
            )
            .await;
        }
    };
}

fixture_test!(eip1559_basic);
fixture_test!(eip1559_with_data);
fixture_test!(eip4844_basic);
