use std::{fs::File, io::BufReader, path::Path, sync::Arc};

use eyre::{Context, Result};
use rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer},
    server::WebPkiClientVerifier,
};
use rustls_pemfile::{certs, private_key};

#[derive(Debug)]
pub struct TlsConfig {
    pub cert_file: String,
    pub key_file: String,
    /// When set, the server requires clients to present a certificate signed by this CA (mTLS).
    pub client_ca_file: Option<String>,
}

impl TlsConfig {
    pub fn build_server_config(&self) -> Result<Arc<ServerConfig>> {
        let certs = load_certs(self.cert_file.as_ref())?;
        let key = load_key(self.key_file.as_ref())?;

        let config = if let Some(ca_path) = &self.client_ca_file {
            let ca_certs = load_certs(ca_path.as_ref())?;
            let mut root_store = rustls::RootCertStore::empty();
            for cert in ca_certs {
                root_store.add(cert).wrap_err("invalid CA certificate")?;
            }
            let verifier = WebPkiClientVerifier::builder(Arc::new(root_store))
                .build()
                .wrap_err("failed to build client verifier")?;
            ServerConfig::builder()
                .with_client_cert_verifier(verifier)
                .with_single_cert(certs, key)
                .wrap_err("invalid server certificate or key")?
        } else {
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, key)
                .wrap_err("invalid server certificate or key")?
        };

        Ok(Arc::new(config))
    }
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let f = File::open(path).wrap_err_with(|| format!("open {}", path.display()))?;
    certs(&mut BufReader::new(f))
        .collect::<Result<Vec<_>, _>>()
        .wrap_err_with(|| format!("parse certs from {}", path.display()))
}

fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let f = File::open(path).wrap_err_with(|| format!("open {}", path.display()))?;
    private_key(&mut BufReader::new(f))
        .wrap_err_with(|| format!("parse key from {}", path.display()))?
        .ok_or_else(|| eyre::eyre!("no private key found in {}", path.display()))
}
