mod error;
mod rpc;
mod signer;
mod tx_builder;

use std::{net::SocketAddr, sync::Arc};

use alloy::primitives::Address;
use eyre::{Context, Result};
use jsonrpsee::server::Server;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use rpc::{SignerRpcServer, SignerServer};
use signer::KmsSigner;

/// Runtime configuration sourced exclusively from environment variables (REQ-M-003).
#[derive(Debug)]
pub struct Config {
    pub kms_key_id: String,
    pub chain_id: u64,
    pub listen_addr: SocketAddr,
    /// Optional allowlist of `to` addresses checked on every sign request (REQ-F-003).
    pub allowed_to: Option<Vec<Address>>,
}

impl Config {
    fn from_env() -> Result<Self> {
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

        Ok(Self {
            kms_key_id,
            chain_id,
            listen_addr,
            allowed_to,
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Structured JSON logging to stdout (REQ-M-005 / DataDog ingestion).
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(fmt::layer().json())
        .init();

    let config = Config::from_env().wrap_err("invalid configuration")?;
    info!(
        chain_id = config.chain_id,
        listen_addr = %config.listen_addr,
        "espresso-kms-signer starting"
    );

    // Derive the signing address from the KMS public key and log it (REQ-F-003 AC-1).
    let signer = Arc::new(
        KmsSigner::new(config.kms_key_id.clone(), config.chain_id)
            .await
            .wrap_err("failed to initialise KMS signer")?,
    );
    info!(
        address = %signer.address,
        kms_key_id = %config.kms_key_id,
        "signer ready"
    );

    let config = Arc::new(config);
    let server = Server::builder()
        .build(config.listen_addr)
        .await
        .wrap_err("failed to bind RPC server")?;

    let handle = server.start(SignerServer::new(signer, config).into_rpc());
    info!("RPC server listening");

    // Block until the process receives a shutdown signal.
    handle.stopped().await;
    Ok(())
}
