pub mod error;
pub mod rpc;
pub mod signer;
pub mod tx_builder;

use std::net::SocketAddr;

use alloy::primitives::Address;
use eyre::{Context, Result};

#[derive(Debug)]
pub struct Config {
    pub kms_key_id: String,
    pub chain_id: u64,
    pub listen_addr: SocketAddr,
    pub allowed_to: Option<Vec<Address>>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let kms_key_id = std::env::var("AWS_KMS_KEY_ID")
            .wrap_err("AWS_KMS_KEY_ID must be set")?;
        let chain_id: u64 = std::env::var("CHAIN_ID")
            .wrap_err("CHAIN_ID must be set")?
            .parse()
            .wrap_err("CHAIN_ID must be a positive integer")?;
        let listen_addr: SocketAddr = std::env::var("LISTEN_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8547".into())
            .parse()
            .wrap_err("LISTEN_ADDR must be a valid socket address")?;
        let allowed_to = std::env::var("ALLOWED_TO").ok().map(|s| {
            s.split(',')
                .filter_map(|a| a.trim().parse::<Address>().ok())
                .collect()
        });
        Ok(Self { kms_key_id, chain_id, listen_addr, allowed_to })
    }
}
