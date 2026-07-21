#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Development,
    Production,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceRecord {
    pub device_id: Uuid,
    pub tenant_id: Uuid,
    pub public_key: Vec<u8>,
    pub policy_channel: String,
    pub acknowledged_revision: u64,
    pub license_entitlement: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyAssignment {
    pub tenant_id: Uuid,
    pub device_id: Uuid,
    pub policy_id: String,
    pub revision: u64,
    pub target_path: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ControlPlaneError {
    #[error("development authentication is forbidden in production")]
    DevelopmentAuthInProduction,
    #[error("OIDC authentication is required")]
    OidcRequired,
    #[error("tenant isolation violation")]
    TenantIsolation,
    #[error("device challenge was replayed or is unknown")]
    ChallengeReplay,
    #[error("policy publication failed before commit")]
    Publication,
    #[error("invalid policy target path")]
    TargetPath,
}

pub struct AuthenticationPolicy {
    mode: RuntimeMode,
    development_auth_enabled: bool,
}

impl AuthenticationPolicy {
    pub fn new(
        mode: RuntimeMode,
        development_auth_enabled: bool,
    ) -> Result<Self, ControlPlaneError> {
        if mode == RuntimeMode::Production && development_auth_enabled {
            return Err(ControlPlaneError::DevelopmentAuthInProduction);
        }
        Ok(Self {
            mode,
            development_auth_enabled,
        })
    }

    pub fn require_oidc(&self, oidc_subject: Option<&str>) -> Result<(), ControlPlaneError> {
        if self.mode == RuntimeMode::Development && self.development_auth_enabled {
            return Ok(());
        }
        if oidc_subject.is_some_and(|subject| !subject.is_empty()) {
            Ok(())
        } else {
            Err(ControlPlaneError::OidcRequired)
        }
    }
}

#[derive(Default)]
pub struct ChallengeRegistry {
    outstanding: HashMap<Uuid, (String, i64)>,
    consumed: HashSet<String>,
}

impl ChallengeRegistry {
    pub fn issue(&mut self, device_id: Uuid, challenge: String) {
        self.outstanding.insert(
            device_id,
            (challenge, time::OffsetDateTime::now_utc().unix_timestamp()),
        );
    }

    pub fn consume(&mut self, device_id: Uuid, challenge: &str) -> Result<(), ControlPlaneError> {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let valid = self
            .outstanding
            .remove(&device_id)
            .is_some_and(|(expected, issued_at)| {
                expected == challenge && now.saturating_sub(issued_at) <= 120
            });
        if self.consumed.contains(challenge) || !valid {
            return Err(ControlPlaneError::ChallengeReplay);
        }
        self.consumed.insert(challenge.to_owned());
        Ok(())
    }
}

pub fn assign_policy(
    actor_tenant: Uuid,
    device: &DeviceRecord,
    policy_id: String,
    revision: u64,
) -> Result<PolicyAssignment, ControlPlaneError> {
    if actor_tenant != device.tenant_id {
        return Err(ControlPlaneError::TenantIsolation);
    }
    let target_path = format!("policies/devices/{}/policy-bundle.json", device.device_id);
    if target_path.contains("..") || target_path.starts_with('/') {
        return Err(ControlPlaneError::TargetPath);
    }
    Ok(PolicyAssignment {
        tenant_id: actor_tenant,
        device_id: device.device_id,
        policy_id,
        revision,
        target_path,
    })
}

pub trait TufPublicationStore {
    type Transaction;
    fn begin(&mut self) -> Result<Self::Transaction, ControlPlaneError>;
    fn write_target(
        &mut self,
        transaction: &mut Self::Transaction,
        path: &str,
        bytes: &[u8],
    ) -> Result<(), ControlPlaneError>;
    fn write_metadata(
        &mut self,
        transaction: &mut Self::Transaction,
        bytes: &[u8],
    ) -> Result<(), ControlPlaneError>;
    fn commit(&mut self, transaction: Self::Transaction) -> Result<(), ControlPlaneError>;
    fn rollback(&mut self, transaction: Self::Transaction);
}

pub fn publish_transactionally<S: TufPublicationStore>(
    store: &mut S,
    target_path: &str,
    target: &[u8],
    metadata: &[u8],
) -> Result<(), ControlPlaneError> {
    let mut transaction = store.begin()?;
    let result = store
        .write_target(&mut transaction, target_path, target)
        .and_then(|()| store.write_metadata(&mut transaction, metadata));
    if let Err(error) = result {
        store.rollback(transaction);
        return Err(error);
    }
    store.commit(transaction)
}

const MAX_DEVICE_BODY_BYTES: usize = 64 * 1024;
const MAX_DEVICE_CLOCK_SKEW_SECONDS: i64 = 120;

#[derive(Clone)]
pub struct HttpState {
    inner: Arc<HttpStateInner>,
}

struct HttpStateInner {
    challenges: Mutex<ChallengeRegistry>,
    devices: Mutex<HashMap<Uuid, DeviceRecord>>,
    consumed_nonces: Mutex<HashSet<String>>,
    enrollment_token: String,
}

impl HttpState {
    pub fn new(enrollment_token: String) -> Result<Self, ControlPlaneError> {
        if enrollment_token.len() < 32 {
            return Err(ControlPlaneError::OidcRequired);
        }
        Ok(Self {
            inner: Arc::new(HttpStateInner {
                challenges: Mutex::new(ChallengeRegistry::default()),
                devices: Mutex::new(HashMap::new()),
                consumed_nonces: Mutex::new(HashSet::new()),
                enrollment_token,
            }),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChallengeRequest {
    device_id: Uuid,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChallengeResponse {
    challenge: String,
    expires_in_seconds: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RegistrationRequest {
    device_id: Uuid,
    tenant_id: Uuid,
    public_key: String,
    challenge: String,
    signature: String,
    enrollment_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HealthPayload {
    state: String,
    product_version: String,
    engine_version: String,
    policy_revision: u64,
    license_state: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignedCheckIn {
    device_id: Uuid,
    timestamp: i64,
    nonce: String,
    payload: HealthPayload,
    signature: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignedAcknowledgement {
    device_id: Uuid,
    timestamp: i64,
    nonce: String,
    revision: u64,
    signature: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyMetadataQuery {
    device_id: Uuid,
    timestamp: i64,
    nonce: String,
    signature: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PolicyMetadataResponse {
    channel: String,
    target_path: String,
    acknowledged_revision: u64,
}

#[derive(Debug)]
struct ApiError {
    status: axum::http::StatusCode,
    code: &'static str,
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            axum::Json(serde_json::json!({"error": {"code": self.code}})),
        )
            .into_response()
    }
}

pub fn router(state: HttpState) -> axum::Router {
    use axum::{
        Router,
        extract::DefaultBodyLimit,
        routing::{get, post},
    };

    Router::new()
        .route("/v1/device/challenge", post(issue_challenge))
        .route("/v1/device/register", post(register_device))
        .route("/v1/device/check-in", post(check_in))
        .route("/v1/device/policy-metadata", get(policy_metadata))
        .route(
            "/v1/device/policy-acknowledgement",
            post(acknowledge_policy),
        )
        .layer(DefaultBodyLimit::max(MAX_DEVICE_BODY_BYTES))
        .with_state(state)
}

async fn issue_challenge(
    axum::extract::State(state): axum::extract::State<HttpState>,
    axum::Json(request): axum::Json<ChallengeRequest>,
) -> Result<axum::Json<ChallengeResponse>, ApiError> {
    let challenge = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    state
        .inner
        .challenges
        .lock()
        .map_err(|_| internal())?
        .issue(request.device_id, challenge.clone());
    Ok(axum::Json(ChallengeResponse {
        challenge,
        expires_in_seconds: 120,
    }))
}

async fn register_device(
    axum::extract::State(state): axum::extract::State<HttpState>,
    axum::Json(request): axum::Json<RegistrationRequest>,
) -> Result<axum::Json<DeviceRecord>, ApiError> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    use subtle::ConstantTimeEq;

    if request
        .enrollment_token
        .as_bytes()
        .ct_eq(state.inner.enrollment_token.as_bytes())
        .unwrap_u8()
        != 1
    {
        return Err(unauthorized("enrollmentDenied"));
    }
    state
        .inner
        .challenges
        .lock()
        .map_err(|_| internal())?
        .consume(request.device_id, &request.challenge)
        .map_err(|_| unauthorized("challengeInvalid"))?;
    let public_key: [u8; 32] = STANDARD
        .decode(request.public_key)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| bad_request("publicKeyInvalid"))?;
    let signature = STANDARD
        .decode(request.signature)
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .ok_or_else(|| unauthorized("signatureInvalid"))?;
    VerifyingKey::from_bytes(&public_key)
        .and_then(|key| key.verify(request.challenge.as_bytes(), &signature))
        .map_err(|_| unauthorized("signatureInvalid"))?;
    let device = DeviceRecord {
        device_id: request.device_id,
        tenant_id: request.tenant_id,
        public_key: public_key.to_vec(),
        policy_channel: "stable".into(),
        acknowledged_revision: 0,
        license_entitlement: "pending".into(),
    };
    state
        .inner
        .devices
        .lock()
        .map_err(|_| internal())?
        .insert(device.device_id, device.clone());
    Ok(axum::Json(device))
}

async fn check_in(
    axum::extract::State(state): axum::extract::State<HttpState>,
    axum::Json(request): axum::Json<SignedCheckIn>,
) -> Result<axum::http::StatusCode, ApiError> {
    let payload = serde_json::to_value(&request.payload).map_err(|_| internal())?;
    verify_device_request(
        &state,
        request.device_id,
        request.timestamp,
        &request.nonce,
        &payload,
        &request.signature,
    )?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn policy_metadata(
    axum::extract::State(state): axum::extract::State<HttpState>,
    axum::extract::Query(request): axum::extract::Query<PolicyMetadataQuery>,
) -> Result<axum::Json<PolicyMetadataResponse>, ApiError> {
    verify_device_request(
        &state,
        request.device_id,
        request.timestamp,
        &request.nonce,
        &serde_json::json!({"operation": "policyMetadata"}),
        &request.signature,
    )?;
    let devices = state.inner.devices.lock().map_err(|_| internal())?;
    let device = devices
        .get(&request.device_id)
        .ok_or_else(|| unauthorized("deviceUnknown"))?;
    Ok(axum::Json(PolicyMetadataResponse {
        channel: device.policy_channel.clone(),
        target_path: format!("policies/devices/{}/policy-bundle.json", device.device_id),
        acknowledged_revision: device.acknowledged_revision,
    }))
}

async fn acknowledge_policy(
    axum::extract::State(state): axum::extract::State<HttpState>,
    axum::Json(request): axum::Json<SignedAcknowledgement>,
) -> Result<axum::http::StatusCode, ApiError> {
    verify_device_request(
        &state,
        request.device_id,
        request.timestamp,
        &request.nonce,
        &serde_json::json!({"revision": request.revision}),
        &request.signature,
    )?;
    let mut devices = state.inner.devices.lock().map_err(|_| internal())?;
    let device = devices
        .get_mut(&request.device_id)
        .ok_or_else(|| unauthorized("deviceUnknown"))?;
    if request.revision < device.acknowledged_revision {
        return Err(bad_request("revisionRollback"));
    }
    device.acknowledged_revision = request.revision;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

fn verify_device_request(
    state: &HttpState,
    device_id: Uuid,
    timestamp: i64,
    nonce: &str,
    payload: &serde_json::Value,
    signature: &str,
) -> Result<(), ApiError> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    if nonce.len() < 32 || (timestamp - now).abs() > MAX_DEVICE_CLOCK_SKEW_SECONDS {
        return Err(unauthorized("requestExpired"));
    }
    let replay_key = format!("{device_id}:{nonce}");
    if !state
        .inner
        .consumed_nonces
        .lock()
        .map_err(|_| internal())?
        .insert(replay_key)
    {
        return Err(unauthorized("requestReplayed"));
    }
    let devices = state.inner.devices.lock().map_err(|_| internal())?;
    let device = devices
        .get(&device_id)
        .ok_or_else(|| unauthorized("deviceUnknown"))?;
    let public_key: [u8; 32] = device
        .public_key
        .clone()
        .try_into()
        .map_err(|_| internal())?;
    let message = serde_json_canonicalizer::to_vec(&serde_json::json!({
        "deviceId": device_id,
        "timestamp": timestamp,
        "nonce": nonce,
        "payload": payload,
    }))
    .map_err(|_| internal())?;
    let signature = STANDARD
        .decode(signature)
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .ok_or_else(|| unauthorized("signatureInvalid"))?;
    VerifyingKey::from_bytes(&public_key)
        .and_then(|key| key.verify(&message, &signature))
        .map_err(|_| unauthorized("signatureInvalid"))
}

fn unauthorized(code: &'static str) -> ApiError {
    ApiError {
        status: axum::http::StatusCode::UNAUTHORIZED,
        code,
    }
}

fn bad_request(code: &'static str) -> ApiError {
    ApiError {
        status: axum::http::StatusCode::BAD_REQUEST,
        code,
    }
}

fn internal() -> ApiError {
    ApiError {
        status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        code: "internal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request as HttpRequest, StatusCode},
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    use ed25519_dalek::{Signer, SigningKey};
    use tower::ServiceExt;

    #[test]
    fn production_refuses_development_auth() {
        assert!(matches!(
            AuthenticationPolicy::new(RuntimeMode::Production, true),
            Err(ControlPlaneError::DevelopmentAuthInProduction)
        ));
    }

    #[test]
    fn challenge_cannot_be_replayed() {
        let device = Uuid::new_v4();
        let mut challenges = ChallengeRegistry::default();
        challenges.issue(device, "signed-challenge".into());
        assert_eq!(challenges.consume(device, "signed-challenge"), Ok(()));
        assert_eq!(
            challenges.consume(device, "signed-challenge"),
            Err(ControlPlaneError::ChallengeReplay)
        );
    }

    #[test]
    fn assignment_enforces_tenant_isolation() {
        let tenant = Uuid::new_v4();
        let device = DeviceRecord {
            device_id: Uuid::new_v4(),
            tenant_id: tenant,
            public_key: vec![1; 32],
            policy_channel: "stable".into(),
            acknowledged_revision: 0,
            license_entitlement: "commercial".into(),
        };
        assert!(assign_policy(tenant, &device, "policy".into(), 1).is_ok());
        assert!(matches!(
            assign_policy(Uuid::new_v4(), &device, "policy".into(), 1),
            Err(ControlPlaneError::TenantIsolation)
        ));
        let serialized = serde_json::to_string(&device).unwrap();
        assert!(!serialized.contains("image"));
        assert!(!serialized.contains("url"));
    }

    #[tokio::test]
    async fn device_registration_requires_signed_single_use_challenge() {
        let device_id = Uuid::new_v4();
        let tenant_id = Uuid::new_v4();
        let application = router(HttpState::new("e".repeat(32)).unwrap());
        let response = application
            .clone()
            .oneshot(
                HttpRequest::post("/v1/device/challenge")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"deviceId":"{device_id}"}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), MAX_DEVICE_BODY_BYTES)
            .await
            .unwrap();
        let challenge = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["challenge"]
            .as_str()
            .unwrap()
            .to_owned();
        let key = SigningKey::from_bytes(&[4; 32]);
        let registration = serde_json::json!({
            "deviceId": device_id,
            "tenantId": tenant_id,
            "publicKey": STANDARD.encode(key.verifying_key().to_bytes()),
            "challenge": challenge,
            "signature": STANDARD.encode(key.sign(challenge.as_bytes()).to_bytes()),
            "enrollmentToken": "e".repeat(32),
        });
        let register = || {
            HttpRequest::post("/v1/device/register")
                .header("content-type", "application/json")
                .body(Body::from(registration.to_string()))
                .unwrap()
        };
        assert_eq!(
            application
                .clone()
                .oneshot(register())
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            application.oneshot(register()).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn oversized_and_unexpected_device_fields_are_rejected() {
        let application = router(HttpState::new("e".repeat(32)).unwrap());
        let unexpected = application
            .clone()
            .oneshot(
                HttpRequest::post("/v1/device/challenge")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"deviceId": Uuid::new_v4(), "image": "forbidden"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unexpected.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let oversized = application
            .oneshot(
                HttpRequest::post("/v1/device/challenge")
                    .header("content-type", "application/json")
                    .body(Body::from(vec![b'a'; MAX_DEVICE_BODY_BYTES + 1]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
