use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::contract::{
    BudgetStore, CounterUsage, ReserveRequest, StoreKey, StoreReservation, StoreReservationResult,
};
use crate::error::StoreError;
use crate::reservation::{IdempotencyKey, ReservationId};
use crate::window::WindowBounds;

/// Single-process in-memory store for tests and local development.
#[derive(Debug, Clone, Default)]
pub struct MemoryStore {
    state: Arc<RwLock<MemoryState>>,
}

impl MemoryStore {
    /// Creates an empty memory store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl BudgetStore for MemoryStore {
    async fn reserve_if_allowed(
        &self,
        request: ReserveRequest<'_>,
    ) -> Result<StoreReservationResult, StoreError> {
        request.validate()?;

        let mut state = self.write_state()?;
        state.cleanup_expired(request.now);

        let counter_key = CounterKey::new(request.key, request.window);
        if let Some(idempotency_key) = request.idempotency_key
            && let Some(existing_id) = state
                .idempotency
                .get(&(counter_key.clone(), idempotency_key.clone()))
                .copied()
        {
            return Ok(state.replay_existing_reservation(existing_id, &counter_key));
        }

        let usage = state.usage_for_counter(&counter_key);
        if usage.used().saturating_add(request.amount) > request.effective_limit {
            return Ok(StoreReservationResult::denied(usage));
        }

        let reservation_id = state.next_reservation_id()?;
        let expires_at = request.now + request.reservation_ttl;
        let usage = {
            let counter = state.counters.entry(counter_key.clone()).or_default();
            counter.reserved = counter.reserved.saturating_add(request.amount);
            CounterUsage::new(counter.committed, counter.reserved)
        };
        state.reservations.insert(
            reservation_id,
            ReservationRecord::active(counter_key.clone(), request.amount, expires_at),
        );
        if let Some(idempotency_key) = request.idempotency_key {
            state
                .idempotency
                .insert((counter_key, idempotency_key.clone()), reservation_id);
        }

        Ok(StoreReservationResult::allowed(
            StoreReservation::new(reservation_id, expires_at),
            usage,
        ))
    }

    async fn commit(&self, reservation_id: ReservationId) -> Result<(), StoreError> {
        let mut state = self.write_state()?;
        state.cleanup_expired(Utc::now());

        let Some(record) = state.reservations.get(&reservation_id) else {
            return Err(StoreError::ReservationNotFound);
        };

        match record.status {
            ReservationStatus::Committed | ReservationStatus::Refunded => Ok(()),
            ReservationStatus::Expired => Err(StoreError::ReservationExpired),
            ReservationStatus::Active => {
                let counter_key = record.counter_key.clone();
                let amount = record.amount;
                let Some(counter) = state.counters.get_mut(&counter_key) else {
                    return Err(StoreError::Other(
                        "reservation counter is missing".to_owned(),
                    ));
                };
                counter.reserved = counter.reserved.saturating_sub(amount);
                counter.committed = counter.committed.saturating_add(amount);
                if let Some(record) = state.reservations.get_mut(&reservation_id) {
                    record.status = ReservationStatus::Committed;
                }
                Ok(())
            }
        }
    }

    async fn refund(&self, reservation_id: ReservationId) -> Result<(), StoreError> {
        let mut state = self.write_state()?;
        state.cleanup_expired(Utc::now());

        let Some(record) = state.reservations.get(&reservation_id) else {
            return Err(StoreError::ReservationNotFound);
        };

        match record.status {
            ReservationStatus::Committed | ReservationStatus::Refunded => Ok(()),
            ReservationStatus::Expired => Err(StoreError::ReservationExpired),
            ReservationStatus::Active => {
                let counter_key = record.counter_key.clone();
                let amount = record.amount;
                let Some(counter) = state.counters.get_mut(&counter_key) else {
                    return Err(StoreError::Other(
                        "reservation counter is missing".to_owned(),
                    ));
                };
                counter.reserved = counter.reserved.saturating_sub(amount);
                if let Some(record) = state.reservations.get_mut(&reservation_id) {
                    record.status = ReservationStatus::Refunded;
                }
                Ok(())
            }
        }
    }

