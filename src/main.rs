use std::sync::Arc;

use eyre::{Context, Result};
use jsonrpsee::server::Server;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use espresso_kms_signer::{
    Config,
    rpc::{SignerRpcServer, SignerServer},
    signer::KmsSigner,
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(fmt::layer().json())
        .init();

    let config = Config::from_env().wrap_err("invalid configuration")?;
    info!(chain_id = config.chain_id, listen_addr = %config.listen_addr, "espresso-kms-signer starting");

    let signer = Arc::new(
        KmsSigner::new(config.kms_key_id.clone(), config.chain_id)
            .await
            .wrap_err("failed to initialise KMS signer")?,
    );
    info!(address = %signer.address, kms_key_id = %config.kms_key_id, "signer ready");

    let config = Arc::new(config);
    let server = Server::builder()
        .build(config.listen_addr)
        .await
        .wrap_err("failed to bind RPC server")?;

    let handle = server.start(SignerServer::new(signer, config).into_rpc());
    info!("RPC server listening");

    handle.stopped().await;
    Ok(())
}
