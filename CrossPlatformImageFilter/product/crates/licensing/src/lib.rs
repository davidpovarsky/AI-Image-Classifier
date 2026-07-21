#![forbid(unsafe_code)]

use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
    #[error("machine activation limit was reached")]
    ActivationLimit,
    #[error("recovery token was already used or rolled back")]
    RecoveryReplay,
    #[error("recovery token is invalid")]
    RecoveryToken,
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

#[derive(Default)]
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
}
