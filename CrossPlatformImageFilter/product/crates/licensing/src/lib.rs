#![forbid(unsafe_code)]

use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, time::Duration};
use thiserror::Error;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LicenseClaims {
    pub license_id: String,
    pub device_id: String,
    pub entitlement: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub offline_until: i64,
    pub revoked: bool,
    #[serde(default)]
    pub suspended: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedLicense {
    pub claims: LicenseClaims,
    pub key_id: String,
    pub algorithm: String,
    pub signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseState {
    Unactivated,
    ActiveOnline,
    ActiveOffline,
    Grace,
    Expired,
    Suspended,
    Revoked,
    ValidationError,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LicenseError {
    #[error("license uses an unsupported signature algorithm")]
    Algorithm,
    #[error("license signing key is unknown")]
    UnknownKey,
    #[error("license signature is invalid")]
    Signature,
    #[error("license belongs to a different device")]
    Device,
    #[error("license claims are malformed")]
    Claims,
    #[error("license provider configuration is invalid")]
    Configuration,
    #[error("license provider request failed")]
    Transport,
    #[error("license provider response is malformed")]
    Response,
    #[error("machine activation limit was reached")]
    ActivationLimit,
    #[error("recovery token was already used or rolled back")]
    RecoveryReplay,
    #[error("recovery token is invalid")]
    RecoveryToken,
}

const MAX_LICENSE_FILE_BYTES: usize = 1_048_576;
const KEYGEN_ENDPOINT: &str = "https://api.keygen.sh";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedKeygenLicense {
    pub certificate: Vec<u8>,
    pub machine_id: String,
    pub license_id: String,
    pub device_fingerprint: String,
    pub device_public_key: String,
    pub issued_at: i64,
    pub offline_until: i64,
    pub license_expires_at: Option<i64>,
    pub suspended: bool,
    pub revoked: bool,
    pub entitlements: Vec<String>,
}

impl VerifiedKeygenLicense {
    pub fn state(&self, now: OffsetDateTime) -> LicenseState {
        let timestamp = now.unix_timestamp();
        if self.revoked {
            LicenseState::Revoked
        } else if self.suspended {
            LicenseState::Suspended
        } else if self
            .license_expires_at
            .is_some_and(|expiry| expiry <= timestamp)
        {
            LicenseState::Expired
        } else if timestamp <= self.offline_until {
            LicenseState::ActiveOffline
        } else {
            LicenseState::Expired
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeygenCertificateEnvelope {
    enc: String,
    sig: String,
    alg: String,
}

fn parse_timestamp(value: &str) -> Result<i64, LicenseError> {
    OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map(OffsetDateTime::unix_timestamp)
        .map_err(|_| LicenseError::Response)
}

fn string_at<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, LicenseError> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or(LicenseError::Response)
}

pub fn verify_keygen_machine_file(
    certificate: &[u8],
    expected_fingerprint: &str,
    expected_device_public_key: &str,
    expected_product_id: &str,
    account_public_key: &[u8; 32],
    now: OffsetDateTime,
) -> Result<VerifiedKeygenLicense, LicenseError> {
    if certificate.len() > MAX_LICENSE_FILE_BYTES {
        return Err(LicenseError::Claims);
    }
    let text = std::str::from_utf8(certificate).map_err(|_| LicenseError::Response)?;
    let normalized = text.replace("\r\n", "\n");
    let body = normalized
        .strip_prefix("-----BEGIN MACHINE FILE-----\n")
        .and_then(|value| value.strip_suffix("-----END MACHINE FILE-----\n"))
        .or_else(|| {
            normalized
                .strip_prefix("-----BEGIN MACHINE FILE-----\n")
                .and_then(|value| value.strip_suffix("-----END MACHINE FILE-----"))
        })
        .ok_or(LicenseError::Response)?;
    let outer_bytes = STANDARD
        .decode(body.split_whitespace().collect::<String>())
        .map_err(|_| LicenseError::Response)?;
    let envelope: KeygenCertificateEnvelope =
        serde_json::from_slice(&outer_bytes).map_err(|_| LicenseError::Response)?;
    if envelope.alg != "base64+ed25519" {
        return Err(LicenseError::Algorithm);
    }
    let signature_bytes = STANDARD
        .decode(&envelope.sig)
        .map_err(|_| LicenseError::Signature)?;
    let signature = Signature::from_slice(&signature_bytes).map_err(|_| LicenseError::Signature)?;
    let signed_message = format!("machine/{}", envelope.enc);
    VerifyingKey::from_bytes(account_public_key)
        .map_err(|_| LicenseError::Signature)?
        .verify(signed_message.as_bytes(), &signature)
        .map_err(|_| LicenseError::Signature)?;
    let decoded = STANDARD
        .decode(&envelope.enc)
        .map_err(|_| LicenseError::Response)?;
    let payload: Value = serde_json::from_slice(&decoded).map_err(|_| LicenseError::Response)?;
    let issued_at = parse_timestamp(string_at(&payload, "/meta/issued")?)?;
    let offline_until = parse_timestamp(string_at(&payload, "/meta/expiry")?)?;
    if issued_at > now.unix_timestamp() || offline_until <= issued_at {
        return Err(LicenseError::Claims);
    }
    let machine_id = string_at(&payload, "/data/id")?.to_owned();
    if string_at(&payload, "/data/type")? != "machines" {
        return Err(LicenseError::Response);
    }
    let fingerprint = string_at(&payload, "/data/attributes/fingerprint")?;
    let device_public_key = string_at(&payload, "/data/attributes/metadata/devicePublicKey")?;
    let license_id = string_at(&payload, "/data/relationships/license/data/id")?.to_owned();
    if fingerprint != expected_fingerprint || device_public_key != expected_device_public_key {
        return Err(LicenseError::Device);
    }

    let included = payload
        .get("included")
        .and_then(Value::as_array)
        .ok_or(LicenseError::Response)?;
    let license = included
        .iter()
        .find(|item| {
            item.get("type").and_then(Value::as_str) == Some("licenses")
                && item.get("id").and_then(Value::as_str) == Some(license_id.as_str())
        })
        .ok_or(LicenseError::Response)?;
    let product_id = string_at(license, "/relationships/product/data/id")?;
    if product_id != expected_product_id {
        return Err(LicenseError::Claims);
    }
    let status = string_at(license, "/attributes/status")?;
    let suspended = license
        .pointer("/attributes/suspended")
        .and_then(Value::as_bool)
        .ok_or(LicenseError::Response)?;
    let revoked = status == "BANNED";
    let license_expires_at = license
        .pointer("/attributes/expiry")
        .and_then(Value::as_str)
        .map(parse_timestamp)
        .transpose()?;
    let mut entitlements = included
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("entitlements"))
        .filter_map(|item| item.pointer("/attributes/code").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    entitlements.sort();
    entitlements.dedup();

    Ok(VerifiedKeygenLicense {
        certificate: certificate.to_vec(),
        machine_id,
        license_id,
        device_fingerprint: fingerprint.to_owned(),
        device_public_key: device_public_key.to_owned(),
        issued_at,
        offline_until,
        license_expires_at,
        suspended,
        revoked,
        entitlements,
    })
}

#[derive(Debug, Clone)]
pub struct KeygenClientConfiguration {
    pub account_id: String,
    pub product_id: String,
    pub account_public_key: [u8; 32],
    pub offline_ttl_seconds: u32,
}

pub struct KeygenHttpClient {
    configuration: KeygenClientConfiguration,
    agent: ureq::Agent,
}

impl KeygenHttpClient {
    pub fn new(configuration: KeygenClientConfiguration) -> Result<Self, LicenseError> {
        let safe_identifier = |value: &str| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        };
        if !safe_identifier(&configuration.account_id)
            || !safe_identifier(&configuration.product_id)
            || !(3600..=2_629_746).contains(&configuration.offline_ttl_seconds)
        {
            return Err(LicenseError::Configuration);
        }
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .https_only(true)
            .build()
            .new_agent();
        Ok(Self {
            configuration,
            agent,
        })
    }

    fn url(&self, suffix: &str) -> String {
        format!(
            "{KEYGEN_ENDPOINT}/v1/accounts/{}/{}",
            self.configuration.account_id, suffix
        )
    }

    fn response_json(
        response: &mut ureq::http::Response<ureq::Body>,
    ) -> Result<Value, LicenseError> {
        response
            .body_mut()
            .with_config()
            .limit(MAX_LICENSE_FILE_BYTES as u64)
            .read_json()
            .map_err(|_| LicenseError::Response)
    }

    fn post_json(
        &self,
        suffix: &str,
        authorization: Option<&str>,
        body: &Value,
    ) -> Result<Value, LicenseError> {
        let mut request = self
            .agent
            .post(&self.url(suffix))
            .header("Accept", "application/vnd.api+json")
            .header("Content-Type", "application/vnd.api+json");
        if let Some(value) = authorization {
            request = request.header("Authorization", value);
        }
        let mut response = request
            .send_json(body)
            .map_err(|_| LicenseError::Transport)?;
        let status = response.status().as_u16();
        let value = Self::response_json(&mut response)?;
        if !(200..300).contains(&status) {
            let activation_limit =
                value
                    .get("errors")
                    .and_then(Value::as_array)
                    .is_some_and(|errors| {
                        errors.iter().any(|item| {
                            matches!(
                                item.pointer("/code").and_then(Value::as_str),
                                Some("MACHINE_LIMIT_EXCEEDED" | "MACHINE_LIMIT_SCOPE_MISMATCH")
                            )
                        })
                    });
            return Err(if activation_limit {
                LicenseError::ActivationLimit
            } else {
                LicenseError::Transport
            });
        }
        Ok(value)
    }

    fn post_without_body(&self, suffix: &str, authorization: &str) -> Result<Value, LicenseError> {
        let mut response = self
            .agent
            .post(&self.url(suffix))
            .header("Accept", "application/vnd.api+json")
            .header("Authorization", authorization)
            .send_empty()
            .map_err(|_| LicenseError::Transport)?;
        if !response.status().is_success() {
            return Err(LicenseError::Transport);
        }
        Self::response_json(&mut response)
    }

    pub fn activate(
        &self,
        license_key: &str,
        device_fingerprint: &str,
        device_public_key: &[u8; 32],
        now: OffsetDateTime,
    ) -> Result<VerifiedKeygenLicense, LicenseError> {
        if license_key.is_empty() || license_key.len() > 4096 || device_fingerprint.len() != 64 {
            return Err(LicenseError::Claims);
        }
        let public_key = STANDARD.encode(device_public_key);
        let validation = self.post_json(
            "licenses/actions/validate-key",
            None,
            &serde_json::json!({
                "meta": {
                    "key": license_key,
                    "scope": {
                        "product": self.configuration.product_id,
                        "fingerprint": device_fingerprint
                    }
                }
            }),
        )?;
        let valid = validation
            .pointer("/meta/valid")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let validation_code = validation.pointer("/meta/code").and_then(Value::as_str);
        if !valid && validation_code != Some("NO_MACHINE") {
            return Err(LicenseError::Claims);
        }
        let license_id = string_at(&validation, "/data/id")?;
        if string_at(&validation, "/data/relationships/product/data/id")?
            != self.configuration.product_id
        {
            return Err(LicenseError::Claims);
        }
        let authorization = format!("License {license_key}");
        let machine = self.post_json(
            "machines",
            Some(&authorization),
            &serde_json::json!({
                "data": {
                    "type": "machines",
                    "attributes": {
                        "fingerprint": device_fingerprint,
                        "platform": std::env::consts::OS,
                        "name": "Local AI Image Filter",
                        "metadata": { "devicePublicKey": public_key }
                    },
                    "relationships": {
                        "license": { "data": { "type": "licenses", "id": license_id } }
                    }
                }
            }),
        )?;
        let machine_id = string_at(&machine, "/data/id")?;
        let checked_out = self.post_without_body(
            &format!(
                "machines/{machine_id}/actions/check-out?ttl={}&algorithm=base64%2Bed25519&include=license,license.entitlements",
                self.configuration.offline_ttl_seconds
            ),
            &authorization,
        )?;
        let certificate = string_at(&checked_out, "/data/attributes/certificate")?;
        verify_keygen_machine_file(
            certificate.as_bytes(),
            device_fingerprint,
            &public_key,
            &self.configuration.product_id,
            &self.configuration.account_public_key,
            now,
        )
    }

    pub fn refresh(
        &self,
        license_key: &str,
        current: &VerifiedKeygenLicense,
        now: OffsetDateTime,
    ) -> Result<VerifiedKeygenLicense, LicenseError> {
        let authorization = format!("License {license_key}");
        let checked_out = self.post_without_body(
            &format!(
                "machines/{}/actions/check-out?ttl={}&algorithm=base64%2Bed25519&include=license,license.entitlements",
                current.machine_id, self.configuration.offline_ttl_seconds
            ),
            &authorization,
        )?;
        let certificate = string_at(&checked_out, "/data/attributes/certificate")?;
        verify_keygen_machine_file(
            certificate.as_bytes(),
            &current.device_fingerprint,
            &current.device_public_key,
            &self.configuration.product_id,
            &self.configuration.account_public_key,
            now,
        )
    }

    pub fn deactivate(&self, license_key: &str, machine_id: &str) -> Result<(), LicenseError> {
        if machine_id.is_empty() || machine_id.len() > 128 {
            return Err(LicenseError::Claims);
        }
        let authorization = format!("License {license_key}");
        let response = self
            .agent
            .delete(&self.url(&format!("machines/{machine_id}")))
            .header("Accept", "application/vnd.api+json")
            .header("Authorization", &authorization)
            .call()
            .map_err(|_| LicenseError::Transport)?;
        if response.status().as_u16() == 204 {
            Ok(())
        } else {
            Err(LicenseError::Transport)
        }
    }
}

pub fn verify_offline_license(
    license: &SignedLicense,
    expected_device_id: &str,
    trusted_key_id: &str,
    trusted_public_key: &[u8; 32],
    now: OffsetDateTime,
) -> Result<LicenseState, LicenseError> {
    if license.algorithm != "Ed25519" {
        return Err(LicenseError::Algorithm);
    }
    if license.key_id != trusted_key_id {
        return Err(LicenseError::UnknownKey);
    }
    if license.claims.device_id != expected_device_id {
        return Err(LicenseError::Device);
    }
    if license.claims.issued_at > now.unix_timestamp()
        || license.claims.offline_until < license.claims.issued_at
    {
        return Err(LicenseError::Claims);
    }
    let payload =
        serde_json_canonicalizer::to_vec(&license.claims).map_err(|_| LicenseError::Claims)?;
    let bytes = STANDARD
        .decode(&license.signature)
        .map_err(|_| LicenseError::Signature)?;
    let signature = Signature::from_slice(&bytes).map_err(|_| LicenseError::Signature)?;
    VerifyingKey::from_bytes(trusted_public_key)
        .map_err(|_| LicenseError::Signature)?
        .verify(&payload, &signature)
        .map_err(|_| LicenseError::Signature)?;
    if license.claims.revoked {
        return Ok(LicenseState::Revoked);
    }
    if license.claims.suspended {
        return Ok(LicenseState::Suspended);
    }
    if now.unix_timestamp() <= license.claims.expires_at {
        return Ok(LicenseState::ActiveOffline);
    }
    if now.unix_timestamp() <= license.claims.offline_until {
        return Ok(LicenseState::Grace);
    }
    Ok(LicenseState::Expired)
}

pub trait LicenseProvider {
    fn activate(
        &self,
        license_key: &str,
        device_public_key: &[u8],
    ) -> Result<SignedLicense, LicenseError>;
    fn refresh(&self, current: &SignedLicense) -> Result<SignedLicense, LicenseError>;
    fn deactivate(&self, license_id: &str, device_id: &str) -> Result<(), LicenseError>;
}

/// Transport boundary for Keygen's documented license-authenticated client flow.
/// Implementations must use TLS and must never attach an administrative token.
pub trait KeygenTransport: Send + Sync {
    fn validate_and_activate(
        &self,
        account_id: &str,
        license_key: &str,
        device_public_key: &[u8],
    ) -> Result<SignedLicense, LicenseError>;
    fn refresh_machine(
        &self,
        account_id: &str,
        current: &SignedLicense,
    ) -> Result<SignedLicense, LicenseError>;
    fn deactivate_machine(
        &self,
        account_id: &str,
        license_id: &str,
        device_id: &str,
    ) -> Result<(), LicenseError>;
}

pub struct KeygenLicenseProvider<T> {
    account_id: String,
    endpoint: String,
    transport: T,
}

impl<T: KeygenTransport> KeygenLicenseProvider<T> {
    pub fn new(account_id: String, endpoint: String, transport: T) -> Result<Self, LicenseError> {
        if account_id.is_empty() || endpoint != "https://api.keygen.sh" {
            return Err(LicenseError::Configuration);
        }
        Ok(Self {
            account_id,
            endpoint,
            transport,
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

impl<T: KeygenTransport> LicenseProvider for KeygenLicenseProvider<T> {
    fn activate(
        &self,
        license_key: &str,
        device_public_key: &[u8],
    ) -> Result<SignedLicense, LicenseError> {
        if license_key.is_empty() || device_public_key.len() != 32 {
            return Err(LicenseError::Claims);
        }
        self.transport
            .validate_and_activate(&self.account_id, license_key, device_public_key)
    }

    fn refresh(&self, current: &SignedLicense) -> Result<SignedLicense, LicenseError> {
        self.transport.refresh_machine(&self.account_id, current)
    }

    fn deactivate(&self, license_id: &str, device_id: &str) -> Result<(), LicenseError> {
        self.transport
            .deactivate_machine(&self.account_id, license_id, device_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveryClaims {
    pub device_id: String,
    pub action: String,
    pub nonce: String,
    pub counter: u64,
    pub issued_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedRecoveryToken {
    pub claims: RecoveryClaims,
    pub key_id: String,
    pub algorithm: String,
    pub signature: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveryReplayStore {
    highest_counter: u64,
    consumed_nonces: HashSet<String>,
}

impl RecoveryReplayStore {
    #[allow(clippy::too_many_arguments)]
    pub fn verify_and_consume(
        &mut self,
        token: &SignedRecoveryToken,
        device_id: &str,
        action: &str,
        trusted_key_id: &str,
        trusted_public_key: &[u8; 32],
        now: OffsetDateTime,
    ) -> Result<(), LicenseError> {
        let claims = &token.claims;
        if token.algorithm != "Ed25519"
            || token.key_id != trusted_key_id
            || claims.device_id != device_id
            || claims.action != action
            || claims.nonce.len() < 32
            || claims.issued_at > now.unix_timestamp()
            || claims.expires_at < now.unix_timestamp()
            || claims.expires_at - claims.issued_at > 900
        {
            return Err(LicenseError::RecoveryToken);
        }
        if claims.counter <= self.highest_counter || self.consumed_nonces.contains(&claims.nonce) {
            return Err(LicenseError::RecoveryReplay);
        }
        let payload =
            serde_json_canonicalizer::to_vec(claims).map_err(|_| LicenseError::RecoveryToken)?;
        let signature = STANDARD
            .decode(&token.signature)
            .ok()
            .and_then(|bytes| Signature::from_slice(&bytes).ok())
            .ok_or(LicenseError::RecoveryToken)?;
        VerifyingKey::from_bytes(trusted_public_key)
            .map_err(|_| LicenseError::RecoveryToken)?
            .verify(&payload, &signature)
            .map_err(|_| LicenseError::RecoveryToken)?;
        self.highest_counter = claims.counter;
        self.consumed_nonces.insert(claims.nonce.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn keygen_machine_certificate(
        signing_key: &SigningKey,
        fingerprint: &str,
        device_public_key: &str,
    ) -> Vec<u8> {
        let payload = serde_json::json!({
            "meta": {
                "issued": "2026-01-01T00:00:00Z",
                "expiry": "2026-02-01T00:00:00Z",
                "ttl": 2_629_746
            },
            "data": {
                "id": "machine-1",
                "type": "machines",
                "attributes": {
                    "fingerprint": fingerprint,
                    "metadata": { "devicePublicKey": device_public_key }
                },
                "relationships": {
                    "license": { "data": { "type": "licenses", "id": "license-1" } }
                }
            },
            "included": [
                {
                    "id": "license-1",
                    "type": "licenses",
                    "attributes": {
                        "expiry": "2026-12-31T00:00:00Z",
                        "status": "ACTIVE",
                        "suspended": false
                    },
                    "relationships": {
                        "product": { "data": { "type": "products", "id": "product-1" } }
                    }
                },
                {
                    "id": "entitlement-1",
                    "type": "entitlements",
                    "attributes": { "code": "commercial-desktop" }
                }
            ]
        });
        let enc = STANDARD.encode(serde_json::to_vec(&payload).unwrap());
        let signature = STANDARD.encode(
            signing_key
                .sign(format!("machine/{enc}").as_bytes())
                .to_bytes(),
        );
        let outer = STANDARD.encode(
            serde_json::to_vec(&serde_json::json!({
                "enc": enc,
                "sig": signature,
                "alg": "base64+ed25519"
            }))
            .unwrap(),
        );
        format!("-----BEGIN MACHINE FILE-----\n{outer}\n-----END MACHINE FILE-----\n").into_bytes()
    }

    fn signed(device: &str, expires: i64, offline: i64) -> (SignedLicense, [u8; 32]) {
        let key = SigningKey::from_bytes(&[7; 32]);
        let claims = LicenseClaims {
            license_id: "license-1".into(),
            device_id: device.into(),
            entitlement: "commercial".into(),
            issued_at: 100,
            expires_at: expires,
            offline_until: offline,
            revoked: false,
            suspended: false,
        };
        let payload = serde_json_canonicalizer::to_vec(&claims).unwrap();
        let signature = STANDARD.encode(key.sign(&payload).to_bytes());
        (
            SignedLicense {
                claims,
                key_id: "vendor".into(),
                algorithm: "Ed25519".into(),
                signature,
            },
            key.verifying_key().to_bytes(),
        )
    }

    #[test]
    fn device_binding_and_grace_are_enforced() {
        let (license, public) = signed("device-a", 200, 300);
        let now = OffsetDateTime::from_unix_timestamp(250).unwrap();
        assert_eq!(
            verify_offline_license(&license, "device-a", "vendor", &public, now),
            Ok(LicenseState::Grace)
        );
        assert_eq!(
            verify_offline_license(&license, "device-b", "vendor", &public, now),
            Err(LicenseError::Device)
        );
    }

    #[test]
    fn tampering_is_rejected() {
        let (mut license, public) = signed("device-a", 200, 300);
        license.claims.entitlement = "tampered".into();
        let now = OffsetDateTime::from_unix_timestamp(150).unwrap();
        assert_eq!(
            verify_offline_license(&license, "device-a", "vendor", &public, now),
            Err(LicenseError::Signature)
        );
    }

    #[test]
    fn recovery_token_is_action_bound_and_single_use() {
        let key = SigningKey::from_bytes(&[8; 32]);
        let claims = RecoveryClaims {
            device_id: "device-a".into(),
            action: "uninstall".into(),
            nonce: "n".repeat(32),
            counter: 1,
            issued_at: 100,
            expires_at: 200,
        };
        let signature = STANDARD.encode(
            key.sign(&serde_json_canonicalizer::to_vec(&claims).unwrap())
                .to_bytes(),
        );
        let token = SignedRecoveryToken {
            claims,
            key_id: "recovery".into(),
            algorithm: "Ed25519".into(),
            signature,
        };
        let mut replay = RecoveryReplayStore::default();
        let now = OffsetDateTime::from_unix_timestamp(150).unwrap();
        assert!(
            replay
                .verify_and_consume(
                    &token,
                    "device-a",
                    "uninstall",
                    "recovery",
                    &key.verifying_key().to_bytes(),
                    now,
                )
                .is_ok()
        );
        assert_eq!(
            replay.verify_and_consume(
                &token,
                "device-a",
                "uninstall",
                "recovery",
                &key.verifying_key().to_bytes(),
                now,
            ),
            Err(LicenseError::RecoveryReplay)
        );
    }

    struct FailingTransport;

    impl KeygenTransport for FailingTransport {
        fn validate_and_activate(
            &self,
            _account_id: &str,
            _license_key: &str,
            _device_public_key: &[u8],
        ) -> Result<SignedLicense, LicenseError> {
            Err(LicenseError::Transport)
        }

        fn refresh_machine(
            &self,
            _account_id: &str,
            _current: &SignedLicense,
        ) -> Result<SignedLicense, LicenseError> {
            Err(LicenseError::Transport)
        }

        fn deactivate_machine(
            &self,
            _account_id: &str,
            _license_id: &str,
            _device_id: &str,
        ) -> Result<(), LicenseError> {
            Err(LicenseError::Transport)
        }
    }

    #[test]
    fn keygen_provider_allows_only_compiled_endpoint_and_no_admin_token() {
        assert!(matches!(
            KeygenLicenseProvider::new(
                "account".into(),
                "https://attacker.invalid".into(),
                FailingTransport,
            ),
            Err(LicenseError::Configuration)
        ));
        let provider = KeygenLicenseProvider::new(
            "account".into(),
            "https://api.keygen.sh".into(),
            FailingTransport,
        )
        .unwrap();
        assert_eq!(provider.endpoint(), "https://api.keygen.sh");
        assert_eq!(
            provider.activate("license-key", &[1; 32]),
            Err(LicenseError::Transport)
        );
    }

    #[test]
    fn keygen_machine_file_is_signature_device_product_and_ttl_bound() {
        let signing_key = SigningKey::from_bytes(&[11; 32]);
        let device_public_key = STANDARD.encode([5; 32]);
        let certificate =
            keygen_machine_certificate(&signing_key, &"a".repeat(64), &device_public_key);
        let now = OffsetDateTime::from_unix_timestamp(1_767_312_000).unwrap();
        let verified = verify_keygen_machine_file(
            &certificate,
            &"a".repeat(64),
            &device_public_key,
            "product-1",
            &signing_key.verifying_key().to_bytes(),
            now,
        )
        .unwrap();
        assert_eq!(verified.machine_id, "machine-1");
        assert_eq!(verified.license_id, "license-1");
        assert_eq!(verified.entitlements, ["commercial-desktop"]);
        assert_eq!(verified.state(now), LicenseState::ActiveOffline);
        assert_eq!(
            verify_keygen_machine_file(
                &certificate,
                &"b".repeat(64),
                &device_public_key,
                "product-1",
                &signing_key.verifying_key().to_bytes(),
                now,
            ),
            Err(LicenseError::Device)
        );
    }
}
