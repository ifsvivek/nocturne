//! TLS for the remote binding: the user's own certificate and key (PEM). No ACME, no self-signed generation. (Same as the gateway's; the gateway module will use this one.)
use rustls::ServerConfig;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::path::Path;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

/// Load the chain and the key. Errors say which file and why; they never contain key material.
pub fn load(cert: &Path, key: &Path) -> Result<TlsAcceptor, String> {
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert)
        .map_err(|e| format!("certificate file {}: {e}", cert.display()))?
        .collect::<Result<_, _>>()
        .map_err(|e| format!("certificate file {}: {e}", cert.display()))?;
    if certs.is_empty() {
        return Err(format!(
            "certificate file {} has no certificate",
            cert.display()
        ));
    }
    let key = PrivateKeyDer::from_pem_file(key)
        .map_err(|e| format!("key file {}: {e}", key.display()))?;
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut cfg = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|e| format!("TLS setup: {e}"))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("certificate and key do not match or are unusable: {e}"))?;
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(TlsAcceptor::from(Arc::new(cfg)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_files_are_refused_with_a_reason() {
        let d = tempfile::tempdir().unwrap();
        let (c, k) = (d.path().join("c.pem"), d.path().join("k.pem"));
        assert!(
            load(&c, &k).err().unwrap().contains("c.pem"),
            "missing file is named"
        );
        std::fs::write(&c, "not a pem").unwrap();
        std::fs::write(&k, "not a key").unwrap();
        assert!(load(&c, &k).err().unwrap().contains("no certificate"));
        std::fs::write(
            &c,
            "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n",
        )
        .unwrap();
        assert!(load(&c, &k).is_err());
    }
}
