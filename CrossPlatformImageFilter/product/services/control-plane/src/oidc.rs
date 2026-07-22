use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OidcError {
    #[error("OIDC configuration requires HTTPS issuer and JWKS URLs plus a non-empty audience")]
    Configuration,
    #[error("OIDC discovery or JWKS retrieval failed")]
    Retrieval,
    #[error("OIDC access token is invalid")]
    Token,
    #[error("OIDC access token does not grant the required role")]
    Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminIdentity {
    pub subject: String,
    pub tenant_id: uuid::Uuid,
    pub roles: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    tenant_id: uuid::Uuid,
    #[serde(default)]
    roles: Vec<String>,
}

#[derive(Clone)]
pub struct OidcVerifier {
    issuer: String,
    audience: String,
    keys: JwkSet,
}

impl OidcVerifier {
    pub async fn load(
        issuer: String,
        audience: String,
        jwks_url: String,
    ) -> Result<Self, OidcError> {
        let issuer_url = reqwest::Url::parse(&issuer).map_err(|_| OidcError::Configuration)?;
        let jwks = reqwest::Url::parse(&jwks_url).map_err(|_| OidcError::Configuration)?;
        if issuer_url.scheme() != "https"
            || jwks.scheme() != "https"
            || issuer_url.username() != ""
            || jwks.username() != ""
            || issuer_url.password().is_some()
            || jwks.password().is_some()
            || audience.is_empty()
        {
            return Err(OidcError::Configuration);
        }
        let keys = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| OidcError::Retrieval)?
            .get(jwks)
            .send()
            .await
            .map_err(|_| OidcError::Retrieval)?
            .error_for_status()
            .map_err(|_| OidcError::Retrieval)?
            .json::<JwkSet>()
            .await
            .map_err(|_| OidcError::Retrieval)?;
        if keys.keys.is_empty() {
            return Err(OidcError::Retrieval);
        }
        Ok(Self {
            issuer: issuer.trim_end_matches('/').to_owned(),
            audience,
            keys,
        })
    }

    pub fn verify(
        &self,
        bearer_token: &str,
        required_role: &str,
    ) -> Result<AdminIdentity, OidcError> {
        let header = decode_header(bearer_token).map_err(|_| OidcError::Token)?;
        if !matches!(
            header.alg,
            Algorithm::RS256 | Algorithm::PS256 | Algorithm::ES256 | Algorithm::EdDSA
        ) {
            return Err(OidcError::Token);
        }
        let kid = header.kid.as_deref().ok_or(OidcError::Token)?;
        let jwk = self.keys.find(kid).ok_or(OidcError::Token)?;
        let key = DecodingKey::from_jwk(jwk).map_err(|_| OidcError::Token)?;
        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        let claims = decode::<Claims>(bearer_token, &key, &validation)
            .map_err(|_| OidcError::Token)?
            .claims;
        if claims.subject_is_invalid() {
            return Err(OidcError::Token);
        }
        if !claims
            .roles
            .iter()
            .any(|role| role == required_role || role == "filter-admin")
        {
            return Err(OidcError::Role);
        }
        Ok(AdminIdentity {
            subject: claims.sub,
            tenant_id: claims.tenant_id,
            roles: claims.roles,
        })
    }
}

impl Claims {
    fn subject_is_invalid(&self) -> bool {
        self.sub.is_empty() || self.sub.len() > 512 || self.roles.len() > 64
    }
}

pub fn bearer(headers: &axum::http::HeaderMap) -> Result<&str, OidcError> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && value.len() <= 16 * 1024)
        .ok_or(OidcError::Token)
}
