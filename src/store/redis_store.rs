use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_redis::Pool;
use redis::Script;

use super::codec;
use crate::error::StoreError;
use crate::reservation::ReservationId;
use crate::store::{
    BudgetStore, CounterUsage, ReserveRequest, StoreKey, StoreReservation, StoreReservationResult,
};
use crate::window::WindowBounds;

const EXPIRATIONS_KEY: &str = "budget-warden:expirations";
const SEQUENCE_KEY: &str = "budget-warden:reservation-seq";

/// Redis-backed distributed counter store.
#[derive(Debug, Clone)]
pub struct RedisStore {
    pool: Pool,
}

impl RedisStore {
    /// Creates a Redis store from a connection pool.
    #[must_use]
    pub const fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Returns the underlying pool.
    #[must_use]
    pub const fn pool(&self) -> &Pool {
        &self.pool
    }
}

#[async_trait]
impl BudgetStore for RedisStore {
    async fn reserve_if_allowed(
        &self,
        request: ReserveRequest<'_>,
    ) -> Result<StoreReservationResult, StoreError> {
        request.validate()?;
        let mut connection = self.pool.get().await.map_err(pool_error)?;
        let counter_key = redis_counter_key(request.key, request.window);
        let idempotency_key = redis_idempotency_key(request.key, request.window);
        let amount = codec::to_i64(request.amount, "amount")?;
        let effective_limit = codec::to_i64(request.effective_limit, "effective limit")?;
        let expires_at = request.now + request.reservation_ttl;
        let retention_expires_at = request.window.end() + request.reservation_ttl;
        let idempotency = request
            .idempotency_key
            .map_or("", crate::reservation::IdempotencyKey::as_str);

        cleanup_expired(&mut connection, request.now).await?;
        let values: (i64, i64, i64, i64, i64) = Script::new(RESERVE_SCRIPT)
            .key(&counter_key)
            .key(redis_reservation_prefix())
            .key(SEQUENCE_KEY)
            .key(&idempotency_key)
            .key(EXPIRATIONS_KEY)
            .arg(amount)
            .arg(effective_limit)
            .arg(expires_at.timestamp())
            .arg(idempotency)
            .arg(retention_expires_at.timestamp())
            .invoke_async(&mut connection)
            .await
            .map_err(redis_error)?;

        let usage = codec::usage_from_i64(values.2, values.3)?;
        if values.0 == 0 {
            return Ok(StoreReservationResult::denied(usage));
        }

        Ok(StoreReservationResult::allowed(
            StoreReservation::new(
                codec::id_from_i64(values.1)?,
                DateTime::<Utc>::from_timestamp(values.4, 0)
                    .ok_or_else(|| StoreError::Other("redis returned invalid expiry".to_owned()))?,
            ),
            usage,
        ))
    }

    async fn commit(&self, reservation_id: ReservationId) -> Result<(), StoreError> {
        finish(&self.pool, reservation_id, "committed").await
    }

    async fn refund(&self, reservation_id: ReservationId) -> Result<(), StoreError> {
        finish(&self.pool, reservation_id, "refunded").await
    }

    async fn usage(
        &self,
        key: &StoreKey,
        window: WindowBounds,
        now: DateTime<Utc>,
    ) -> Result<CounterUsage, StoreError> {
        let mut connection = self.pool.get().await.map_err(pool_error)?;
        cleanup_expired(&mut connection, now).await?;
        let values: (i64, i64) = Script::new(USAGE_SCRIPT)
            .key(redis_counter_key(key, window))
            .invoke_async(&mut connection)
            .await
            .map_err(redis_error)?;
        codec::usage_from_i64(values.0, values.1)
    }
}

async fn finish(
    pool: &Pool,
    reservation_id: ReservationId,
    status: &str,
) -> Result<(), StoreError> {
    let mut connection = pool.get().await.map_err(pool_error)?;
    let id = codec::to_i64(reservation_id.as_u64(), "reservation id")?;
    let result: i64 = Script::new(FINISH_SCRIPT)
        .key(redis_reservation_key(id))
        .key(EXPIRATIONS_KEY)
        .arg(status)
        .arg(id)
        .arg(Utc::now().timestamp())
        .invoke_async(&mut connection)
        .await
        .map_err(redis_error)?;

    match result {
        1 | 2 => Ok(()),
        0 => Err(StoreError::ReservationNotFound),
        -1 => Err(StoreError::ReservationExpired),
        _ => Err(StoreError::Other(
            "unexpected redis finish result".to_owned(),
        )),
    }
}

async fn cleanup_expired<C>(connection: &mut C, now: DateTime<Utc>) -> Result<(), StoreError>
where
    C: redis::aio::ConnectionLike,
{
    let _: i64 = Script::new(CLEANUP_SCRIPT)
        .key(EXPIRATIONS_KEY)
        .arg(now.timestamp())
        .invoke_async(connection)
        .await
        .map_err(redis_error)?;
    Ok(())
}

fn redis_counter_key(key: &StoreKey, window: WindowBounds) -> String {
    format!(
        "budget-warden:counter:{}:{}:{}",
        codec::key(key),
        codec::window_start(window).timestamp(),
        codec::window_end(window).timestamp()
    )
}

fn redis_idempotency_key(key: &StoreKey, window: WindowBounds) -> String {
    format!("{}:idempotency", redis_counter_key(key, window))
}

