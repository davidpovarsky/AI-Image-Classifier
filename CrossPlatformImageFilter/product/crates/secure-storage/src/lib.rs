#![forbid(unsafe_code)]

use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use rand_core::OsRng;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

const MEMORY_KIB: u32 = 65_536;
const ITERATIONS: u32 = 3;
const LANES: u32 = 1;

#[derive(Debug, Error)]
pub enum SecureStorageError {
    #[error("secure storage backend failed")]
    Backend,
    #[error("password hashing failed")]
    PasswordHash,
    #[error("password verification is temporarily locked")]
    Locked,
}

pub trait SecureStore: Send + Sync {
    fn put(&self, name: &str, secret: &[u8]) -> Result<(), SecureStorageError>;
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, SecureStorageError>;
    fn delete(&self, name: &str) -> Result<(), SecureStorageError>;
}

fn valid_secret_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

#[cfg(windows)]
pub struct DpapiMachineStore {
    directory: PathBuf,
    entropy: Vec<u8>,
}

#[cfg(windows)]
impl DpapiMachineStore {
    pub fn new(directory: PathBuf, product_namespace: &str) -> Result<Self, SecureStorageError> {
        if product_namespace.is_empty() || !directory.is_absolute() {
            return Err(SecureStorageError::Backend);
        }
        fs::create_dir_all(&directory).map_err(|_| SecureStorageError::Backend)?;
        Ok(Self {
            directory,
            entropy: product_namespace.as_bytes().to_vec(),
        })
    }

    fn path(&self, name: &str) -> Result<PathBuf, SecureStorageError> {
        if !valid_secret_name(name) {
            return Err(SecureStorageError::Backend);
        }
        Ok(self.directory.join(format!("{name}.dpapi")))
    }

    fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), SecureStorageError> {
        let temporary = path.with_extension("dpapi.tmp");
        fs::write(&temporary, bytes).map_err(|_| SecureStorageError::Backend)?;
        fs::rename(temporary, path).map_err(|_| SecureStorageError::Backend)
    }
}

#[cfg(windows)]
impl SecureStore for DpapiMachineStore {
    fn put(&self, name: &str, secret: &[u8]) -> Result<(), SecureStorageError> {
        let encrypted =
            windows_dpapi::encrypt_data(secret, windows_dpapi::Scope::Machine, Some(&self.entropy))
                .map_err(|_| SecureStorageError::Backend)?;
        Self::atomic_write(&self.path(name)?, &encrypted)
    }

    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        let path = self.path(name)?;
        let encrypted = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(SecureStorageError::Backend),
        };
        windows_dpapi::decrypt_data(
            &encrypted,
            windows_dpapi::Scope::Machine,
            Some(&self.entropy),
        )
        .map(Some)
        .map_err(|_| SecureStorageError::Backend)
    }

    fn delete(&self, name: &str) -> Result<(), SecureStorageError> {
        match fs::remove_file(self.path(name)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(SecureStorageError::Backend),
        }
    }
}

#[cfg(unix)]
pub struct OsKeyringStore {
    service: String,
}

#[cfg(unix)]
impl OsKeyringStore {
    pub fn new(service: String) -> Result<Self, SecureStorageError> {
        if service.is_empty() || service.len() > 128 {
            return Err(SecureStorageError::Backend);
        }
        Ok(Self { service })
    }

    fn entry(&self, name: &str) -> Result<keyring::Entry, SecureStorageError> {
        if !valid_secret_name(name) {
            return Err(SecureStorageError::Backend);
        }
        keyring::Entry::new(&self.service, name).map_err(|_| SecureStorageError::Backend)
    }
}

#[cfg(unix)]
impl SecureStore for OsKeyringStore {
    fn put(&self, name: &str, secret: &[u8]) -> Result<(), SecureStorageError> {
        self.entry(name)?
            .set_secret(secret)
            .map_err(|_| SecureStorageError::Backend)
    }

    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        match self.entry(name)?.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(SecureStorageError::Backend),
        }
    }

    fn delete(&self, name: &str) -> Result<(), SecureStorageError> {
        match self.entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(SecureStorageError::Backend),
        }
    }
}

fn argon2id() -> Result<Argon2<'static>, SecureStorageError> {
    let params = Params::new(MEMORY_KIB, ITERATIONS, LANES, Some(32))
        .map_err(|_| SecureStorageError::PasswordHash)?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

pub fn hash_admin_password(password: SecretString) -> Result<String, SecureStorageError> {
    let salt = SaltString::generate(&mut OsRng);
    argon2id()?
        .hash_password(password.expose_secret().as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| SecureStorageError::PasswordHash)
}

pub fn verify_admin_password(
    password: SecretString,
    encoded_hash: &str,
) -> Result<bool, SecureStorageError> {
    let parsed = PasswordHash::new(encoded_hash).map_err(|_| SecureStorageError::PasswordHash)?;
    Ok(argon2id()?
        .verify_password(password.expose_secret().as_bytes(), &parsed)
        .is_ok())
}

pub fn password_hash_needs_upgrade(encoded_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(encoded_hash) else {
        return true;
    };
    parsed.algorithm.as_str() != "argon2id"
        || parsed.version != Some(19)
        || parsed.params.get_decimal("m") != Some(MEMORY_KIB)
        || parsed.params.get_decimal("t") != Some(ITERATIONS)
        || parsed.params.get_decimal("p") != Some(LANES)
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RateLimiter {
    failures: u32,
    blocked_until_epoch_seconds: i64,
}

impl RateLimiter {
    pub fn check(&self, now_epoch_seconds: i64) -> Result<(), SecureStorageError> {
        if now_epoch_seconds < self.blocked_until_epoch_seconds {
            Err(SecureStorageError::Locked)
        } else {
            Ok(())
        }
    }

    pub fn record_failure(&mut self, now_epoch_seconds: i64) {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= 5 {
            let exponent = (self.failures - 5).min(8);
            self.blocked_until_epoch_seconds = now_epoch_seconds + 30 * (1_i64 << exponent);
        }
    }

    pub fn record_success(&mut self) {
        self.failures = 0;
        self.blocked_until_epoch_seconds = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_uses_argon2id_and_verifies() {
        let hash = hash_admin_password(SecretString::from("correct horse".to_owned())).unwrap();
        assert!(hash.starts_with("$argon2id$v=19$m=65536,t=3,p=1$"));
        assert!(
            verify_admin_password(SecretString::from("correct horse".to_owned()), &hash).unwrap()
        );
        assert!(!verify_admin_password(SecretString::from("wrong".to_owned()), &hash).unwrap());
        assert!(!password_hash_needs_upgrade(&hash));
    }

    #[test]
    fn rate_limit_grows_after_repeated_failures() {
        let mut limiter = RateLimiter::default();
        for _ in 0..5 {
            limiter.record_failure(100);
        }
        assert!(matches!(
            limiter.check(129),
            Err(SecureStorageError::Locked)
        ));
        assert!(limiter.check(130).is_ok());
        limiter.record_success();
        assert!(limiter.check(100).is_ok());
    }

    #[test]
    fn secret_names_cannot_escape_the_backend_namespace() {
        assert!(valid_secret_name("device-private-key"));
        assert!(!valid_secret_name("../device-private-key"));
        assert!(!valid_secret_name("directory/name"));
    }
}
