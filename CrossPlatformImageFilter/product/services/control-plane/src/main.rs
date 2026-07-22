#![forbid(unsafe_code)]

use control_plane::{
    AuthenticationPolicy, HttpState, RuntimeMode, keygen_webhook::KeygenWebhookVerifier,
    oidc::OidcVerifier, router, signer::RemoteSigner, storage::Store,
};
use std::{env, net::SocketAddr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let production = env::var("CONTROL_PLANE_MODE").as_deref() == Ok("production");
    let development_auth = env::var("CONTROL_PLANE_DEVELOPMENT_AUTH").as_deref() == Ok("1");
    let mode = if production {
        RuntimeMode::Production
    } else {
        RuntimeMode::Development
    };
    AuthenticationPolicy::new(mode, development_auth)?;
    let enrollment_token = env::var("CONTROL_PLANE_ENROLLMENT_TOKEN")?;
    let state = if production {
        let database_url = env::var("CONTROL_PLANE_DATABASE_URL")?;
        let oidc = OidcVerifier::load(
            env::var("CONTROL_PLANE_OIDC_ISSUER")?,
            env::var("CONTROL_PLANE_OIDC_AUDIENCE")?,
            env::var("CONTROL_PLANE_OIDC_JWKS_URL")?,
        )
        .await?;
        let keygen_webhook = KeygenWebhookVerifier::new(
            env::var("CONTROL_PLANE_KEYGEN_ACCOUNT_ID")?,
            &env::var("CONTROL_PLANE_KEYGEN_ED25519_PUBLIC_KEY_BASE64")?,
            env::var("CONTROL_PLANE_PUBLIC_HOST")?,
        )?;
        let signer = RemoteSigner::new(
            env::var("CONTROL_PLANE_SIGNER_URL")?,
            env::var("CONTROL_PLANE_SIGNER_BEARER_TOKEN")?,
            env::var("CONTROL_PLANE_POLICY_SIGNING_KEY_ID")?,
            &env::var("CONTROL_PLANE_POLICY_SIGNING_PUBLIC_KEY_BASE64")?,
        )?;
        HttpState::production(
            enrollment_token,
            Store::postgres(&database_url).await?,
            oidc,
            keygen_webhook,
            signer,
        )?
    } else {
        HttpState::new(enrollment_token)?
    };
    let address: SocketAddr = env::var("CONTROL_PLANE_LISTEN_ADDRESS")
        .unwrap_or_else(|_| "127.0.0.1:8787".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}
