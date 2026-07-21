#![forbid(unsafe_code)]

use control_plane::{AuthenticationPolicy, HttpState, RuntimeMode, router};
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
    let state = HttpState::new(enrollment_token)?;
    let address: SocketAddr = env::var("CONTROL_PLANE_LISTEN_ADDRESS")
        .unwrap_or_else(|_| "127.0.0.1:8787".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}
