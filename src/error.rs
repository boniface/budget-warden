use thiserror::Error;

/// Errors produced by the budget policy engine.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum BudgetError {
    /// No policy matched a request.
    #[error("no matching budget policy found")]
    NoMatchingPolicy,

    /// A request failed validation.
    #[error("budget request is invalid: {0}")]
    InvalidRequest(String),

    /// The backing store is unavailable.
    #[error("budget store unavailable")]
    StoreUnavailable,

    /// The backing store failed.
    #[error("budget store error: {0}")]
    StoreError(String),

    /// A reservation could not be found.
    #[error("reservation not found")]
    ReservationNotFound,

    /// A reservation has expired.
    #[error("reservation expired")]
    ReservationExpired,

    /// Configuration is invalid.
    #[error("configuration error: {0}")]
    ConfigError(String),

    /// A window could not be calculated.
    #[error("time/window error: {0}")]
    WindowError(String),
}

/// Errors produced by a budget store implementation.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The store is unavailable.
    #[error("store unavailable")]
    Unavailable,

    /// The reservation was not found.
    #[error("reservation not found")]
    ReservationNotFound,

    /// The reservation expired.
    #[error("reservation expired")]
    ReservationExpired,

    /// The store rejected invalid input.
    #[error("invalid store input: {0}")]
    InvalidInput(String),

    /// An implementation-specific store failure occurred.
    #[error("store failure: {0}")]
    Other(String),
}

impl From<StoreError> for BudgetError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unavailable => Self::StoreUnavailable,
            StoreError::ReservationNotFound => Self::ReservationNotFound,
            StoreError::ReservationExpired => Self::ReservationExpired,
            StoreError::InvalidInput(message) | StoreError::Other(message) => {
                Self::StoreError(message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_error_maps_to_budget_error() {
        let error = BudgetError::from(StoreError::ReservationExpired);

        assert_eq!(error, BudgetError::ReservationExpired);
    }
}
