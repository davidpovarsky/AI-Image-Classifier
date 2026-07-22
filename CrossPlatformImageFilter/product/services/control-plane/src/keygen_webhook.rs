use axum::http::HeaderMap;
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime};
use subtle::ConstantTimeEq;
use thiserror::Error;

const WEBHOOK_PATH: &str = "/v1/webhooks/keygen";

#[derive(Debug, Error)]
pub enum WebhookError {
    #[error("Keygen webhook verifier configuration is invalid")]
    Configuration,
    #[error("Keygen webhook signature headers are invalid")]
    Headers,
    #[error("Keygen webhook body digest is invalid")]
    Digest,
    #[error("Keygen webhook signature is invalid")]
    Signature,
    #[error("Keygen webhook is stale")]
    Stale,
}

#[derive(Clone)]
pub struct KeygenWebhookVerifier {
    account_id: String,
    public_key: [u8; 32],
    public_host: String,
}

impl KeygenWebhookVerifier {
    pub fn new(
        account_id: String,
        public_key_base64: &str,
        public_host: String,
    ) -> Result<Self, WebhookError> {
        let public_key = STANDARD
            .decode(public_key_base64)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(WebhookError::Configuration)?;
        if account_id.is_empty()
            || public_host.is_empty()
            || public_host.contains('/')
            || public_host.contains('@')
            || public_host.chars().any(char::is_whitespace)
        {
            return Err(WebhookError::Configuration);
        }
        Ok(Self {
            account_id,
            public_key,
            public_host,
        })
    }

    pub fn verify(&self, headers: &HeaderMap, body: &[u8]) -> Result<(), WebhookError> {
        let date = header(headers, "date")?;
        let sent_at = httpdate::parse_http_date(date).map_err(|_| WebhookError::Headers)?;
        let age = SystemTime::now()
            .duration_since(sent_at)
            .or_else(|error| Ok::<Duration, std::convert::Infallible>(error.duration()))
            .expect("infallible duration conversion");
        if age > Duration::from_secs(300) {
            return Err(WebhookError::Stale);
        }
        let expected_digest = format!("sha-256={}", STANDARD.encode(Sha256::digest(body)));
        let supplied_digest = header(headers, "digest")?;
        if expected_digest
            .as_bytes()
            .ct_eq(supplied_digest.as_bytes())
            .unwrap_u8()
            != 1
        {
            return Err(WebhookError::Digest);
        }
        let signature_header = header(headers, "keygen-signature")?;
        let key_id = signature_parameter(signature_header, "keyid")?;
        let algorithm = signature_parameter(signature_header, "algorithm")?;
        let signed_headers = signature_parameter(signature_header, "headers")?;
        let encoded_signature = signature_parameter(signature_header, "signature")?;
        if key_id != self.account_id
            || algorithm != "ed25519"
            || signed_headers != "(request-target) host date digest"
        {
            return Err(WebhookError::Headers);
        }
        let signing_data = format!(
            "(request-target): post {WEBHOOK_PATH}\nhost: {}\ndate: {date}\ndigest: {expected_digest}",
            self.public_host
        );
        let signature = STANDARD
            .decode(encoded_signature)
            .ok()
            .and_then(|bytes| Signature::from_slice(&bytes).ok())
            .ok_or(WebhookError::Signature)?;
        VerifyingKey::from_bytes(&self.public_key)
            .and_then(|key| key.verify(signing_data.as_bytes(), &signature))
            .map_err(|_| WebhookError::Signature)
    }
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, WebhookError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .ok_or(WebhookError::Headers)
}

fn signature_parameter<'a>(header: &'a str, name: &str) -> Result<&'a str, WebhookError> {
    header
        .split(',')
        .map(str::trim)
        .find_map(|part| {
            let (key, value) = part.split_once('=')?;
            (key == name)
                .then(|| value.strip_prefix('"')?.strip_suffix('"'))
                .flatten()
        })
        .ok_or(WebhookError::Headers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn verifies_raw_body_digest_signature_and_freshness() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let verifier = KeygenWebhookVerifier::new(
            "account".into(),
            &STANDARD.encode(key.verifying_key().to_bytes()),
            "control.example".into(),
        )
        .unwrap();
        let body = br#"{"data":{"id":"event","attributes":{"event":"license.updated"}}}"#;
        let date = httpdate::fmt_http_date(SystemTime::now());
        let digest = format!("sha-256={}", STANDARD.encode(Sha256::digest(body)));
        let signing_data = format!(
            "(request-target): post {WEBHOOK_PATH}\nhost: control.example\ndate: {date}\ndigest: {digest}"
        );
        let signature = STANDARD.encode(key.sign(signing_data.as_bytes()).to_bytes());
        let mut headers = HeaderMap::new();
        headers.insert("date", date.parse().unwrap());
        headers.insert("digest", digest.parse().unwrap());
        headers.insert(
            "keygen-signature",
            format!("keyid=\"account\",algorithm=\"ed25519\",signature=\"{signature}\",headers=\"(request-target) host date digest\"")
                .parse()
                .unwrap(),
        );
        assert!(verifier.verify(&headers, body).is_ok());
        assert!(matches!(
            verifier.verify(&headers, b"altered"),
            Err(WebhookError::Digest)
        ));
    }
}
