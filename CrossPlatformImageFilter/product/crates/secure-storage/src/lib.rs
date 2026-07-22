#![forbid(unsafe_code)]

use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
#[cfg(unix)]
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, AeadCore, KeyInit, OsRng as AeadOsRng, Payload},
};
use rand_core::OsRng;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
#[cfg(any(windows, unix))]
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
pub struct MachineFileStore {
    directory: PathBuf,
    cipher: XChaCha20Poly1305,
    namespace: Vec<u8>,
}

#[cfg(unix)]
impl MachineFileStore {
    #[cfg(target_os = "macos")]
    pub fn new_or_create_key(
        directory: PathBuf,
        key_path: &Path,
        product_namespace: &str,
    ) -> Result<Self, SecureStorageError> {
        if !key_path.exists() {
            let parent = key_path.parent().ok_or(SecureStorageError::Backend)?;
            fs::create_dir_all(parent).map_err(|_| SecureStorageError::Backend)?;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|_| SecureStorageError::Backend)?;
            let key = XChaCha20Poly1305::generate_key(&mut AeadOsRng);
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true).mode(0o600);
            match options.open(key_path) {
                Ok(mut file) => {
                    std::io::Write::write_all(&mut file, &key)
                        .map_err(|_| SecureStorageError::Backend)?;
                    file.sync_all().map_err(|_| SecureStorageError::Backend)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(SecureStorageError::Backend),
            }
        }
        Self::new(directory, key_path, product_namespace)
    }

    pub fn new(
        directory: PathBuf,
        key_path: &Path,
        product_namespace: &str,
    ) -> Result<Self, SecureStorageError> {
        if product_namespace.is_empty() || !directory.is_absolute() || !key_path.is_absolute() {
            return Err(SecureStorageError::Backend);
        }
        let key: [u8; 32] = fs::read(key_path)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(SecureStorageError::Backend)?;
        fs::create_dir_all(&directory).map_err(|_| SecureStorageError::Backend)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| SecureStorageError::Backend)?;
        Ok(Self {
            directory,
            cipher: XChaCha20Poly1305::new((&key).into()),
            namespace: product_namespace.as_bytes().to_vec(),
        })
    }

    fn path(&self, name: &str) -> Result<PathBuf, SecureStorageError> {
        if !valid_secret_name(name) {
            return Err(SecureStorageError::Backend);
        }
        Ok(self.directory.join(format!("{name}.secret")))
    }

    fn associated_data(&self, name: &str) -> Vec<u8> {
        let mut value = self.namespace.clone();
        value.push(0);
        value.extend_from_slice(name.as_bytes());
        value
    }

    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> Result<(), SecureStorageError> {
        let temporary = path.with_extension("secret.tmp");
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true).mode(0o600);
        let mut file = options
            .open(&temporary)
            .map_err(|_| SecureStorageError::Backend)?;
        std::io::Write::write_all(&mut file, bytes).map_err(|_| SecureStorageError::Backend)?;
        file.sync_all().map_err(|_| SecureStorageError::Backend)?;
        fs::rename(temporary, path).map_err(|_| SecureStorageError::Backend)
    }
}

#[cfg(unix)]
impl SecureStore for MachineFileStore {
    fn put(&self, name: &str, secret: &[u8]) -> Result<(), SecureStorageError> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut AeadOsRng);
        let encrypted = self
            .cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: secret,
                    aad: &self.associated_data(name),
                },
            )
            .map_err(|_| SecureStorageError::Backend)?;
        let mut stored = nonce.to_vec();
        stored.extend_from_slice(&encrypted);
        self.atomic_write(&self.path(name)?, &stored)
    }

    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        let stored = match fs::read(self.path(name)?) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(SecureStorageError::Backend),
        };
        if stored.len() < 24 {
            return Err(SecureStorageError::Backend);
        }
        let (nonce, encrypted) = stored.split_at(24);
        self.cipher
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: encrypted,
                    aad: &self.associated_data(name),
                },
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

    #[cfg(unix)]
    #[test]
    fn machine_store_encrypts_at_rest_and_rejects_tampering() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "local-filter-secure-store-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let key_path = root.join("machine.key");
        fs::write(&key_path, [9_u8; 32]).unwrap();
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600)).unwrap();
        let secrets = root.join("secrets");
        let store = MachineFileStore::new(secrets.clone(), &key_path, "test.namespace").unwrap();
        store.put("license-key", b"commercial-secret").unwrap();
        let stored_path = secrets.join("license-key.secret");
        let stored = fs::read(&stored_path).unwrap();
        assert!(
            !stored
                .windows(17)
                .any(|value| value == b"commercial-secret")
        );
        assert_eq!(
            store.get("license-key").unwrap().unwrap(),
            b"commercial-secret"
        );
        let mut tampered = stored;
        *tampered.last_mut().unwrap() ^= 1;
        fs::write(&stored_path, tampered).unwrap();
        assert!(matches!(
            store.get("license-key"),
            Err(SecureStorageError::Backend)
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
