use alloy::{
    consensus::TypedTransaction,
    eips::eip2718::Encodable2718,
    network::TxSigner,
    primitives::{hex, Address, Bytes, B256},
    signers::{local::PrivateKeySigner, Signer},
};
use espresso_kms_signer::tx_builder::TransactionArgs;
use std::path::Path;

fn fixture_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.json"))
}

fn load<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    let raw =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TxFixture {
    description: String,
    private_key: String,
    input: TransactionArgs,
    expected_rlp: String,
}

async fn run_tx_fixture(path: &Path) {
    let fixture: TxFixture = load(path);

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

    let got = format!(
        "0x{}",
        hex::encode(typed_tx.into_envelope(sig).encoded_2718())
    );
    assert_eq!(
        got, fixture.expected_rlp,
        "{}: RLP mismatch",
        fixture.description
    );
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignFixture {
    description: String,
    private_key: String,
    input: SignFixtureInput,
    expected_sig: String,
}

#[derive(serde::Deserialize)]
struct SignFixtureInput {
    address: Address,
    data: Bytes,
}

async fn run_sign_fixture(path: &Path) {
    let fixture: SignFixture = load(path);

    let signer: PrivateKeySigner = fixture
        .private_key
        .parse()
        .unwrap_or_else(|e| panic!("{}: bad private key: {e}", fixture.description));
    assert_eq!(
        signer.address(),
        fixture.input.address,
        "{}: signer address mismatch",
        fixture.description
    );

    let digest = B256::try_from(fixture.input.data.as_ref())
        .unwrap_or_else(|_| panic!("{}: data is not 32 bytes", fixture.description));

    let sig = Signer::sign_hash(&signer, &digest)
        .await
        .unwrap_or_else(|e| panic!("{}: sign: {e}", fixture.description));

    let got = format!("0x{}", hex::encode(sig.as_rsy()));
    assert_eq!(
        got, fixture.expected_sig,
        "{}: signature mismatch",
        fixture.description
    );
}

macro_rules! tx_fixture_test {
    ($name:ident) => {
        #[tokio::test]
        async fn $name() {
            run_tx_fixture(&fixture_path(stringify!($name))).await;
        }
    };
}

macro_rules! sign_fixture_test {
    ($name:ident) => {
        #[tokio::test]
        async fn $name() {
            run_sign_fixture(&fixture_path(stringify!($name))).await;
        }
    };
}

tx_fixture_test!(eip1559_basic);
tx_fixture_test!(eip1559_with_data);
tx_fixture_test!(eip4844_basic);
sign_fixture_test!(eth_sign_basic);
