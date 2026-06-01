use crate::reservation::BudgetReservation;
use crate::usage::UsageSnapshot;

/// Action recommended when live access is denied.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallbackAction {
    /// Use a fresh cache entry.
    UseFreshCache,
    /// Use a stale cache entry.
    UseStaleCache,
    /// Queue the work for later.
    QueueForLater,
    /// Use a cheaper provider.
    UseCheaperProvider,
    /// Downgrade response quality.
    DowngradeQuality,
    /// Reject the request.
    Reject,
    /// Return a temporary unavailable response.
    ReturnUnavailable,
    /// Application-specific fallback.
    Custom(String),
}

/// Reason live budget access was denied.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenialReason {
    /// Hard provider budget would be exceeded.
    HardLimitReached,
    /// Request is ahead of the safe budget pace.
    AheadOfBudgetPace,
    /// Budget is reserved for higher-priority traffic.
    ReservedForHigherPriority,
    /// Minimum remaining budget would be violated.
    MinimumRemainingWouldBeViolated,
    /// No policy matched the request.
    NoMatchingPolicy,
    /// Store was unavailable.
    StoreUnavailable,
    /// Request was invalid.
    InvalidRequest,
}

/// Budget decision returned by the broker.
#[derive(Debug, Clone)]
pub enum BudgetDecision {
    /// Live access is allowed.
    AllowLive {
        /// Reservation for the allowed live call.
        reservation: BudgetReservation,
        /// Usage state at decision time.
        usage: UsageSnapshot,
    },
    /// Live access is denied.
    DenyLive {
        /// Denial reason.
        reason: DenialReason,
        /// Usage state at decision time.
        usage: UsageSnapshot,
        /// Recommended fallback.
        recommended_action: FallbackAction,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_action_is_comparable() {
        assert_eq!(FallbackAction::UseStaleCache, FallbackAction::UseStaleCache);
    }
}
