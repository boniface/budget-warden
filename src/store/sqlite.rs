use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx_core::query::query;
use sqlx_core::row::Row;
use sqlx_sqlite::{Sqlite, SqlitePool, SqliteTransaction};

use super::codec;
use crate::error::StoreError;
use crate::reservation::ReservationId;
use crate::store::{
    BudgetStore, CounterUsage, ReserveRequest, StoreKey, StoreReservation, StoreReservationResult,
};
use crate::window::WindowBounds;

/// SQLite-backed store for single-node and embedded deployments.
#[derive(Debug, Clone)]
pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    /// Creates a `SQLite` store from a pool.
    #[must_use]
    pub const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Returns the underlying pool.
    #[must_use]
    pub const fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Creates required tables if they do not exist.
    ///
    /// # Errors
    ///
    /// Returns an error when schema setup fails.
    pub async fn setup_schema(&self) -> Result<(), StoreError> {
        for statement in include_str!("../../migrations/sqlite/001_budget_warden.sql")
            .split(';')
            .map(str::trim)
            .filter(|statement| !statement.is_empty())
        {
            query::<Sqlite>(statement)
                .execute(&self.pool)
                .await
                .map_err(sql_error)?;
        }
        Ok(())
    }
}

#[async_trait]
impl BudgetStore for SqliteStore {
    async fn reserve_if_allowed(
        &self,
        request: ReserveRequest<'_>,
    ) -> Result<StoreReservationResult, StoreError> {
        request.validate()?;
        let store_key = codec::key(request.key);
        let amount = codec::to_i64(request.amount, "amount")?;
        let effective_limit = codec::to_i64(request.effective_limit, "effective limit")?;
        let expires_at = request.now + request.reservation_ttl;
        let mut tx = self.pool.begin().await.map_err(sql_error)?;

        cleanup_expired(&mut tx, request.now).await?;
        if let Some(idempotency_key) = request.idempotency_key
            && let Some(result) = existing_idempotent_reservation(
                &mut tx,
                &store_key,
                request.window,
                idempotency_key.as_str(),
            )
            .await?
        {
            tx.commit().await.map_err(sql_error)?;
            return Ok(result);
        }

        query::<Sqlite>(
            "INSERT OR IGNORE INTO budget_warden_counters \
             (store_key, window_start, window_end, committed, reserved) VALUES (?, ?, ?, 0, 0)",
        )
        .bind(&store_key)
        .bind(codec::window_start(request.window))
        .bind(codec::window_end(request.window))
        .execute(&mut *tx)
        .await
        .map_err(sql_error)?;

        let update = query::<Sqlite>(
            "UPDATE budget_warden_counters SET reserved = reserved + ?, updated_at = CURRENT_TIMESTAMP \
             WHERE store_key = ? AND window_start = ? AND window_end = ? \
             AND committed + reserved + ? <= ?",
        )
        .bind(amount)
        .bind(&store_key)
        .bind(codec::window_start(request.window))
        .bind(codec::window_end(request.window))
        .bind(amount)
        .bind(effective_limit)
        .execute(&mut *tx)
        .await
        .map_err(sql_error)?;

        if update.rows_affected() == 0 {
            let usage = load_usage(&mut tx, &store_key, request.window).await?;
            tx.commit().await.map_err(sql_error)?;
            return Ok(StoreReservationResult::denied(usage));
        }

        let idempotency_key = request
            .idempotency_key
            .map(crate::reservation::IdempotencyKey::as_str);
        let row = query::<Sqlite>(
            "INSERT INTO budget_warden_reservations \
             (store_key, window_start, window_end, amount, status, expires_at, idempotency_key) \
             VALUES (?, ?, ?, ?, 'active', ?, ?) RETURNING reservation_id",
        )
        .bind(&store_key)
        .bind(codec::window_start(request.window))
        .bind(codec::window_end(request.window))
        .bind(amount)
        .bind(expires_at)
        .bind(idempotency_key)
        .fetch_one(&mut *tx)
        .await;
        let row = match row {
            Ok(row) => row,
            Err(error) if request.idempotency_key.is_some() => {
                let store_error = sql_error(error);
                tx.rollback().await.map_err(sql_error)?;
                if let Some(result) =
                    existing_idempotent_reservation_from_pool(&self.pool, &store_key, request)
                        .await?
                {
                    return Ok(result);
                }
                return Err(store_error);
            }
            Err(error) => return Err(sql_error(error)),
        };

        let usage = load_usage(&mut tx, &store_key, request.window).await?;
        tx.commit().await.map_err(sql_error)?;
        Ok(StoreReservationResult::allowed(
            StoreReservation::new(codec::id_from_i64(row.get("reservation_id"))?, expires_at),
            usage,
        ))
    }

