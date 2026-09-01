pub mod batch_shape;
pub mod error;
pub mod rpc;
pub mod signer;
pub mod tls;
pub mod tx_builder;

use std::net::SocketAddr;

use alloy::primitives::Address;
use eyre::{Context, Result};

use crate::tls::TlsConfig;

#[derive(Debug)]
pub struct Config {
    pub kms_key_id: String,
    pub chain_id: u64,
    pub listen_addr: SocketAddr,
    pub allowed_to: Option<Vec<Address>>,
    pub tls: Option<TlsConfig>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let kms_key_id = std::env::var("AWS_KMS_KEY_ID").wrap_err("AWS_KMS_KEY_ID must be set")?;
        let chain_id: u64 = std::env::var("CHAIN_ID")
            .wrap_err("CHAIN_ID must be set")?
            .parse()
            .wrap_err("CHAIN_ID must be a positive integer")?;
        let listen_addr: SocketAddr = std::env::var("LISTEN_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8547".into())
            .parse()
            .wrap_err("LISTEN_ADDR must be a valid socket address")?;
        let allowed_to = if let Ok(raw) = std::env::var("ALLOWED_TO") {
            let addrs = raw
                .split(',')
                .map(|a| {
                    a.trim()
                        .parse::<Address>()
                        .wrap_err_with(|| format!("invalid address in ALLOWED_TO: {a}"))
                })
                .collect::<Result<Vec<_>>>()
                .wrap_err("ALLOWED_TO must be a comma-separated list of hex addresses")?;
            Some(addrs)
        } else {
            None
        };
        let tls = match (
            std::env::var("TLS_CERT_FILE").ok(),
            std::env::var("TLS_KEY_FILE").ok(),
        ) {
            (Some(cert_file), Some(key_file)) => Some(TlsConfig {
                cert_file,
                key_file,
                client_ca_file: std::env::var("TLS_CLIENT_CA_FILE").ok(),
            }),
            (None, None) => None,
            _ => eyre::bail!("TLS_CERT_FILE and TLS_KEY_FILE must both be set or both be unset"),
        };

        Ok(Self {
            kms_key_id,
            chain_id,
            listen_addr,
            allowed_to,
            tls,
        })
    }
}
