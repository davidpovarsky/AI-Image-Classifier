#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySnapshot {
    pub platform: String,
    pub network_service_id: String,
    pub exact_state: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateIdentity {
    pub sha256_fingerprint: String,
    pub store: String,
}

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("platform operation is unsupported: {0}")]
    Unsupported(&'static str),
    #[error("platform command failed: {0}")]
    Failed(String),
    #[error("saved proxy state is invalid")]
    InvalidProxySnapshot,
    #[error("certificate fingerprint mismatch")]
    CertificateMismatch,
}

pub trait CaptureBackend {
    fn detect_conflicts(&self) -> Result<Vec<String>, PlatformError>;
    fn start(&mut self) -> Result<(), PlatformError>;
    fn health_check(&self) -> Result<(), PlatformError>;
    fn stop(&mut self) -> Result<(), PlatformError>;
}

pub trait ProxyConfiguration {
    fn snapshot(&self) -> Result<ProxySnapshot, PlatformError>;
    fn enable_loopback_proxy(&mut self, snapshot: &ProxySnapshot) -> Result<(), PlatformError>;
    fn restore_exact(&mut self, snapshot: &ProxySnapshot) -> Result<(), PlatformError>;
    fn verify_internet_independent(&self) -> Result<(), PlatformError>;
}

pub trait CertificateTrust {
    fn install_product_ca(&mut self) -> Result<CertificateIdentity, PlatformError>;
    fn verify_trusted(&self, identity: &CertificateIdentity) -> Result<(), PlatformError>;
    fn remove_exact(&mut self, identity: &CertificateIdentity) -> Result<(), PlatformError>;
}

pub trait ServiceRegistration {
    fn install(&mut self) -> Result<(), PlatformError>;
    fn start(&mut self) -> Result<(), PlatformError>;
    fn stop(&mut self) -> Result<(), PlatformError>;
    fn uninstall(&mut self) -> Result<(), PlatformError>;
}

/// A future WFP or NetworkExtension implementation must satisfy this contract;
/// no kernel driver or undocumented API is part of the current product.
pub trait NativeEnforcementBackend: CaptureBackend {}
