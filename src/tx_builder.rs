use alloy::{
    consensus::{TxEip1559, TxEip4844, TxEip4844Variant, TypedTransaction},
    eips::eip2930::AccessList,
    primitives::{Address, Bytes, TxKind, B256, U256},
};
use serde::Deserialize;

use crate::error::SignerError;

/// Wire shape of `eth_signTransaction` — matches go-ethereum's `TransactionArgs` JSON encoding.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionArgs {
    pub from: Option<Address>,
    pub to: Option<Address>,
    pub gas: Option<U256>,
    pub gas_price: Option<U256>, // accepted for go-ethereum wire compatibility; ignored (legacy txs unsupported)
    pub max_fee_per_gas: Option<U256>,
    pub max_priority_fee_per_gas: Option<U256>,
    pub value: Option<U256>,
    pub nonce: Option<U256>,
    pub data: Option<Bytes>,
    pub chain_id: Option<U256>,
    pub access_list: Option<AccessList>,
    // EIP-4844
    pub blob_fee_cap: Option<U256>,
    pub blob_hashes: Option<Vec<B256>>,
}

/// Fields shared by every transaction type we support.
/// Excludes fee fields — those are validated after type detection,
/// since their absence is what signals an unsupported tx type.
struct CommonFields {
    chain_id: u64,
    nonce: u64,
    gas_limit: u64,
    value: U256,
    input: Bytes,
    access_list: AccessList,
}

impl TransactionArgs {
    pub fn into_typed_transaction(self) -> Result<TypedTransaction, SignerError> {
        // Detect type first — absence of fee fields means unsupported, not missing.
        let is_blob = self.blob_hashes.is_some() || self.blob_fee_cap.is_some();
        let is_1559 = self.max_fee_per_gas.is_some() || self.max_priority_fee_per_gas.is_some();

        if !is_blob && !is_1559 {
            return Err(SignerError::UnsupportedTxType(
                "only EIP-1559 and EIP-4844 are supported; use maxFeePerGas or blobFeeCap".into(),
            ));
        }

        let common = CommonFields {
            chain_id: parse_field(self.chain_id, "chainId")?,
            nonce: parse_field(self.nonce, "nonce")?,
            gas_limit: parse_field(self.gas, "gas")?,
            value: self.value.unwrap_or_default(),
            input: self.data.unwrap_or_default(),
            access_list: self.access_list.unwrap_or_default(),
        };

        let max_fee_per_gas: u128 = parse_field(self.max_fee_per_gas, "maxFeePerGas")?;
        let max_priority_fee_per_gas: u128 =
            parse_field(self.max_priority_fee_per_gas, "maxPriorityFeePerGas")?;

        if is_blob {
            Ok(TypedTransaction::Eip4844(TxEip4844Variant::TxEip4844(
                TxEip4844 {
                    chain_id: common.chain_id,
                    nonce: common.nonce,
                    gas_limit: common.gas_limit,
                    max_fee_per_gas,
                    max_priority_fee_per_gas,
                    to: self.to.ok_or(SignerError::MissingField("to"))?,
                    value: common.value,
                    input: common.input,
                    access_list: common.access_list,
                    max_fee_per_blob_gas: parse_field(self.blob_fee_cap, "blobFeeCap")?,
                    blob_versioned_hashes: self.blob_hashes.unwrap_or_default(),
                },
            )))
        } else {
            Ok(TypedTransaction::Eip1559(TxEip1559 {
                chain_id: common.chain_id,
                nonce: common.nonce,
                gas_limit: common.gas_limit,
                max_fee_per_gas,
                max_priority_fee_per_gas,
                to: TxKind::from(self.to),
                value: common.value,
                input: common.input,
                access_list: common.access_list,
            }))
        }
    }
}

