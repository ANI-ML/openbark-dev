//! The hub's certificate (ADR-0029 section 4): self-signed, ECDSA P-256,
//! made the first time hub mode is turned on and kept (sealed by the vault
//! in the hub's folder) so devices that pinned it keep working across
//! restarts. Devices pin its SHA-256 fingerprint; nothing here is trusted
//! through a certificate authority.

use std::path::Path;
use std::sync::Arc;

use os_sync::keys;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};

use crate::HubError;

#[derive(Clone, Serialize, Deserialize)]
pub struct Identity {
    /// The certificate, DER, base64.
    cert: String,
    /// The private key, PKCS#8 DER, base64. Never logged.
    key: String,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("fingerprint", &hex::encode(self.fingerprint()))
            .finish_non_exhaustive()
    }
}

impl Identity {
    pub fn cert_der(&self) -> Vec<u8> {
        keys::unb64(&self.cert).unwrap_or_default()
    }

    /// SHA-256 of the certificate: what devices pin.
    pub fn fingerprint(&self) -> [u8; 32] {
        os_sync::hub::fingerprint(&self.cert_der())
    }

    pub fn fingerprint_hex(&self) -> String {
        hex::encode(self.fingerprint())
    }

    /// The certificate check shown beside a join code.
    pub fn check_code(&self) -> String {
        os_sync::hub::check_code(&self.fingerprint())
    }

    pub fn server_config(&self) -> Result<Arc<rustls::ServerConfig>, HubError> {
        let key = keys::unb64(&self.key).map_err(|_| HubError::Io("the hub's key is damaged".into()))?;
        let cfg = rustls::ServerConfig::builder_with_provider(os_sync::hub::crypto_provider())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| HubError::Io(e.to_string()))?
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(self.cert_der())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
            )
            .map_err(|e| HubError::Io(e.to_string()))?;
        Ok(Arc::new(cfg))
    }
}

/// A new certificate and key.
pub fn generate() -> Result<Identity, HubError> {
    let err = |e: rcgen::Error| HubError::Io(format!("couldn't make the hub's certificate: {e}"));
    let key = rcgen::KeyPair::generate().map_err(err)?;
    let mut params = rcgen::CertificateParams::new(vec!["openbark-hub".to_string()]).map_err(err)?;
    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, "OpenBark clinic hub");
    params.distinguished_name = dn;
    params.not_before = rcgen::date_time_ymd(2026, 1, 1);
    params.not_after = rcgen::date_time_ymd(2046, 1, 1);
    let cert = params.self_signed(&key).map_err(err)?;
    Ok(Identity { cert: keys::b64(cert.der().as_ref()), key: keys::b64(&key.serialize_der()) })
}

/// The hub's certificate, made on first use.
pub fn load_or_create(dir: &Path) -> Result<Identity, HubError> {
    let path = dir.join("tls.json");
    match os_engine::vault::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| HubError::Io("the hub's certificate is damaged".into())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let id = generate()?;
            std::fs::create_dir_all(dir).map_err(|e| HubError::Io(e.to_string()))?;
            let bytes = serde_json::to_vec(&id).map_err(|e| HubError::Io(e.to_string()))?;
            os_engine::vault::write(&path, &bytes).map_err(|e| HubError::Io(e.to_string()))?;
            Ok(id)
        }
        Err(e) => Err(HubError::Io(e.to_string())),
    }
}