    async fn commit(&self, reservation_id: ReservationId) -> Result<(), StoreError> {
        finish_reservation(&self.pool, reservation_id, FinishAction::Commit).await
    }

    async fn refund(&self, reservation_id: ReservationId) -> Result<(), StoreError> {
        finish_reservation(&self.pool, reservation_id, FinishAction::Refund).await
    }

    async fn usage(
        &self,
        key: &StoreKey,
        window: WindowBounds,
        now: DateTime<Utc>,
    ) -> Result<CounterUsage, StoreError> {
        let mut tx = self.pool.begin().await.map_err(sql_error)?;
        cleanup_expired(&mut tx, now).await?;
        let usage = load_usage(&mut tx, &codec::key(key), window).await?;
        tx.commit().await.map_err(sql_error)?;
        Ok(usage)
    }
}

async fn existing_idempotent_reservation(
    tx: &mut SqliteTransaction<'_>,
    store_key: &str,
    window: WindowBounds,
    idempotency_key: &str,
) -> Result<Option<StoreReservationResult>, StoreError> {
    let row = query::<Sqlite>(
        "SELECT reservation_id, expires_at FROM budget_warden_reservations \
         WHERE store_key = ? AND window_start = ? AND window_end = ? AND idempotency_key = ? \
         AND status IN ('active', 'committed') LIMIT 1",
    )
    .bind(store_key)
    .bind(codec::window_start(window))
    .bind(codec::window_end(window))
    .bind(idempotency_key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(sql_error)?;

    let Some(row) = row else {
        return Ok(None);
    };
    let usage = load_usage(tx, store_key, window).await?;
    Ok(Some(StoreReservationResult::allowed(
        StoreReservation::new(
            codec::id_from_i64(row.get("reservation_id"))?,
            row.get("expires_at"),
        ),
        usage,
    )))
}

async fn existing_idempotent_reservation_from_pool(
    pool: &SqlitePool,
    store_key: &str,
    request: ReserveRequest<'_>,
) -> Result<Option<StoreReservationResult>, StoreError> {
    let Some(idempotency_key) = request.idempotency_key else {
        return Ok(None);
    };
    let mut tx = pool.begin().await.map_err(sql_error)?;
    let result = existing_idempotent_reservation(
        &mut tx,
        store_key,
        request.window,
        idempotency_key.as_str(),
    )
    .await?;
    tx.commit().await.map_err(sql_error)?;
    Ok(result)
}

async fn cleanup_expired(
    tx: &mut SqliteTransaction<'_>,
    now: DateTime<Utc>,
) -> Result<(), StoreError> {
    let expired = query::<Sqlite>(
        "SELECT reservation_id, store_key, window_start, window_end, amount \
         FROM budget_warden_reservations WHERE status = 'active' AND expires_at <= ?",
    )
    .bind(now)
    .fetch_all(&mut **tx)
    .await
    .map_err(sql_error)?;

    for row in expired {
        let reservation_id: i64 = row.get("reservation_id");
        let amount: i64 = row.get("amount");
        query::<Sqlite>(
            "UPDATE budget_warden_counters SET reserved = MAX(reserved - ?, 0), updated_at = CURRENT_TIMESTAMP \
             WHERE store_key = ? AND window_start = ? AND window_end = ?",
        )
        .bind(amount)
        .bind(row.get::<String, _>("store_key"))
        .bind(row.get::<DateTime<Utc>, _>("window_start"))
        .bind(row.get::<DateTime<Utc>, _>("window_end"))
        .execute(&mut **tx)
        .await
        .map_err(sql_error)?;
        query::<Sqlite>(
            "UPDATE budget_warden_reservations SET status = 'expired' WHERE reservation_id = ?",
        )
        .bind(reservation_id)
        .execute(&mut **tx)
        .await
        .map_err(sql_error)?;
    }

    Ok(())
}

async fn load_usage(
    tx: &mut SqliteTransaction<'_>,
    store_key: &str,
    window: WindowBounds,
) -> Result<CounterUsage, StoreError> {
    let row = query::<Sqlite>(
        "SELECT committed, reserved FROM budget_warden_counters \
         WHERE store_key = ? AND window_start = ? AND window_end = ?",
    )
    .bind(store_key)
    .bind(codec::window_start(window))
    .bind(codec::window_end(window))
    .fetch_optional(&mut **tx)
    .await
    .map_err(sql_error)?;

    row.map_or_else(
        || Ok(CounterUsage::default()),
        |row| codec::usage_from_i64(row.get("committed"), row.get("reserved")),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FinishAction {
    Commit,
    Refund,
}

async fn finish_reservation(
    pool: &SqlitePool,
    reservation_id: ReservationId,
    action: FinishAction,
) -> Result<(), StoreError> {
    let id = codec::to_i64(reservation_id.as_u64(), "reservation id")?;
    let mut tx = pool.begin().await.map_err(sql_error)?;
    let row = query::<Sqlite>(
        "SELECT store_key, window_start, window_end, amount, status, expires_at \
         FROM budget_warden_reservations WHERE reservation_id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(sql_error)?;

    let Some(row) = row else {
        return Err(StoreError::ReservationNotFound);
    };
    let status: String = row.get("status");
    match status.as_str() {
        "committed" | "refunded" => {
            tx.commit().await.map_err(sql_error)?;
            Ok(())
        }
        "expired" => {
            tx.commit().await.map_err(sql_error)?;
            Err(StoreError::ReservationExpired)
        }
        "active" => {
            let amount: i64 = row.get("amount");
            let expires_at: DateTime<Utc> = row.get("expires_at");
            if expires_at <= Utc::now() {
                query::<Sqlite>(
                    "UPDATE budget_warden_counters SET reserved = MAX(reserved - ?, 0), updated_at = CURRENT_TIMESTAMP \
                     WHERE store_key = ? AND window_start = ? AND window_end = ?",
                )
                .bind(amount)
                .bind(row.get::<String, _>("store_key"))
                .bind(row.get::<DateTime<Utc>, _>("window_start"))
                .bind(row.get::<DateTime<Utc>, _>("window_end"))
                .execute(&mut *tx)
                .await
                .map_err(sql_error)?;
                query::<Sqlite>(
                    "UPDATE budget_warden_reservations SET status = 'expired' WHERE reservation_id = ?",
                )
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(sql_error)?;
                tx.commit().await.map_err(sql_error)?;
                return Err(StoreError::ReservationExpired);
            }
            let (committed_delta, status) = match action {
                FinishAction::Commit => (amount, "committed"),
                FinishAction::Refund => (0, "refunded"),
            };
            query::<Sqlite>(
                "UPDATE budget_warden_counters SET reserved = MAX(reserved - ?, 0), \
                 committed = committed + ?, updated_at = CURRENT_TIMESTAMP \
                 WHERE store_key = ? AND window_start = ? AND window_end = ?",
            )
            .bind(amount)
            .bind(committed_delta)
            .bind(row.get::<String, _>("store_key"))
            .bind(row.get::<DateTime<Utc>, _>("window_start"))
            .bind(row.get::<DateTime<Utc>, _>("window_end"))
            .execute(&mut *tx)
            .await
            .map_err(sql_error)?;
            query::<Sqlite>(
                "UPDATE budget_warden_reservations SET status = ? WHERE reservation_id = ?",
            )
            .bind(status)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(sql_error)?;
            tx.commit().await.map_err(sql_error)?;
            Ok(())
        }
        _ => Err(StoreError::Other("unknown reservation status".to_owned())),
    }
}

fn sql_error(error: sqlx_core::Error) -> StoreError {
    let message = error.to_string();
    drop(error);
    StoreError::Other(message)
}
