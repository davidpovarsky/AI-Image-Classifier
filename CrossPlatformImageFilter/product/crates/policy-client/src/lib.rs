#![forbid(unsafe_code)]

use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::OffsetDateTime;

pub const MAX_POLICY_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicySubject {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleSignature {
    pub key_id: String,
    pub algorithm: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyBundle {
    pub schema_version: u32,
    pub policy_id: String,
    pub revision: u64,
    pub channel: String,
    pub issued_at: String,
    pub expires_at: String,
    pub minimum_engine_version: String,
    pub minimum_product_version: String,
    pub subject: PolicySubject,
    pub policy: Value,
    pub processing: Value,
    pub metadata: Value,
    pub signature: BundleSignature,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyError {
    #[error("policy target is oversized")]
    Oversized,
    #[error("policy JSON is invalid: {0}")]
    Json(String),
    #[error("policy signing key is unknown")]
    UnknownKey,
    #[error("policy signature is invalid")]
    Signature,
    #[error("policy is expired or not yet valid")]
    Time,
    #[error("policy rollback was rejected")]
    Rollback,
    #[error("policy subject does not match this tenant or device")]
    Subject,
}

pub struct VerificationContext<'a> {
    pub key_id: &'a str,
    pub public_key: &'a [u8; 32],
    pub tenant_id: Option<&'a str>,
    pub device_id: Option<&'a str>,
    pub minimum_revision: u64,
    pub now: OffsetDateTime,
}

pub fn verify(bytes: &[u8], context: VerificationContext<'_>) -> Result<PolicyBundle, PolicyError> {
    if bytes.len() > MAX_POLICY_BYTES {
        return Err(PolicyError::Oversized);
    }
    let bundle: PolicyBundle =
        serde_json::from_slice(bytes).map_err(|error| PolicyError::Json(error.to_string()))?;
    if bundle.revision < context.minimum_revision {
        return Err(PolicyError::Rollback);
    }
    if bundle.signature.key_id != context.key_id {
        return Err(PolicyError::UnknownKey);
    }
    if bundle.signature.algorithm != "Ed25519" {
        return Err(PolicyError::Signature);
    }
    if (bundle.subject.kind == "tenant" && Some(bundle.subject.id.as_str()) != context.tenant_id)
        || (bundle.subject.kind == "device"
            && Some(bundle.subject.id.as_str()) != context.device_id)
    {
        return Err(PolicyError::Subject);
    }
    let issued = OffsetDateTime::parse(
        &bundle.issued_at,
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| PolicyError::Time)?;
    let expires = OffsetDateTime::parse(
        &bundle.expires_at,
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| PolicyError::Time)?;
    if issued > context.now || expires <= context.now || expires <= issued {
        return Err(PolicyError::Time);
    }

    let mut value =
        serde_json::to_value(&bundle).map_err(|error| PolicyError::Json(error.to_string()))?;
    value
        .as_object_mut()
        .ok_or_else(|| PolicyError::Json("bundle is not an object".into()))?
        .remove("signature");
    let canonical = serde_json_canonicalizer::to_vec(&value)
        .map_err(|error| PolicyError::Json(error.to_string()))?;
    let signature_bytes = STANDARD
        .decode(&bundle.signature.signature)
        .map_err(|_| PolicyError::Signature)?;
    let signature = Signature::from_slice(&signature_bytes).map_err(|_| PolicyError::Signature)?;
    VerifyingKey::from_bytes(context.public_key)
        .map_err(|_| PolicyError::Signature)?
        .verify(&canonical, &signature)
        .map_err(|_| PolicyError::Signature)?;
    Ok(bundle)
}

pub fn effective_fingerprint(
    model_manifest_fingerprint: &str,
    bundle: &PolicyBundle,
    merge_settings: &Value,
) -> String {
    let canonical = serde_json_canonicalizer::to_vec(&serde_json::json!({
        "modelManifest": model_manifest_fingerprint,
        "policyId": bundle.policy_id,
        "policyRevision": bundle.revision,
        "policy": bundle.policy,
        "processing": bundle.processing,
        "merge": merge_settings,
    }))
    .expect("serializable policy fingerprint input");
    format!("{:x}", Sha256::digest(canonical))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationPlan {
    pub staged_path: std::path::PathBuf,
    pub active_path: std::path::PathBuf,
    pub last_known_good_path: std::path::PathBuf,
}

pub trait AtomicPolicyStore {
    fn stage(&self, verified_bytes: &[u8]) -> Result<ActivationPlan, PolicyError>;
    fn dry_run_engine_configuration(&self, plan: &ActivationPlan) -> Result<(), PolicyError>;
    fn activate(&self, plan: ActivationPlan) -> Result<(), PolicyError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_changes_cache_fingerprint_even_for_identical_values() {
        let mut first: PolicyBundle = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1, "policyId": "p", "revision": 1, "channel": "stable",
            "issuedAt": "2026-01-01T00:00:00Z", "expiresAt": "2027-01-01T00:00:00Z",
            "minimumEngineVersion": "0.1.0", "minimumProductVersion": "0.1.0",
            "subject": {"type": "vendor-global", "id": "vendor"},
            "policy": {}, "processing": {}, "metadata": {},
            "signature": {"keyId": "key", "algorithm": "Ed25519", "signature": ""}
        }))
        .unwrap();
        let one = effective_fingerprint("models", &first, &serde_json::json!({}));
        first.revision = 2;
        let two = effective_fingerprint("models", &first, &serde_json::json!({}));
        assert_ne!(one, two);
    }

    #[test]
    fn oversized_policy_fails_before_parsing() {
        let context = VerificationContext {
            key_id: "key",
            public_key: &[0; 32],
            tenant_id: None,
            device_id: None,
            minimum_revision: 0,
            now: OffsetDateTime::UNIX_EPOCH,
        };
        assert!(matches!(
            verify(&vec![0; MAX_POLICY_BYTES + 1], context),
            Err(PolicyError::Oversized)
        ));
    }
}