fn redis_reservation_prefix() -> &'static str {
    "budget-warden:reservation:"
}

fn redis_reservation_key(id: i64) -> String {
    format!("{}{id}", redis_reservation_prefix())
}

fn pool_error(error: deadpool_redis::PoolError) -> StoreError {
    let message = error.to_string();
    drop(error);
    StoreError::Other(message)
}

fn redis_error(error: redis::RedisError) -> StoreError {
    let message = error.to_string();
    drop(error);
    StoreError::Other(message)
}

const RESERVE_SCRIPT: &str = r"
local counter_key = KEYS[1]
local reservation_prefix = KEYS[2]
local sequence_key = KEYS[3]
local idempotency_key = KEYS[4]
local expirations_key = KEYS[5]
local amount = tonumber(ARGV[1])
local effective_limit = tonumber(ARGV[2])
local expires_at = tonumber(ARGV[3])
local idempotency = ARGV[4]
local retention_expires_at = tonumber(ARGV[5])

if idempotency ~= '' then
  local existing_id = redis.call('HGET', idempotency_key, idempotency)
  if existing_id then
    local reservation_key = reservation_prefix .. existing_id
    local status = redis.call('HGET', reservation_key, 'status')
    local committed = tonumber(redis.call('HGET', counter_key, 'committed') or '0')
    local reserved = tonumber(redis.call('HGET', counter_key, 'reserved') or '0')
    local existing_expiry = tonumber(redis.call('HGET', reservation_key, 'expires_at') or '0')
    if status == 'active' or status == 'committed' then
      return {1, tonumber(existing_id), committed, reserved, existing_expiry}
    end
    if status == false then
      redis.call('HDEL', idempotency_key, idempotency)
    end
  end
end

local committed = tonumber(redis.call('HGET', counter_key, 'committed') or '0')
local reserved = tonumber(redis.call('HGET', counter_key, 'reserved') or '0')
if committed + reserved + amount > effective_limit then
  return {0, 0, committed, reserved, 0}
end

local reservation_id = redis.call('INCR', sequence_key)
local reservation_key = reservation_prefix .. reservation_id
redis.call('HINCRBY', counter_key, 'reserved', amount)
reserved = reserved + amount
redis.call('HSET', reservation_key, 'counter_key', counter_key, 'amount', amount, 'status', 'active', 'expires_at', expires_at)
redis.call('ZADD', expirations_key, expires_at, reservation_id)
redis.call('EXPIREAT', counter_key, retention_expires_at)
redis.call('EXPIREAT', reservation_key, retention_expires_at)
if idempotency ~= '' then
  redis.call('HSET', idempotency_key, idempotency, reservation_id)
  redis.call('EXPIREAT', idempotency_key, retention_expires_at)
end
return {1, reservation_id, committed, reserved, expires_at}
";

const CLEANUP_SCRIPT: &str = r"
local expirations_key = KEYS[1]
local now = tonumber(ARGV[1])
local ids = redis.call('ZRANGEBYSCORE', expirations_key, '-inf', now)
for _, id in ipairs(ids) do
  local reservation_key = 'budget-warden:reservation:' .. id
  local status = redis.call('HGET', reservation_key, 'status')
  if status == 'active' then
    local counter_key = redis.call('HGET', reservation_key, 'counter_key')
    local amount = tonumber(redis.call('HGET', reservation_key, 'amount') or '0')
    local reserved = tonumber(redis.call('HGET', counter_key, 'reserved') or '0') - amount
    redis.call('HSET', counter_key, 'reserved', math.max(reserved, 0))
    redis.call('HSET', reservation_key, 'status', 'expired')
  end
  redis.call('ZREM', expirations_key, id)
end
return #ids
";

const FINISH_SCRIPT: &str = r"
local reservation_key = KEYS[1]
local expirations_key = KEYS[2]
local target_status = ARGV[1]
local reservation_id = ARGV[2]
local now = tonumber(ARGV[3])
if redis.call('EXISTS', reservation_key) == 0 then
  return 0
end
local status = redis.call('HGET', reservation_key, 'status')
if status == 'committed' or status == 'refunded' then
  return 2
end
if status == 'expired' then
  return -1
end
local counter_key = redis.call('HGET', reservation_key, 'counter_key')
local amount = tonumber(redis.call('HGET', reservation_key, 'amount') or '0')
local expires_at = tonumber(redis.call('HGET', reservation_key, 'expires_at') or '0')
if expires_at <= now then
  local reserved = tonumber(redis.call('HGET', counter_key, 'reserved') or '0') - amount
  redis.call('HSET', counter_key, 'reserved', math.max(reserved, 0))
  redis.call('HSET', reservation_key, 'status', 'expired')
  redis.call('ZREM', expirations_key, reservation_id)
  return -1
end
local reserved = tonumber(redis.call('HGET', counter_key, 'reserved') or '0') - amount
redis.call('HSET', counter_key, 'reserved', math.max(reserved, 0))
if target_status == 'committed' then
  redis.call('HINCRBY', counter_key, 'committed', amount)
end
redis.call('HSET', reservation_key, 'status', target_status)
redis.call('ZREM', expirations_key, reservation_id)
return 1
";

const USAGE_SCRIPT: &str = r"
local committed = tonumber(redis.call('HGET', KEYS[1], 'committed') or '0')
local reserved = tonumber(redis.call('HGET', KEYS[1], 'reserved') or '0')
return {committed, reserved}
";
