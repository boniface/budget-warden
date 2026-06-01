use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::error::BudgetError;
use crate::key::BudgetKey;
use crate::store::BudgetStore;
use crate::unit::BudgetUnit;

/// Type-safe reservation identifier.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReservationId(u64);

impl ReservationId {
    /// Creates a reservation identifier.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw identifier value.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// Type-safe idempotency key.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// Creates an idempotency key.
    ///
    /// # Errors
    ///
    /// Returns an error when the key is empty or whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, BudgetError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(BudgetError::InvalidRequest(
                "idempotency key must not be empty".to_owned(),
            ));
        }

        Ok(Self(value))
    }

    /// Returns the key as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Reserved budget that can be committed or refunded.
#[derive(Clone)]
pub struct BudgetReservation {
    id: ReservationId,
    policy_name: String,
    key: BudgetKey,
    amount: u64,
    unit: BudgetUnit,
    expires_at: DateTime<Utc>,
    store: Option<Arc<dyn BudgetStore>>,
}

impl fmt::Debug for BudgetReservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BudgetReservation")
            .field("id", &self.id)
            .field("policy_name", &self.policy_name)
            .field("key", &self.key)
            .field("amount", &self.amount)
            .field("unit", &self.unit)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl BudgetReservation {
    /// Creates a reservation attached to a store.
    #[must_use]
    pub fn new(
        id: ReservationId,
        policy_name: String,
        key: BudgetKey,
        amount: u64,
        unit: BudgetUnit,
        expires_at: DateTime<Utc>,
        store: Arc<dyn BudgetStore>,
    ) -> Self {
        Self {
            id,
            policy_name,
            key,
            amount,
            unit,
            expires_at,
            store: Some(store),
        }
    }

    /// Creates a decision-only reservation placeholder for non-mutating authorization.
    #[must_use]
    pub fn preview(
        policy_name: String,
        key: BudgetKey,
        amount: u64,
        unit: BudgetUnit,
        expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: ReservationId::new(0),
            policy_name,
            key,
            amount,
            unit,
            expires_at,
            store: None,
        }
    }

    /// Returns the reservation identifier.
    #[must_use]
    pub const fn id(&self) -> ReservationId {
        self.id
    }

    /// Returns the amount reserved.
    #[must_use]
    pub const fn amount(&self) -> u64 {
        self.amount
    }

    /// Returns the reservation expiry timestamp.
    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    /// Commits this reservation.
    ///
    /// # Errors
    ///
    /// Returns an error if this is a preview reservation or if the store rejects the commit.
    pub async fn commit(&self) -> Result<(), BudgetError> {
        let Some(store) = &self.store else {
            return Err(BudgetError::ReservationNotFound);
        };

        store.commit(self.id).await.map_err(BudgetError::from)?;
        crate::telemetry::reservation_committed(&self.policy_name, self.amount);
        Ok(())
    }

    /// Refunds this reservation.
    ///
    /// # Errors
    ///
    /// Returns an error if this is a preview reservation or if the store rejects the refund.
    pub async fn refund(&self) -> Result<(), BudgetError> {
        let Some(store) = &self.store else {
            return Err(BudgetError::ReservationNotFound);
        };

        store.refund(self.id).await.map_err(BudgetError::from)?;
        crate::telemetry::reservation_refunded(&self.policy_name, self.amount);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idempotency_key_rejects_blank_values() {
        let result = IdempotencyKey::new(" ");

        assert_eq!(
            result,
            Err(BudgetError::InvalidRequest(
                "idempotency key must not be empty".to_owned(),
            ))
        );
    }

    #[tokio::test]
    async fn preview_reservation_exposes_fields_and_rejects_mutation() {
        let key = BudgetKey::new("serpapi", "search", "google", "global").expect("valid key");
        let expires_at = Utc::now();
        let reservation = BudgetReservation::preview(
            "policy".to_owned(),
            key,
            7,
            BudgetUnit::Requests,
            expires_at,
        );

        assert_eq!(reservation.id(), ReservationId::new(0));
        assert_eq!(reservation.amount(), 7);
        assert_eq!(reservation.expires_at(), expires_at);
        assert_eq!(
            reservation.commit().await,
            Err(BudgetError::ReservationNotFound)
        );
        assert_eq!(
            reservation.refund().await,
            Err(BudgetError::ReservationNotFound)
        );
    }

    #[test]
    fn idempotency_key_accepts_non_empty_value() {
        let key = IdempotencyKey::new("request-1").expect("valid key");

        assert_eq!(key.as_str(), "request-1");
    }
}
