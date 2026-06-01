use std::collections::BTreeMap;

use crate::error::BudgetError;
use crate::key::BudgetKey;
use crate::priority::Priority;
use crate::reservation::IdempotencyKey;
use crate::unit::BudgetUnit;

/// Request for budget authorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetRequest {
    key: BudgetKey,
    unit: BudgetUnit,
    amount: u64,
    priority: Priority,
    idempotency_key: Option<IdempotencyKey>,
    metadata: BTreeMap<String, String>,
}

impl BudgetRequest {
    /// Starts building a request.
    pub fn builder(
        provider: impl Into<String>,
        domain: impl Into<String>,
        resource: impl Into<String>,
    ) -> BudgetRequestBuilder {
        BudgetRequestBuilder::new(provider, domain, resource)
    }

    /// Returns the budget key.
    #[must_use]
    pub const fn key(&self) -> &BudgetKey {
        &self.key
    }

    /// Returns the budget unit.
    #[must_use]
    pub const fn unit(&self) -> &BudgetUnit {
        &self.unit
    }

    /// Returns the requested amount.
    #[must_use]
    pub const fn amount(&self) -> u64 {
        self.amount
    }

    /// Returns the request priority.
    #[must_use]
    pub const fn priority(&self) -> Priority {
        self.priority
    }

    /// Returns the idempotency key when provided.
    #[must_use]
    pub const fn idempotency_key(&self) -> Option<&IdempotencyKey> {
        self.idempotency_key.as_ref()
    }
}

/// Builder for [`BudgetRequest`].
#[derive(Debug, Clone)]
#[must_use]
pub struct BudgetRequestBuilder {
    provider: String,
    domain: String,
    resource: String,
    subject: String,
    unit: BudgetUnit,
    amount: u64,
    priority: Priority,
    idempotency_key: Option<IdempotencyKey>,
    metadata: BTreeMap<String, String>,
}

impl BudgetRequestBuilder {
    fn new(
        provider: impl Into<String>,
        domain: impl Into<String>,
        resource: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            domain: domain.into(),
            resource: resource.into(),
            subject: "global".to_owned(),
            unit: BudgetUnit::default(),
            amount: 1,
            priority: Priority::default(),
            idempotency_key: None,
            metadata: BTreeMap::new(),
        }
    }

    /// Sets the budget subject.
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = subject.into();
        self
    }

    /// Sets the budget unit.
    pub fn unit(mut self, unit: BudgetUnit) -> Self {
        self.unit = unit;
        self
    }

    /// Sets the requested amount.
    pub const fn amount(mut self, amount: u64) -> Self {
        self.amount = amount;
        self
    }

    /// Sets the request priority.
    pub const fn priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Sets the idempotency key.
    pub fn idempotency_key(mut self, key: IdempotencyKey) -> Self {
        self.idempotency_key = Some(key);
        self
    }

    /// Adds metadata.
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Builds a validated request.
    ///
    /// # Errors
    ///
    /// Returns an error when the key is invalid or the amount is zero.
    pub fn build(self) -> Result<BudgetRequest, BudgetError> {
        if self.amount == 0 {
            return Err(BudgetError::InvalidRequest(
                "amount must be greater than zero".to_owned(),
            ));
        }

        Ok(BudgetRequest {
            key: BudgetKey::new(self.provider, self.domain, self.resource, self.subject)?,
            unit: self.unit,
            amount: self.amount,
            priority: self.priority,
            idempotency_key: self.idempotency_key,
            metadata: self.metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_builder_sets_defaults() {
        let request = BudgetRequest::builder("serpapi", "search", "google")
            .build()
            .expect("valid request fixture");

        assert_eq!(request.key().subject(), "global");
        assert_eq!(request.amount(), 1);
        assert_eq!(request.priority(), Priority::Normal);
    }

    #[test]
    fn request_builder_rejects_zero_amount() {
        let request = BudgetRequest::builder("serpapi", "search", "google")
            .amount(0)
            .build();

        assert!(matches!(request, Err(BudgetError::InvalidRequest(_))));
    }
}
