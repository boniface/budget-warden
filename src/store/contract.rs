use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};

use crate::error::StoreError;
use crate::key::BudgetKey;
use crate::reservation::{IdempotencyKey, ReservationId};
use crate::unit::BudgetUnit;
use crate::window::WindowBounds;

/// Store key used for budget enforcement.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StoreKey {
    policy_name: String,
    budget_key: BudgetKey,
    unit: BudgetUnit,
}

impl StoreKey {
    /// Creates a store key.
    #[must_use]
    pub const fn new(policy_name: String, budget_key: BudgetKey, unit: BudgetUnit) -> Self {
        Self {
            policy_name,
            budget_key,
            unit,
        }
    }

    /// Returns policy name.
    #[must_use]
    pub fn policy_name(&self) -> &str {
        &self.policy_name
    }

    /// Returns budget key.
    #[must_use]
    pub const fn budget_key(&self) -> &BudgetKey {
        &self.budget_key
    }

    /// Returns unit.
    #[must_use]
    pub const fn unit(&self) -> &BudgetUnit {
        &self.unit
    }

    /// Returns a stable compact key for external stores.
    #[must_use]
    pub fn compact(&self) -> String {
        let budget_key = self.budget_key();
        format!(
            "{}:{}:{}:{}:{}:{}",
            self.policy_name(),
            budget_key.provider(),
            budget_key.domain(),
            budget_key.resource(),
            budget_key.subject(),
            self.unit().as_str()
        )
    }
}

/// Store-level reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreReservation {
    id: ReservationId,
    expires_at: DateTime<Utc>,
}

impl StoreReservation {
    /// Creates a store reservation.
    #[must_use]
    pub const fn new(id: ReservationId, expires_at: DateTime<Utc>) -> Self {
        Self { id, expires_at }
    }

    /// Returns reservation id.
    #[must_use]
    pub const fn id(self) -> ReservationId {
        self.id
    }

    /// Returns expiry.
    #[must_use]
    pub const fn expires_at(self) -> DateTime<Utc> {
        self.expires_at
    }
}

/// Store usage counters for a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CounterUsage {
    committed: u64,
    reserved: u64,
}

impl CounterUsage {
    /// Creates counter usage.
    #[must_use]
    pub const fn new(committed: u64, reserved: u64) -> Self {
        Self {
            committed,
            reserved,
        }
    }

    /// Returns committed usage.
    #[must_use]
    pub const fn committed(self) -> u64 {
        self.committed
    }

    /// Returns active reserved usage.
    #[must_use]
    pub const fn reserved(self) -> u64 {
        self.reserved
    }

    /// Returns committed plus active reserved usage.
    #[must_use]
    pub const fn used(self) -> u64 {
        self.committed.saturating_add(self.reserved)
    }
}

/// Result from atomic reserve-if-allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreReservationResult {
    allowed: bool,
    reservation: Option<StoreReservation>,
    usage: CounterUsage,
}

impl StoreReservationResult {
    /// Creates an allowed result.
    #[must_use]
    pub const fn allowed(reservation: StoreReservation, usage: CounterUsage) -> Self {
        Self {
            allowed: true,
            reservation: Some(reservation),
            usage,
        }
    }

    /// Creates a denied result.
    #[must_use]
    pub const fn denied(usage: CounterUsage) -> Self {
        Self {
            allowed: false,
            reservation: None,
            usage,
        }
    }

    /// Returns whether the reservation was allowed.
    #[must_use]
    pub const fn is_allowed(self) -> bool {
        self.allowed
    }

    /// Returns the store reservation.
    #[must_use]
    pub const fn reservation(self) -> Option<StoreReservation> {
        self.reservation
    }

    /// Returns usage after the operation.
    #[must_use]
    pub const fn usage(self) -> CounterUsage {
        self.usage
    }
}

/// Stateful backend for budget counters and reservations.
#[async_trait]
pub trait BudgetStore: Send + Sync {
    /// Atomically reserves budget only if the effective limit allows it.
    async fn reserve_if_allowed(
        &self,
        request: ReserveRequest<'_>,
    ) -> Result<StoreReservationResult, StoreError>;

    /// Commits a reservation.
    async fn commit(&self, reservation_id: ReservationId) -> Result<(), StoreError>;

    /// Refunds a reservation.
    async fn refund(&self, reservation_id: ReservationId) -> Result<(), StoreError>;

    /// Returns current usage for a key and window.
    async fn usage(
        &self,
        key: &StoreKey,
        window: WindowBounds,
        now: DateTime<Utc>,
    ) -> Result<CounterUsage, StoreError>;
}

/// Borrowed input for atomic reservation.
#[derive(Debug, Clone, Copy)]
pub struct ReserveRequest<'a> {
    /// Store key.
    pub key: &'a StoreKey,
    /// Requested amount.
    pub amount: u64,
    /// Provider hard limit.
    pub hard_limit: u64,
    /// Effective policy limit for the instant.
    pub effective_limit: u64,
    /// Reservation time-to-live.
    pub reservation_ttl: Duration,
    /// Active window.
    pub window: WindowBounds,
    /// Optional idempotency key.
    pub idempotency_key: Option<&'a IdempotencyKey>,
    /// Current timestamp.
    pub now: DateTime<Utc>,
}

impl ReserveRequest<'_> {
    /// Validates store-independent reservation invariants.
    ///
    /// # Errors
    ///
    /// Returns an error when the request would corrupt store accounting.
    pub fn validate(self) -> Result<(), StoreError> {
        if self.amount == 0 {
            return Err(StoreError::InvalidInput(
                "amount must be greater than zero".to_owned(),
            ));
        }
        if self.effective_limit > self.hard_limit {
            return Err(StoreError::InvalidInput(
                "effective limit must not exceed hard limit".to_owned(),
            ));
        }
        if self.reservation_ttl <= Duration::zero() {
            return Err(StoreError::InvalidInput(
                "reservation ttl must be positive".to_owned(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};

    use super::*;
    use crate::key::BudgetKey;
    use crate::unit::BudgetUnit;
    use crate::window::WindowBounds;

    #[test]
    fn store_model_accessors_return_values() {
        let key = BudgetKey::new("serpapi", "search", "google", "global").expect("valid key");
        let store_key = StoreKey::new("policy".to_owned(), key, BudgetUnit::Requests);
        let start = Utc
            .with_ymd_and_hms(2026, 6, 1, 0, 0, 0)
            .single()
            .expect("valid timestamp");
        let window = WindowBounds::new(start, start + Duration::days(1)).expect("valid window");
        let reservation = StoreReservation::new(ReservationId::new(3), window.end());
        let usage = CounterUsage::new(2, 4);
        let allowed = StoreReservationResult::allowed(reservation, usage);
        let denied = StoreReservationResult::denied(usage);

        assert_eq!(store_key.policy_name(), "policy");
        assert_eq!(store_key.unit(), &BudgetUnit::Requests);
        assert_eq!(reservation.id(), ReservationId::new(3));
        assert_eq!(reservation.expires_at(), window.end());
        assert_eq!(usage.used(), 6);
        assert!(allowed.is_allowed());
        assert_eq!(allowed.reservation(), Some(reservation));
        assert!(!denied.is_allowed());
    }
}