    async fn usage(
        &self,
        key: &StoreKey,
        window: WindowBounds,
        now: DateTime<Utc>,
    ) -> Result<CounterUsage, StoreError> {
        let mut state = self.write_state()?;
        state.cleanup_expired(now);
        Ok(state.usage_for_counter(&CounterKey::new(key, window)))
    }
}

impl MemoryStore {
    fn write_state(&self) -> Result<std::sync::RwLockWriteGuard<'_, MemoryState>, StoreError> {
        self.state
            .write()
            .map_err(|_| StoreError::Other("memory store lock is poisoned".to_owned()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CounterKey {
    store_key: StoreKey,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
}

impl CounterKey {
    fn new(store_key: &StoreKey, window: WindowBounds) -> Self {
        Self {
            store_key: store_key.clone(),
            window_start: window.start(),
            window_end: window.end(),
        }
    }
}

#[derive(Debug, Default)]
struct MemoryState {
    counters: HashMap<CounterKey, CounterRecord>,
    reservations: HashMap<ReservationId, ReservationRecord>,
    idempotency: HashMap<(CounterKey, IdempotencyKey), ReservationId>,
    next_id: u64,
}

impl MemoryState {
    fn next_reservation_id(&mut self) -> Result<ReservationId, StoreError> {
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| StoreError::Other("reservation id overflow".to_owned()))?;
        Ok(ReservationId::new(self.next_id))
    }

    fn usage_for_counter(&self, key: &CounterKey) -> CounterUsage {
        self.counters
            .get(key)
            .map_or_else(CounterUsage::default, |counter| {
                CounterUsage::new(counter.committed, counter.reserved)
            })
    }

    fn replay_existing_reservation(
        &self,
        reservation_id: ReservationId,
        counter_key: &CounterKey,
    ) -> StoreReservationResult {
        let usage = self.usage_for_counter(counter_key);
        let Some(record) = self.reservations.get(&reservation_id) else {
            return StoreReservationResult::denied(usage);
        };

        match record.status {
            ReservationStatus::Active | ReservationStatus::Committed => {
                StoreReservationResult::allowed(
                    StoreReservation::new(reservation_id, record.expires_at),
                    usage,
                )
            }
            ReservationStatus::Refunded | ReservationStatus::Expired => {
                StoreReservationResult::denied(usage)
            }
        }
    }

    fn cleanup_expired(&mut self, now: DateTime<Utc>) {
        let expired_ids: Vec<ReservationId> = self
            .reservations
            .iter()
            .filter_map(|(id, record)| {
                (record.status == ReservationStatus::Active && record.expires_at <= now)
                    .then_some(*id)
            })
            .collect();

        for id in expired_ids {
            if let Some(record) = self.reservations.get_mut(&id) {
                if let Some(counter) = self.counters.get_mut(&record.counter_key) {
                    counter.reserved = counter.reserved.saturating_sub(record.amount);
                }
                record.status = ReservationStatus::Expired;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct CounterRecord {
    committed: u64,
    reserved: u64,
}

#[derive(Debug, Clone)]
struct ReservationRecord {
    counter_key: CounterKey,
    amount: u64,
    expires_at: DateTime<Utc>,
    status: ReservationStatus,
}

impl ReservationRecord {
    fn active(counter_key: CounterKey, amount: u64, expires_at: DateTime<Utc>) -> Self {
        Self {
            counter_key,
            amount,
            expires_at,
            status: ReservationStatus::Active,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReservationStatus {
    Active,
    Committed,
    Refunded,
    Expired,
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use tokio::task::JoinSet;

    use super::*;
    use crate::unit::BudgetUnit;

    fn store_key() -> StoreKey {
        let key =
            crate::key::BudgetKey::new("serpapi", "search", "google", "global").expect("valid key");
        StoreKey::new("policy".to_owned(), key, BudgetUnit::Requests)
    }

    fn window() -> WindowBounds {
        let start = Utc
            .with_ymd_and_hms(2026, 6, 1, 0, 0, 0)
            .single()
            .expect("valid start");
        WindowBounds::new(start, start + Duration::days(1)).expect("valid window")
    }

    #[tokio::test]
    async fn reserve_commit_and_refund_are_tracked() {
        let store = MemoryStore::new();
        let key = store_key();
        let now = Utc::now();
        let result = store
            .reserve_if_allowed(ReserveRequest {
                key: &key,
                amount: 5,
                hard_limit: 10,
                effective_limit: 10,
                reservation_ttl: Duration::minutes(5),
                window: window(),
                idempotency_key: None,
                now,
            })
            .await
            .expect("reserve succeeds");
        let reservation = result.reservation().expect("allowed reservation");

        store
            .commit(reservation.id())
            .await
            .expect("commit succeeds");
        store
            .refund(reservation.id())
            .await
            .expect("refund is idempotent");
        let usage = store
            .usage(&key, window(), now)
            .await
            .expect("usage succeeds");

        assert_eq!(usage.committed(), 5);
        assert_eq!(usage.reserved(), 0);
    }

    #[tokio::test]
    async fn concurrent_reservations_do_not_exceed_limit() {
        let store = MemoryStore::new();
        let key = store_key();
        let now = window().start();
        let mut tasks = JoinSet::new();

        (0..100_u8).for_each(|_| {
            let store = store.clone();
            let key = key.clone();
            tasks.spawn(async move {
                store
                    .reserve_if_allowed(ReserveRequest {
                        key: &key,
                        amount: 1,
                        hard_limit: 10,
                        effective_limit: 10,
                        reservation_ttl: Duration::minutes(5),
                        window: window(),
                        idempotency_key: None,
                        now,
                    })
                    .await
                    .expect("reserve call succeeds")
                    .is_allowed()
            });
        });

        let mut allowed = 0_u64;
        while let Some(result) = tasks.join_next().await {
            if result.expect("task joins") {
                allowed += 1;
            }
        }

        assert_eq!(allowed, 10);
    }

    #[tokio::test]
    async fn expired_reservation_is_removed_from_usage() {
        let store = MemoryStore::new();
        let key = store_key();
        let now = window().start();
        let result = store
            .reserve_if_allowed(ReserveRequest {
                key: &key,
                amount: 5,
                hard_limit: 10,
                effective_limit: 10,
                reservation_ttl: Duration::seconds(1),
                window: window(),
                idempotency_key: None,
                now,
            })
            .await
            .expect("reserve succeeds");

        assert!(result.is_allowed());
        let later = now + Duration::seconds(2);
        let usage = store
            .usage(&key, window(), later)
            .await
            .expect("usage succeeds");

        assert_eq!(usage.used(), 0);
    }

    #[tokio::test]
    async fn expired_reservation_cannot_be_committed_or_refunded() {
        let store = MemoryStore::new();
        let key = store_key();
        let now = Utc::now() - Duration::seconds(2);
        let result = store
            .reserve_if_allowed(ReserveRequest {
                key: &key,
                amount: 5,
                hard_limit: 10,
                effective_limit: 10,
                reservation_ttl: Duration::seconds(1),
                window: window(),
                idempotency_key: None,
                now,
            })
            .await
            .expect("reserve succeeds");
        let reservation = result.reservation().expect("allowed reservation");

        let commit = store.commit(reservation.id()).await;
        let refund = store.refund(reservation.id()).await;

        assert_eq!(commit, Err(StoreError::ReservationExpired));
        assert_eq!(refund, Err(StoreError::ReservationExpired));
    }

    #[tokio::test]
    async fn idempotency_key_replays_existing_reservation() {
        let store = MemoryStore::new();
        let key = store_key();
        let idempotency_key =
            crate::reservation::IdempotencyKey::new("request-1").expect("valid idempotency key");
        let now = Utc::now();
        let request = ReserveRequest {
            key: &key,
            amount: 5,
            hard_limit: 10,
            effective_limit: 10,
            reservation_ttl: Duration::minutes(5),
            window: window(),
            idempotency_key: Some(&idempotency_key),
            now,
        };

        let first = store
            .reserve_if_allowed(request)
            .await
            .expect("first reserve succeeds");
        let second = store
            .reserve_if_allowed(request)
            .await
            .expect("second reserve succeeds");

        assert_eq!(first.reservation(), second.reservation());
        assert_eq!(second.usage().reserved(), 5);
    }
}
