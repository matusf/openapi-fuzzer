use std::sync::Arc;

use rustls::client::ServerCertVerifier;

// See https://quinn-rs.github.io/quinn/quinn/certificate.html#insecure-connection
struct SkipTlsVerification {}

impl ServerCertVerifier for SkipTlsVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        // always accept the certificate
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

pub fn skip_tls_verification_config() -> rustls::ClientConfig {
    rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(Arc::new(SkipTlsVerification {}))
        .with_no_client_auth()
}
