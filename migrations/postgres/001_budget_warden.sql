CREATE TABLE IF NOT EXISTS budget_warden_counters (
    store_key TEXT NOT NULL,
    window_start TIMESTAMPTZ NOT NULL,
    window_end TIMESTAMPTZ NOT NULL,
    committed BIGINT NOT NULL DEFAULT 0 CHECK (committed >= 0),
    reserved BIGINT NOT NULL DEFAULT 0 CHECK (reserved >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (store_key, window_start, window_end)
);

CREATE TABLE IF NOT EXISTS budget_warden_reservations (
    reservation_id BIGSERIAL PRIMARY KEY,
    store_key TEXT NOT NULL,
    window_start TIMESTAMPTZ NOT NULL,
    window_end TIMESTAMPTZ NOT NULL,
    amount BIGINT NOT NULL CHECK (amount > 0),
    status TEXT NOT NULL CHECK (status IN ('active', 'committed', 'refunded', 'expired')),
    expires_at TIMESTAMPTZ NOT NULL,
    idempotency_key TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS budget_warden_reservations_active_expiry_idx
    ON budget_warden_reservations (expires_at)
    WHERE status = 'active';

CREATE UNIQUE INDEX IF NOT EXISTS budget_warden_reservations_idempotency_idx
    ON budget_warden_reservations (store_key, window_start, window_end, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
