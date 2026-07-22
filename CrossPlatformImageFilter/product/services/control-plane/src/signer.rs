use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SignerError {
    #[error("KMS/HSM signer configuration is invalid")]
    Configuration,
    #[error("KMS/HSM signer request failed")]
    Transport,
    #[error("KMS/HSM signer returned an invalid signature")]
    Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyAssignmentClaims {
    pub tenant_id: uuid::Uuid,
    pub device_id: uuid::Uuid,
    pub channel: String,
    pub target_path: String,
    pub minimum_revision: u64,
    pub issued_at: i64,
    pub expires_at: i64,
    pub nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedPolicyAssignment {
    pub claims: PolicyAssignmentClaims,
    pub key_id: String,
    pub algorithm: String,
    pub signature: String,
}

#[derive(Clone)]
pub struct RemoteSigner {
    endpoint: reqwest::Url,
    bearer_token: String,
    key_id: String,
    public_key: [u8; 32],
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignerResponse {
    key_id: String,
    algorithm: String,
    signature_base64: String,
}

impl RemoteSigner {
    pub fn new(
        endpoint: String,
        bearer_token: String,
        key_id: String,
        public_key_base64: &str,
    ) -> Result<Self, SignerError> {
        let endpoint = reqwest::Url::parse(&endpoint).map_err(|_| SignerError::Configuration)?;
        let public_key = STANDARD
            .decode(public_key_base64)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(SignerError::Configuration)?;
        if endpoint.scheme() != "https"
            || endpoint.username() != ""
            || endpoint.password().is_some()
            || bearer_token.len() < 32
            || key_id.is_empty()
            || key_id.len() > 128
        {
            return Err(SignerError::Configuration);
        }
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| SignerError::Configuration)?;
        Ok(Self {
            endpoint,
            bearer_token,
            key_id,
            public_key,
            client,
        })
    }

    pub async fn sign_assignment(
        &self,
        claims: PolicyAssignmentClaims,
    ) -> Result<SignedPolicyAssignment, SignerError> {
        let canonical =
            serde_json_canonicalizer::to_vec(&claims).map_err(|_| SignerError::Signature)?;
        let response = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(&self.bearer_token)
            .json(&serde_json::json!({
                "keyId": self.key_id,
                "algorithm": "Ed25519",
                "payloadBase64": STANDARD.encode(&canonical),
            }))
            .send()
            .await
            .map_err(|_| SignerError::Transport)?
            .error_for_status()
            .map_err(|_| SignerError::Transport)?
            .json::<SignerResponse>()
            .await
            .map_err(|_| SignerError::Transport)?;
        if response.key_id != self.key_id || response.algorithm != "Ed25519" {
            return Err(SignerError::Signature);
        }
        let signature = STANDARD
            .decode(&response.signature_base64)
            .ok()
            .and_then(|bytes| Signature::from_slice(&bytes).ok())
            .ok_or(SignerError::Signature)?;
        VerifyingKey::from_bytes(&self.public_key)
            .and_then(|key| key.verify(&canonical, &signature))
            .map_err(|_| SignerError::Signature)?;
        Ok(SignedPolicyAssignment {
            claims,
            key_id: response.key_id,
            algorithm: response.algorithm,
            signature: response.signature_base64,
        })
    }
}
