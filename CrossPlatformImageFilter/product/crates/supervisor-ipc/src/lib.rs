#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{Read, Write};
use thiserror::Error;
use uuid::Uuid;

pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "payload")]
pub enum Request {
    GetStatus,
    GetHealth,
    StartFiltering,
    StopFiltering {
        authorization: String,
    },
    PauseFiltering {
        authorization: String,
        seconds: u32,
    },
    ResumeFiltering,
    SetAdminPassword {
        password: String,
    },
    VerifyAdminPassword {
        password: String,
        scope: String,
    },
    ActivateLicense {
        license_key: String,
    },
    ImportOfflineLicense {
        license: Vec<u8>,
    },
    DeactivateDevice {
        authorization: String,
    },
    CheckPolicyUpdate,
    GetEffectivePolicySummary,
    ApplyPolicyAssignment {
        authorization: String,
        assignment: String,
    },
    GetDiagnosticsSummary,
    ExportSupportBundle {
        authorization: String,
    },
    RequestUninstallAuthorization {
        authorization: String,
    },
    PrepareUninstall {
        authorization: String,
    },
    InstallOrRepairCertificate {
        authorization: String,
    },
    RepairNetworkConfiguration {
        authorization: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub protocol_version: u16,
    pub request_id: Uuid,
    pub nonce: String,
    pub timestamp: i64,
    #[serde(flatten)]
    pub request: Request,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StructuredError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Response<T> {
    pub protocol_version: u16,
    pub request_id: Uuid,
    pub timestamp: i64,
    pub status: ResponseStatus,
    pub payload: Option<T>,
    pub error: Option<StructuredError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerIdentity {
    pub user_id: String,
    pub process_id: u32,
    pub executable_identity: String,
    pub is_elevated: bool,
}

pub trait PeerVerifier {
    fn verify(&self, peer: &PeerIdentity) -> bool;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("IPC message exceeds {MAX_MESSAGE_BYTES} bytes")]
    Oversized,
    #[error("malformed IPC message: {0}")]
    Malformed(String),
    #[error("unsupported IPC protocol version")]
    UnsupportedVersion,
    #[error("IPC peer identity is not authorized")]
    UnauthorizedPeer,
    #[error("IPC frame is truncated")]
    Truncated,
    #[error("IPC transport failed: {0}")]
    Transport(String),
}

pub fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<(), ProtocolError> {
    let bytes =
        serde_json::to_vec(value).map_err(|error| ProtocolError::Malformed(error.to_string()))?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::Oversized);
    }
    let length = u32::try_from(bytes.len()).map_err(|_| ProtocolError::Oversized)?;
    writer
        .write_all(&length.to_be_bytes())
        .and_then(|()| writer.write_all(&bytes))
        .map_err(|error| ProtocolError::Transport(error.to_string()))
}

pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T, ProtocolError> {
    let mut prefix = [0_u8; 4];
    reader
        .read_exact(&mut prefix)
        .map_err(|_| ProtocolError::Truncated)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::Oversized);
    }
    let mut bytes = vec![0_u8; length];
    reader
        .read_exact(&mut bytes)
        .map_err(|_| ProtocolError::Truncated)?;
    serde_json::from_slice(&bytes).map_err(|error| ProtocolError::Malformed(error.to_string()))
}

pub fn decode(
    bytes: &[u8],
    peer: &PeerIdentity,
    verifier: &impl PeerVerifier,
) -> Result<Envelope, ProtocolError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::Oversized);
    }
    if !verifier.verify(peer) {
        return Err(ProtocolError::UnauthorizedPeer);
    }
    let envelope: Envelope = serde_json::from_slice(bytes)
        .map_err(|error| ProtocolError::Malformed(error.to_string()))?;
    if envelope.protocol_version != 1 {
        return Err(ProtocolError::UnsupportedVersion);
    }
    if envelope.nonce.len() < 32 || envelope.nonce.len() > 256 {
        return Err(ProtocolError::Malformed("nonce length is invalid".into()));
    }
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Allow(bool);
    impl PeerVerifier for Allow {
        fn verify(&self, _peer: &PeerIdentity) -> bool {
            self.0
        }
    }

    fn peer() -> PeerIdentity {
        PeerIdentity {
            user_id: "S-1-5-21-test".into(),
            process_id: 42,
            executable_identity: "signed-ui".into(),
            is_elevated: false,
        }
    }

    #[test]
    fn narrow_protocol_round_trip() {
        let envelope = Envelope {
            protocol_version: 1,
            request_id: Uuid::nil(),
            nonce: "a".repeat(32),
            timestamp: 1_700_000_000,
            request: Request::GetStatus,
        };
        let bytes = serde_json::to_vec(&envelope).unwrap();
        assert_eq!(decode(&bytes, &peer(), &Allow(true)).unwrap(), envelope);
    }

    #[test]
    fn rejects_oversized_and_unauthorized_messages() {
        assert_eq!(
            decode(&vec![0; MAX_MESSAGE_BYTES + 1], &peer(), &Allow(true)),
            Err(ProtocolError::Oversized)
        );
        assert_eq!(
            decode(b"{}", &peer(), &Allow(false)),
            Err(ProtocolError::UnauthorizedPeer)
        );
    }

    #[test]
    fn arbitrary_commands_are_not_representable() {
        let raw = br#"{"protocolVersion":1,"requestId":"00000000-0000-0000-0000-000000000000","nonce":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","timestamp":1700000000,"method":"ExecuteShell","payload":{"command":"whoami"}}"#;
        assert!(matches!(
            decode(raw, &peer(), &Allow(true)),
            Err(ProtocolError::Malformed(_))
        ));
    }

    #[test]
    fn framing_round_trip_and_size_limit() {
        let response = Response {
            protocol_version: 1,
            request_id: Uuid::nil(),
            timestamp: 1_700_000_000,
            status: ResponseStatus::Ok,
            payload: Some("healthy".to_owned()),
            error: None,
        };
        let mut wire = Vec::new();
        write_frame(&mut wire, &response).unwrap();
        assert_eq!(
            read_frame::<Response<String>>(&mut wire.as_slice()).unwrap(),
            response
        );

        let oversized = (MAX_MESSAGE_BYTES as u32 + 1).to_be_bytes();
        assert_eq!(
            read_frame::<Envelope>(&mut oversized.as_slice()),
            Err(ProtocolError::Oversized)
        );
    }
}
