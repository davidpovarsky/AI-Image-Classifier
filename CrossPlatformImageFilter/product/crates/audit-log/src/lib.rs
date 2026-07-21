#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

const SENSITIVE_KEYS: &[&str] = &[
    "authorization",
    "browsingUrl",
    "devicePrivateKey",
    "image",
    "imageBytes",
    "licenseKey",
    "password",
    "passwordHash",
    "url",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,
    pub event_type: String,
    pub actor: String,
    pub outcome: String,
    pub details: Value,
    pub previous_hash: String,
    pub event_hash: String,
}

pub fn redact(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if SENSITIVE_KEYS
                    .iter()
                    .any(|candidate| key.eq_ignore_ascii_case(candidate))
                {
                    *child = Value::String("[REDACTED]".into());
                } else {
                    redact(child);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact),
        _ => {}
    }
}

pub fn chained_hash(previous_hash: &str, event_without_hash: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(previous_hash.as_bytes());
    digest.update(event_without_hash);
    format!("{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recursively_redacts_forbidden_diagnostics() {
        let mut value =
            json!({"ok": true, "nested": {"url": "https://secret", "imageBytes": "..."}});
        redact(&mut value);
        assert_eq!(value["nested"]["url"], "[REDACTED]");
        assert_eq!(value["nested"]["imageBytes"], "[REDACTED]");
    }

    #[test]
    fn audit_chain_is_order_sensitive() {
        assert_ne!(chained_hash("a", b"b"), chained_hash("b", b"a"));
    }
}