fn parse_field<T: TryFrom<U256>>(val: Option<U256>, field: &'static str) -> Result<T, SignerError> {
    val.ok_or(SignerError::MissingField(field))?
        .try_into()
        .map_err(|_| SignerError::Internal(format!("{field} overflow")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::address;

    fn base_1559() -> TransactionArgs {
        TransactionArgs {
            from: Some(address!("0000000000000000000000000000000000000001")),
            to: Some(address!("0000000000000000000000000000000000000002")),
            gas: Some(U256::from(21000u64)),
            gas_price: None,
            max_fee_per_gas: Some(U256::from(1_000_000_000u64)),
            max_priority_fee_per_gas: Some(U256::from(1_000_000u64)),
            value: Some(U256::from(1u64)),
            nonce: Some(U256::from(7u64)),
            data: Some(Bytes::from_static(b"\xde\xad\xbe\xef")),
            chain_id: Some(U256::from(11155111u64)),
            access_list: None,
            blob_fee_cap: None,
            blob_hashes: None,
        }
    }

    #[test]
    fn eip1559_fields_mapped_correctly() {
        let TypedTransaction::Eip1559(tx) = base_1559().into_typed_transaction().unwrap() else {
            panic!("expected EIP-1559 transaction");
        };
        assert_eq!(tx.chain_id, 11155111);
        assert_eq!(tx.nonce, 7);
        assert_eq!(tx.gas_limit, 21000);
        assert_eq!(tx.max_fee_per_gas, 1_000_000_000);
        assert_eq!(tx.max_priority_fee_per_gas, 1_000_000);
        assert_eq!(tx.value, U256::from(1u64));
        assert_eq!(tx.input.as_ref(), b"\xde\xad\xbe\xef");
        assert_eq!(
            tx.to,
            TxKind::Call(address!("0000000000000000000000000000000000000002"))
        );
    }

    #[test]
    fn eip4844_fields_mapped_correctly() {
        let blob_hash = B256::repeat_byte(0xab);
        let mut args = base_1559();
        args.blob_fee_cap = Some(U256::from(500u64));
        args.blob_hashes = Some(vec![blob_hash]);

        let TypedTransaction::Eip4844(alloy::consensus::TxEip4844Variant::TxEip4844(tx)) =
            args.into_typed_transaction().unwrap()
        else {
            panic!("expected EIP-4844 transaction");
        };
        assert_eq!(tx.chain_id, 11155111);
        assert_eq!(tx.nonce, 7);
        assert_eq!(tx.gas_limit, 21000);
        assert_eq!(tx.max_fee_per_gas, 1_000_000_000);
        assert_eq!(tx.max_priority_fee_per_gas, 1_000_000);
        assert_eq!(tx.max_fee_per_blob_gas, 500);
        assert_eq!(tx.blob_versioned_hashes, vec![blob_hash]);
        assert_eq!(tx.to, address!("0000000000000000000000000000000000000002"));
    }

    #[test]
    fn missing_chain_id_rejected() {
        let mut args = base_1559();
        args.chain_id = None;
        assert!(matches!(
            args.into_typed_transaction(),
            Err(SignerError::MissingField("chainId"))
        ));
    }

    #[test]
    fn missing_gas_rejected() {
        let mut args = base_1559();
        args.gas = None;
        assert!(matches!(
            args.into_typed_transaction(),
            Err(SignerError::MissingField("gas"))
        ));
    }

    #[test]
    fn missing_nonce_rejected() {
        let mut args = base_1559();
        args.nonce = None;
        assert!(matches!(
            args.into_typed_transaction(),
            Err(SignerError::MissingField("nonce"))
        ));
    }

    #[test]
    fn legacy_tx_rejected() {
        let mut args = base_1559();
        args.max_fee_per_gas = None;
        args.max_priority_fee_per_gas = None;
        assert!(matches!(
            args.into_typed_transaction(),
            Err(SignerError::UnsupportedTxType(_))
        ));
    }

    #[test]
    fn blob_tx_missing_to_rejected() {
        let mut args = base_1559();
        args.blob_fee_cap = Some(U256::from(500u64));
        args.blob_hashes = Some(vec![B256::ZERO]);
        args.to = None;
        assert!(matches!(
            args.into_typed_transaction(),
            Err(SignerError::MissingField("to"))
        ));
    }
}
