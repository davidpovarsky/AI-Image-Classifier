use crate::{ChallengeRegistry, ControlPlaneError, DeviceRecord, HealthPayload};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use thiserror::Error;
use tokio_postgres::Client;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("PostgreSQL connection failed")]
    Connection,
    #[error("PostgreSQL operation failed")]
    Database,
    #[error("in-memory storage lock failed")]
    Memory,
    #[error("device is unknown")]
    UnknownDevice,
    #[error("challenge or nonce was replayed")]
    Replay,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecord {
    pub actor: String,
    pub action: String,
    pub device_id: Option<Uuid>,
    pub details: serde_json::Value,
    pub created_at: String,
}

#[derive(Clone)]
pub enum Store {
    Memory(Arc<MemoryStore>),
    Postgres(Arc<Client>),
}

#[derive(Default)]
pub struct MemoryStore {
    challenges: Mutex<ChallengeRegistry>,
    devices: Mutex<HashMap<Uuid, DeviceRecord>>,
    nonces: Mutex<HashSet<String>>,
    audits: Mutex<Vec<AuditRecord>>,
}

impl Store {
    pub fn memory() -> Self {
        Self::Memory(Arc::new(MemoryStore::default()))
    }

    pub async fn postgres(database_url: &str) -> Result<Self, StorageError> {
        if !database_url.contains("sslmode=require")
            && !database_url.contains("sslmode=verify-full")
        {
            return Err(StorageError::Connection);
        }
        let (tls, certificate_errors) =
            tokio_postgres_rustls::MakeRustlsConnect::with_native_certs()
                .map_err(|_| StorageError::Connection)?;
        if !certificate_errors.is_empty() {
            return Err(StorageError::Connection);
        }
        let (client, connection) = tokio_postgres::connect(database_url, tls)
            .await
            .map_err(|_| StorageError::Connection)?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                eprintln!("PostgreSQL connection terminated: {error}");
            }
        });
        let client = Arc::new(client);
        client
            .batch_execute(include_str!("../migrations/0001_production.sql"))
            .await
            .map_err(|_| StorageError::Database)?;
        Ok(Self::Postgres(client))
    }

    pub async fn issue_challenge(
        &self,
        device_id: Uuid,
        challenge: String,
    ) -> Result<(), StorageError> {
        match self {
            Self::Memory(store) => store
                .challenges
                .lock()
                .map_err(|_| StorageError::Memory)?
                .issue(device_id, challenge),
            Self::Postgres(client) => {
                client
                    .execute(
                        "INSERT INTO device_challenges(device_id, challenge, expires_at) VALUES($1,$2,now()+interval '120 seconds') ON CONFLICT(device_id) DO UPDATE SET challenge=excluded.challenge, expires_at=excluded.expires_at, consumed_at=NULL",
                        &[&device_id, &challenge],
                    )
                    .await
                    .map_err(|_| StorageError::Database)?;
            }
        }
        Ok(())
    }

    pub async fn consume_challenge(
        &self,
        device_id: Uuid,
        challenge: &str,
    ) -> Result<(), StorageError> {
        match self {
            Self::Memory(store) => store
                .challenges
                .lock()
                .map_err(|_| StorageError::Memory)?
                .consume(device_id, challenge)
                .map_err(|_| StorageError::Replay),
            Self::Postgres(client) => {
                let changed = client
                    .execute(
                        "UPDATE device_challenges SET consumed_at=now() WHERE device_id=$1 AND challenge=$2 AND consumed_at IS NULL AND expires_at>=now()",
                        &[&device_id, &challenge],
                    )
                    .await
                    .map_err(|_| StorageError::Database)?;
                if changed == 1 {
                    Ok(())
                } else {
                    Err(StorageError::Replay)
                }
            }
        }
    }

    pub async fn upsert_device(&self, device: &DeviceRecord) -> Result<(), StorageError> {
        match self {
            Self::Memory(store) => {
                store.devices.lock().map_err(|_| StorageError::Memory)?.insert(device.device_id, device.clone());
                Ok(())
            }
            Self::Postgres(client) => client
                .execute(
                    "INSERT INTO devices(device_id,tenant_id,public_key,policy_channel,acknowledged_revision,license_entitlement) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(device_id) DO UPDATE SET tenant_id=excluded.tenant_id,public_key=excluded.public_key,updated_at=now()",
                    &[&device.device_id, &device.tenant_id, &device.public_key, &device.policy_channel, &(device.acknowledged_revision as i64), &device.license_entitlement],
                )
                .await
                .map(|_| ())
                .map_err(|_| StorageError::Database),
        }
    }

    pub async fn device(&self, device_id: Uuid) -> Result<DeviceRecord, StorageError> {
        match self {
            Self::Memory(store) => store
                .devices
                .lock()
                .map_err(|_| StorageError::Memory)?
                .get(&device_id)
                .cloned()
                .ok_or(StorageError::UnknownDevice),
            Self::Postgres(client) => {
                let row = client
                    .query_opt("SELECT device_id,tenant_id,public_key,policy_channel,acknowledged_revision,license_entitlement FROM devices WHERE device_id=$1", &[&device_id])
                    .await
                    .map_err(|_| StorageError::Database)?
                    .ok_or(StorageError::UnknownDevice)?;
                Ok(DeviceRecord {
                    device_id: row.get(0),
                    tenant_id: row.get(1),
                    public_key: row.get(2),
                    policy_channel: row.get(3),
                    acknowledged_revision: u64::try_from(row.get::<_, i64>(4))
                        .map_err(|_| StorageError::Database)?,
                    license_entitlement: row.get(5),
                })
            }
        }
    }

    pub async fn consume_nonce(&self, device_id: Uuid, nonce: &str) -> Result<(), StorageError> {
        match self {
            Self::Memory(store) => {
                let key = format!("{device_id}:{nonce}");
                if store.nonces.lock().map_err(|_| StorageError::Memory)?.insert(key) { Ok(()) } else { Err(StorageError::Replay) }
            }
            Self::Postgres(client) => client
                .execute("INSERT INTO device_nonces(device_id,nonce,expires_at) VALUES($1,$2,now()+interval '10 minutes')", &[&device_id, &nonce])
                .await
                .map(|_| ())
                .map_err(|_| StorageError::Replay),
        }
    }

    pub(crate) async fn record_health(
        &self,
        device_id: Uuid,
        health: &HealthPayload,
    ) -> Result<(), StorageError> {
        if let Self::Postgres(client) = self {
            let value = serde_json::to_value(health).map_err(|_| StorageError::Database)?;
            let changed = client.execute("UPDATE devices SET last_health=$2,last_seen_at=now(),updated_at=now() WHERE device_id=$1", &[&device_id, &value]).await.map_err(|_| StorageError::Database)?;
            if changed != 1 {
                return Err(StorageError::UnknownDevice);
            }
        }
        Ok(())
    }

    pub async fn acknowledge(&self, device_id: Uuid, revision: u64) -> Result<(), StorageError> {
        match self {
            Self::Memory(store) => {
                let mut devices = store.devices.lock().map_err(|_| StorageError::Memory)?;
                let device = devices
                    .get_mut(&device_id)
                    .ok_or(StorageError::UnknownDevice)?;
                if revision < device.acknowledged_revision {
                    return Err(StorageError::Replay);
                }
                device.acknowledged_revision = revision;
                Ok(())
            }
            Self::Postgres(client) => {
                let revision = i64::try_from(revision).map_err(|_| StorageError::Database)?;
                let changed = client.execute("UPDATE devices SET acknowledged_revision=$2,updated_at=now() WHERE device_id=$1 AND acknowledged_revision<=$2", &[&device_id, &revision]).await.map_err(|_| StorageError::Database)?;
                if changed == 1 {
                    Ok(())
                } else {
                    Err(StorageError::Replay)
                }
            }
        }
    }

    pub async fn set_policy_channel(
        &self,
        tenant_id: Uuid,
        device_id: Uuid,
        channel: &str,
        actor: &str,
    ) -> Result<DeviceRecord, StorageError> {
        if !matches!(channel, "stable" | "beta") {
            return Err(StorageError::Database);
        }
        match self {
            Self::Memory(store) => {
                let mut devices = store.devices.lock().map_err(|_| StorageError::Memory)?;
                let device = devices
                    .get_mut(&device_id)
                    .filter(|d| d.tenant_id == tenant_id)
                    .ok_or(StorageError::UnknownDevice)?;
                device.policy_channel = channel.to_owned();
                let result = device.clone();
                drop(devices);
                store
                    .audits
                    .lock()
                    .map_err(|_| StorageError::Memory)?
                    .push(AuditRecord {
                        actor: actor.into(),
                        action: "policy-channel".into(),
                        device_id: Some(device_id),
                        details: serde_json::json!({"channel": channel}),
                        created_at: time::OffsetDateTime::now_utc().to_string(),
                    });
                Ok(result)
            }
            Self::Postgres(client) => {
                let details = serde_json::json!({"channel": channel});
                let row = client.query_opt(
                    "WITH updated AS (UPDATE devices SET policy_channel=$3,policy_refresh_requested=true,updated_at=now() WHERE tenant_id=$1 AND device_id=$2 RETURNING device_id,tenant_id,public_key,policy_channel,acknowledged_revision,license_entitlement), audited AS (INSERT INTO audit_log(tenant_id,actor,action,device_id,details) SELECT $1,$4,'policy-channel',$2,$5 FROM updated RETURNING id) SELECT device_id,tenant_id,public_key,policy_channel,acknowledged_revision,license_entitlement FROM updated WHERE EXISTS (SELECT 1 FROM audited)",
                    &[&tenant_id, &device_id, &channel, &actor, &details],
                ).await.map_err(|_| StorageError::Database)?.ok_or(StorageError::UnknownDevice)?;
                Ok(DeviceRecord {
                    device_id: row.get(0),
                    tenant_id: row.get(1),
                    public_key: row.get(2),
                    policy_channel: row.get(3),
                    acknowledged_revision: u64::try_from(row.get::<_, i64>(4))
                        .map_err(|_| StorageError::Database)?,
                    license_entitlement: row.get(5),
                })
            }
        }
    }

    pub async fn request_policy_refresh(
        &self,
        tenant_id: Uuid,
        device_id: Uuid,
        actor: &str,
    ) -> Result<(), StorageError> {
        match self {
            Self::Memory(store) => {
                if store
                    .devices
                    .lock()
                    .map_err(|_| StorageError::Memory)?
                    .get(&device_id)
                    .is_none_or(|device| device.tenant_id != tenant_id)
                {
                    return Err(StorageError::UnknownDevice);
                }
            }
            Self::Postgres(client) => {
                let changed = client.execute(
                    "WITH updated AS (UPDATE devices SET policy_refresh_requested=true,updated_at=now() WHERE tenant_id=$1 AND device_id=$2 RETURNING device_id) INSERT INTO audit_log(tenant_id,actor,action,device_id,details) SELECT $1,$3,'policy-refresh',$2,'{}'::jsonb FROM updated",
                    &[&tenant_id, &device_id, &actor],
                ).await.map_err(|_| StorageError::Database)?;
                if changed == 0 {
                    return Err(StorageError::UnknownDevice);
                }
                return Ok(());
            }
        }
        self.audit(
            tenant_id,
            actor,
            "policy-refresh",
            Some(device_id),
            serde_json::json!({}),
        )
        .await
    }

    pub async fn audit(
        &self,
        tenant_id: Uuid,
        actor: &str,
        action: &str,
        device_id: Option<Uuid>,
        details: serde_json::Value,
    ) -> Result<(), StorageError> {
        match self {
            Self::Memory(store) => {
                store
                    .audits
                    .lock()
                    .map_err(|_| StorageError::Memory)?
                    .push(AuditRecord {
                        actor: actor.into(),
                        action: action.into(),
                        device_id,
                        details,
                        created_at: time::OffsetDateTime::now_utc().to_string(),
                    })
            }
            Self::Postgres(client) => {
                client.execute("INSERT INTO audit_log(tenant_id,actor,action,device_id,details) VALUES($1,$2,$3,$4,$5)", &[&tenant_id,&actor,&action,&device_id,&details]).await.map_err(|_| StorageError::Database)?;
            }
        }
        Ok(())
    }

    pub async fn audit_history(
        &self,
        tenant_id: Uuid,
        device_id: Uuid,
    ) -> Result<Vec<AuditRecord>, StorageError> {
        match self {
            Self::Memory(store) => Ok(store
                .audits
                .lock()
                .map_err(|_| StorageError::Memory)?
                .iter()
                .filter(|row| row.device_id == Some(device_id))
                .cloned()
                .collect()),
            Self::Postgres(client) => {
                let rows = client.query("SELECT actor,action,device_id,details,created_at::text FROM audit_log WHERE tenant_id=$1 AND device_id=$2 ORDER BY created_at DESC LIMIT 100", &[&tenant_id,&device_id]).await.map_err(|_| StorageError::Database)?;
                Ok(rows
                    .into_iter()
                    .map(|row| AuditRecord {
                        actor: row.get(0),
                        action: row.get(1),
                        device_id: row.get(2),
                        details: row.get(3),
                        created_at: row.get(4),
                    })
                    .collect())
            }
        }
    }

    pub async fn record_keygen_webhook(
        &self,
        event_id: &str,
        event_type: &str,
        payload: serde_json::Value,
    ) -> Result<(), StorageError> {
        match self {
            Self::Memory(store) => {
                let key = format!("keygen:{event_id}");
                if !store
                    .nonces
                    .lock()
                    .map_err(|_| StorageError::Memory)?
                    .insert(key)
                {
                    return Err(StorageError::Replay);
                }
            }
            Self::Postgres(client) => {
                match client.execute(
                    "INSERT INTO keygen_webhook_events(event_id,event_type,payload) VALUES($1,$2,$3)",
                    &[&event_id, &event_type, &payload],
                ).await {
                    Ok(_) => {}
                    Err(error) if error.as_db_error().is_some_and(|database| {
                        database.code() == &tokio_postgres::error::SqlState::UNIQUE_VIOLATION
                    }) => return Err(StorageError::Replay),
                    Err(_) => return Err(StorageError::Database),
                }
            }
        }
        Ok(())
    }
}

impl From<ControlPlaneError> for StorageError {
    fn from(_: ControlPlaneError) -> Self {
        Self::Replay
    }
}
