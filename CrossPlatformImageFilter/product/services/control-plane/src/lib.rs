#![forbid(unsafe_code)]

pub mod keygen_webhook;
pub mod oidc;
pub mod signer;
pub mod storage;

use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
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
    store: storage::Store,
    oidc: Option<oidc::OidcVerifier>,
    keygen_webhook: Option<keygen_webhook::KeygenWebhookVerifier>,
    signer: Option<signer::RemoteSigner>,
    enrollment_token: String,
}

impl HttpState {
    pub fn new(enrollment_token: String) -> Result<Self, ControlPlaneError> {
        if enrollment_token.len() < 32 {
            return Err(ControlPlaneError::OidcRequired);
        }
        Ok(Self {
            inner: Arc::new(HttpStateInner {
                store: storage::Store::memory(),
                oidc: None,
                keygen_webhook: None,
                signer: None,
                enrollment_token,
            }),
        })
    }

    pub fn production(
        enrollment_token: String,
        store: storage::Store,
        oidc: oidc::OidcVerifier,
        keygen_webhook: keygen_webhook::KeygenWebhookVerifier,
        signer: signer::RemoteSigner,
    ) -> Result<Self, ControlPlaneError> {
        if enrollment_token.len() < 32 || !matches!(&store, storage::Store::Postgres(_)) {
            return Err(ControlPlaneError::OidcRequired);
        }
        Ok(Self {
            inner: Arc::new(HttpStateInner {
                store,
                oidc: Some(oidc),
                keygen_webhook: Some(keygen_webhook),
                signer: Some(signer),
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
pub(crate) struct HealthPayload {
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
        .route("/v1/admin/device/locate", post(admin_locate_device))
        .route(
            "/v1/admin/device/policy-channel",
            post(admin_policy_channel),
        )
        .route(
            "/v1/admin/device/policy-refresh",
            post(admin_policy_refresh),
        )
        .route(
            "/v1/admin/device/policy-acknowledgement",
            post(admin_policy_acknowledgement),
        )
        .route(
            "/v1/admin/device/license-entitlement",
            post(admin_license_entitlement),
        )
        .route("/v1/admin/device/audit-history", post(admin_audit_history))
        .route("/v1/webhooks/keygen", post(receive_keygen_webhook))
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
        .store
        .issue_challenge(request.device_id, challenge.clone())
        .await
        .map_err(|_| internal())?;
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
        .store
        .consume_challenge(request.device_id, &request.challenge)
        .await
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
        .store
        .upsert_device(&device)
        .await
        .map_err(|_| internal())?;
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
    )
    .await?;
    state
        .inner
        .store
        .record_health(request.device_id, &request.payload)
        .await
        .map_err(|_| internal())?;
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
    )
    .await?;
    let device = state
        .inner
        .store
        .device(request.device_id)
        .await
        .map_err(|_| unauthorized("deviceUnknown"))?;
    Ok(axum::Json(PolicyMetadataResponse {
        channel: device.policy_channel,
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
    )
    .await?;
    state
        .inner
        .store
        .acknowledge(request.device_id, request.revision)
        .await
        .map_err(|_| bad_request("revisionRollback"))?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AdminDeviceRequest {
    device_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AdminPolicyChannelRequest {
    device_id: Uuid,
    channel: String,
}

fn admin_identity(
    state: &HttpState,
    headers: &axum::http::HeaderMap,
    role: &str,
) -> Result<oidc::AdminIdentity, ApiError> {
    if let Some(verifier) = &state.inner.oidc {
        return verifier
            .verify(
                oidc::bearer(headers).map_err(|_| unauthorized("oidcTokenInvalid"))?,
                role,
            )
            .map_err(|_| unauthorized("oidcTokenInvalid"));
    }
    let subject = headers
        .get("x-development-subject")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| unauthorized("developmentIdentityRequired"))?;
    let tenant_id = headers
        .get("x-development-tenant")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| unauthorized("developmentIdentityRequired"))?;
    Ok(oidc::AdminIdentity {
        subject: subject.to_owned(),
        tenant_id,
        roles: vec!["filter-admin".into()],
    })
}

async fn tenant_device(
    state: &HttpState,
    identity: &oidc::AdminIdentity,
    device_id: Uuid,
) -> Result<DeviceRecord, ApiError> {
    let device = state
        .inner
        .store
        .device(device_id)
        .await
        .map_err(|_| unauthorized("tenantIsolation"))?;
    if device.tenant_id != identity.tenant_id {
        return Err(unauthorized("tenantIsolation"));
    }
    Ok(device)
}

async fn admin_locate_device(
    axum::extract::State(state): axum::extract::State<HttpState>,
    headers: axum::http::HeaderMap,
    axum::Json(request): axum::Json<AdminDeviceRequest>,
) -> Result<axum::Json<DeviceRecord>, ApiError> {
    let identity = admin_identity(&state, &headers, "filter-viewer")?;
    Ok(axum::Json(
        tenant_device(&state, &identity, request.device_id).await?,
    ))
}

async fn admin_policy_channel(
    axum::extract::State(state): axum::extract::State<HttpState>,
    headers: axum::http::HeaderMap,
    axum::Json(request): axum::Json<AdminPolicyChannelRequest>,
) -> Result<axum::Json<serde_json::Value>, ApiError> {
    let identity = admin_identity(&state, &headers, "filter-admin")?;
    if !matches!(request.channel.as_str(), "stable" | "beta") {
        return Err(bad_request("policyChannelInvalid"));
    }
    let current_device = tenant_device(&state, &identity, request.device_id).await?;
    let signer = state
        .inner
        .signer
        .as_ref()
        .ok_or_else(|| unavailable("policySignerCredentialRequired"))?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let signed_assignment = signer
        .sign_assignment(signer::PolicyAssignmentClaims {
            tenant_id: current_device.tenant_id,
            device_id: current_device.device_id,
            channel: request.channel.clone(),
            target_path: format!(
                "policies/devices/{}/policy-bundle.json",
                current_device.device_id
            ),
            minimum_revision: current_device
                .acknowledged_revision
                .checked_add(1)
                .ok_or_else(|| bad_request("policyRevisionExhausted"))?,
            issued_at: now,
            expires_at: now + 900,
            nonce: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
        })
        .await
        .map_err(|_| unavailable("policySignerUnavailable"))?;
    // Do not commit the requested channel until the external signer has returned
    // an assignment that this service verified locally.
    let device = state
        .inner
        .store
        .set_policy_channel(
            identity.tenant_id,
            request.device_id,
            &request.channel,
            &identity.subject,
        )
        .await
        .map_err(|_| bad_request("policyChannelInvalid"))?;
    Ok(axum::Json(serde_json::json!({
        "device": device,
        "signedAssignment": signed_assignment,
    })))
}

async fn admin_policy_refresh(
    axum::extract::State(state): axum::extract::State<HttpState>,
    headers: axum::http::HeaderMap,
    axum::Json(request): axum::Json<AdminDeviceRequest>,
) -> Result<axum::Json<serde_json::Value>, ApiError> {
    let identity = admin_identity(&state, &headers, "filter-admin")?;
    state
        .inner
        .store
        .request_policy_refresh(identity.tenant_id, request.device_id, &identity.subject)
        .await
        .map_err(|_| unauthorized("tenantIsolation"))?;
    Ok(axum::Json(serde_json::json!({"requested": true})))
}

async fn admin_policy_acknowledgement(
    axum::extract::State(state): axum::extract::State<HttpState>,
    headers: axum::http::HeaderMap,
    axum::Json(request): axum::Json<AdminDeviceRequest>,
) -> Result<axum::Json<serde_json::Value>, ApiError> {
    let identity = admin_identity(&state, &headers, "filter-viewer")?;
    let device = tenant_device(&state, &identity, request.device_id).await?;
    Ok(axum::Json(serde_json::json!({
        "deviceId": device.device_id,
        "acknowledgedRevision": device.acknowledged_revision,
        "policyChannel": device.policy_channel,
    })))
}

async fn admin_license_entitlement(
    axum::extract::State(state): axum::extract::State<HttpState>,
    headers: axum::http::HeaderMap,
    axum::Json(request): axum::Json<AdminDeviceRequest>,
) -> Result<axum::Json<serde_json::Value>, ApiError> {
    let identity = admin_identity(&state, &headers, "filter-viewer")?;
    let device = tenant_device(&state, &identity, request.device_id).await?;
    Ok(axum::Json(serde_json::json!({
        "deviceId": device.device_id,
        "licenseEntitlement": device.license_entitlement,
    })))
}

async fn admin_audit_history(
    axum::extract::State(state): axum::extract::State<HttpState>,
    headers: axum::http::HeaderMap,
    axum::Json(request): axum::Json<AdminDeviceRequest>,
) -> Result<axum::Json<Vec<storage::AuditRecord>>, ApiError> {
    let identity = admin_identity(&state, &headers, "filter-viewer")?;
    tenant_device(&state, &identity, request.device_id).await?;
    let records = state
        .inner
        .store
        .audit_history(identity.tenant_id, request.device_id)
        .await
        .map_err(|_| internal())?;
    Ok(axum::Json(records))
}

async fn receive_keygen_webhook(
    axum::extract::State(state): axum::extract::State<HttpState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<axum::http::StatusCode, ApiError> {
    let verifier = state
        .inner
        .keygen_webhook
        .as_ref()
        .ok_or_else(|| unavailable("keygenWebhookCredentialRequired"))?;
    verifier
        .verify(&headers, &body)
        .map_err(|_| unauthorized("keygenWebhookSignatureInvalid"))?;
    let payload: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| bad_request("keygenWebhookBodyInvalid"))?;
    let event_id = payload
        .pointer("/data/id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .ok_or_else(|| bad_request("keygenWebhookBodyInvalid"))?
        .to_owned();
    let event_type = payload
        .pointer("/data/attributes/event")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .ok_or_else(|| bad_request("keygenWebhookBodyInvalid"))?
        .to_owned();
    state
        .inner
        .store
        .record_keygen_webhook(&event_id, &event_type, payload)
        .await
        .map_err(|_| conflict("keygenWebhookReplay"))?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn verify_device_request(
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
    state
        .inner
        .store
        .consume_nonce(device_id, nonce)
        .await
        .map_err(|_| unauthorized("requestReplayed"))?;
    let device = state
        .inner
        .store
        .device(device_id)
        .await
        .map_err(|_| unauthorized("deviceUnknown"))?;
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

fn conflict(code: &'static str) -> ApiError {
    ApiError {
        status: axum::http::StatusCode::CONFLICT,
        code,
    }
}

fn unavailable(code: &'static str) -> ApiError {
    ApiError {
        status: axum::http::StatusCode::SERVICE_UNAVAILABLE,
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

    #[tokio::test]
    async fn signer_failure_does_not_commit_policy_channel() {
        let tenant_id = Uuid::new_v4();
        let device_id = Uuid::new_v4();
        let state = HttpState::new("e".repeat(32)).unwrap();
        state
            .inner
            .store
            .upsert_device(&DeviceRecord {
                device_id,
                tenant_id,
                public_key: vec![7; 32],
                policy_channel: "stable".into(),
                acknowledged_revision: 4,
                license_entitlement: "commercial".into(),
            })
            .await
            .unwrap();
        let response = router(state.clone())
            .oneshot(
                HttpRequest::post("/v1/admin/device/policy-channel")
                    .header("content-type", "application/json")
                    .header("x-development-subject", "administrator")
                    .header("x-development-tenant", tenant_id.to_string())
                    .body(Body::from(
                        serde_json::json!({"deviceId": device_id, "channel": "beta"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            state
                .inner
                .store
                .device(device_id)
                .await
                .unwrap()
                .policy_channel,
            "stable"
        );
    }
}
