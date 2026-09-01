//! Shape validation for `espresso_signBatch` payloads (REQ-SIDECAR-S-003).
//!
//! A payload is accepted only if it is an RLP list of exactly four elements
//! whose first element is itself a list, with nothing after the list. No
//! Ethereum signing-preimage matches this shape: a legacy transaction preimage
//! is a list starting with a scalar (the nonce), typed-transaction preimages
//! start with a type byte so they are not RLP lists, and EIP-191/EIP-712
//! messages are not RLP. The four elements are espresso-streamers'
//! `EspressoBatch`: block header (a list), singular batch, L1 info deposit,
//! signer address. The fixture suite encodes the real Go type, so a struct
//! change upstream fails tests here rather than production.

use alloy_rlp::Header;

/// The espresso-streamers version whose `EspressoBatch` encoding this shape check
/// targets. Must equal the pin in `tests/fixtures/gen/go.mod`; CI asserts it. When a
/// new streamer version changes the batch format, bump both together and the sidecar's
/// major version (see the versioning doc).
pub const SUPPORTED_STREAMER_VERSION: &str = "v1.3.0";

/// Pre-decode guard, not the security boundary (mTLS is). 4 MiB comfortably
/// exceeds any real batch: a full L2 block of calldata is under 2 MiB.
pub const MAX_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

/// The smallest payload `validate_batch_shape` accepts: the RLP list `[ [], "", "", "" ]`.
/// A stand-in batch for signing tests, since the sidecar decodes no field.
pub const MINIMAL_VALID_PAYLOAD: [u8; 5] = [0xc4, 0xc0, 0x80, 0x80, 0x80];

/// Checks that `payload` has the shape of an RLP-encoded Espresso batch.
pub fn validate_batch_shape(payload: &[u8]) -> Result<(), String> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(format!(
            "payload of {} bytes exceeds the {MAX_PAYLOAD_BYTES}-byte cap",
            payload.len()
        ));
    }
    let mut buf = payload;
    let outer = Header::decode(&mut buf).map_err(|e| format!("not RLP: {e}"))?;
    if !outer.list {
        return Err("not an RLP list".into());
    }
    // Header::decode leaves `buf` at the list payload; more means trailing data.
    if buf.len() != outer.payload_length {
        return Err("data after the end of the list".into());
    }
    let (mut count, mut first_is_list) = (0usize, false);
    while !buf.is_empty() {
        // decode errors on truncated elements, so the slice below cannot panic.
        let item =
            Header::decode(&mut buf).map_err(|e| format!("element {count} is not RLP: {e}"))?;
        if count == 0 {
            first_is_list = item.list;
        }
        buf = &buf[item.payload_length..];
        count += 1;
    }
    if count != 4 {
        return Err(format!(
            "expected a four-element list, got {count} elements"
        ));
    }
    if !first_is_list {
        return Err("first element is not a list; a batch starts with the block header".into());
    }
    Ok(())
}

// The negative cases (transaction preimages, EIP-191, structural near-misses)
// are covered end-to-end by the Go-generated fixtures in tests/rpc_wire.rs.
// Here: only what fixtures cannot carry.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_valid_shape_accepted() {
        validate_batch_shape(&MINIMAL_VALID_PAYLOAD).unwrap();
    }

    #[test]
    fn oversized_payload_rejected() {
        let err = validate_batch_shape(&vec![0u8; MAX_PAYLOAD_BYTES + 1]).unwrap_err();
        assert!(err.contains("cap"), "{err}");
    }
}
