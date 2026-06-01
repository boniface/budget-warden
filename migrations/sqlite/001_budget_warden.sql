CREATE TABLE IF NOT EXISTS budget_warden_counters (
    store_key TEXT NOT NULL,
    window_start TEXT NOT NULL,
    window_end TEXT NOT NULL,
    committed INTEGER NOT NULL DEFAULT 0 CHECK (committed >= 0),
    reserved INTEGER NOT NULL DEFAULT 0 CHECK (reserved >= 0),
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (store_key, window_start, window_end)
);

CREATE TABLE IF NOT EXISTS budget_warden_reservations (
    reservation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    store_key TEXT NOT NULL,
    window_start TEXT NOT NULL,
    window_end TEXT NOT NULL,
    amount INTEGER NOT NULL CHECK (amount > 0),
    status TEXT NOT NULL CHECK (status IN ('active', 'committed', 'refunded', 'expired')),
    expires_at TEXT NOT NULL,
    idempotency_key TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS budget_warden_reservations_active_expiry_idx
    ON budget_warden_reservations (expires_at)
    WHERE status = 'active';

CREATE UNIQUE INDEX IF NOT EXISTS budget_warden_reservations_idempotency_idx
    ON budget_warden_reservations (store_key, window_start, window_end, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
