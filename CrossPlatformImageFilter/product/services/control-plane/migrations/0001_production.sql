CREATE TABLE IF NOT EXISTS devices (
    device_id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    public_key bytea NOT NULL CHECK (octet_length(public_key) = 32),
    policy_channel text NOT NULL CHECK (policy_channel IN ('stable', 'beta')),
    acknowledged_revision bigint NOT NULL DEFAULT 0 CHECK (acknowledged_revision >= 0),
    license_entitlement text NOT NULL,
    last_health jsonb,
    last_seen_at timestamptz,
    policy_refresh_requested boolean NOT NULL DEFAULT false,
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS devices_tenant_idx ON devices(tenant_id, device_id);

CREATE TABLE IF NOT EXISTS device_challenges (
    device_id uuid PRIMARY KEY,
    challenge text NOT NULL,
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz
);

CREATE TABLE IF NOT EXISTS device_nonces (
    device_id uuid NOT NULL,
    nonce text NOT NULL,
    expires_at timestamptz NOT NULL,
    PRIMARY KEY(device_id, nonce)
);

CREATE TABLE IF NOT EXISTS audit_log (
    id bigserial PRIMARY KEY,
    tenant_id uuid NOT NULL,
    actor text NOT NULL,
    action text NOT NULL,
    device_id uuid,
    details jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS audit_tenant_device_idx ON audit_log(tenant_id, device_id, created_at DESC);

CREATE TABLE IF NOT EXISTS keygen_webhook_events (
    event_id text PRIMARY KEY,
    event_type text NOT NULL,
    payload jsonb NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now()
);
