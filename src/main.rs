use std::sync::Arc;

use eyre::{Context, Result};
use jsonrpsee::server::{serve_with_graceful_shutdown, stop_channel};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use espresso_kms_signer::{
    rpc::{SignerRpcServer, SignerServer},
    signer::KmsSigner,
    Config,
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

    let tls_acceptor = config.tls.as_ref().map(|t| {
        let mtls = t.client_ca_file.is_some();
        let acceptor = TlsAcceptor::from(t.build_server_config()?);
        info!(mtls, "TLS enabled");
        Ok::<_, eyre::Report>(acceptor)
    }).transpose().wrap_err("invalid TLS configuration")?;

    let listener = TcpListener::bind(config.listen_addr)
        .await
        .wrap_err("failed to bind RPC server")?;

    let methods = SignerServer::new(signer, Arc::new(config)).into_rpc();
    let svc_builder = jsonrpsee::server::Server::builder().to_service_builder();
    let (stop_handle, server_handle) = stop_channel();

    // Forward OS shutdown signals to the jsonrpsee stop channel so in-flight
    // requests complete before the process exits.
    tokio::spawn({
        let server_handle = server_handle.clone();
        async move {
            shutdown_signal().await;
            let _ = server_handle.stop();
        }
    });

    info!("RPC server listening");

    let mut shutdown = std::pin::pin!(stop_handle.clone().shutdown());
    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => break,
            result = listener.accept() => {
                let (stream, peer) = match result {
                    Ok(v) => v,
                    Err(e) => { error!(error = %e, "accept failed"); continue; }
                };

                let svc = svc_builder.clone().build(methods.clone(), stop_handle.clone());
                let stop = stop_handle.clone().shutdown();
                let acceptor = tls_acceptor.clone();

                tokio::spawn(async move {
                    let result = match acceptor {
                        Some(a) => match a.accept(stream).await {
                            Ok(s) => serve_with_graceful_shutdown(s, svc, stop).await,
                            Err(e) => { warn!(peer = %peer, error = %e, "TLS handshake failed"); return; }
                        },
                        None => serve_with_graceful_shutdown(stream, svc, stop).await,
                    };
                    if let Err(e) = result {
                        warn!(peer = %peer, error = %e, "connection error");
                    }
                });
            }
        }
    }

    server_handle.stopped().await;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = sigterm.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await.ok();
}
