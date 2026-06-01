use crate::decision::DenialReason;
use crate::error::StoreError;
use crate::policy::BudgetPolicy;
use crate::request::BudgetRequest;
use crate::usage::UsageSnapshot;

/// Emits an allow event when tracing is enabled.
pub fn allowed(request: &BudgetRequest, usage: &UsageSnapshot) {
    #[cfg(feature = "tracing")]
    tracing::info!(
        provider = %request.key().provider(),
        domain = %request.key().domain(),
        resource = %request.key().resource(),
        subject = %request.key().subject(),
        used = usage.used(),
        remaining = usage.remaining_hard(),
        safe_to_spend_now = usage.safe_to_spend_now(),
        "budget decision allowed"
    );

    #[cfg(not(feature = "tracing"))]
    let _ = (request, usage);
}

/// Emits a denial event when tracing is enabled.
pub fn denied(request: &BudgetRequest, reason: DenialReason, usage: &UsageSnapshot) {
    #[cfg(feature = "tracing")]
    tracing::info!(
        provider = %request.key().provider(),
        domain = %request.key().domain(),
        resource = %request.key().resource(),
        subject = %request.key().subject(),
        reason = ?reason,
        used = usage.used(),
        remaining = usage.remaining_hard(),
        safe_to_spend_now = usage.safe_to_spend_now(),
        "budget decision denied"
    );

    #[cfg(not(feature = "tracing"))]
    let _ = (request, reason, usage);
}

/// Emits a reservation-created event when tracing is enabled.
pub fn reservation_created(request: &BudgetRequest, usage: &UsageSnapshot) {
    #[cfg(feature = "tracing")]
    tracing::info!(
        provider = %request.key().provider(),
        domain = %request.key().domain(),
        resource = %request.key().resource(),
        subject = %request.key().subject(),
        amount = request.amount(),
        used = usage.used(),
        "budget reservation created"
    );

    #[cfg(not(feature = "tracing"))]
    let _ = (request, usage);
}

/// Emits a reservation-committed event when tracing is enabled.
pub fn reservation_committed(policy_name: &str, amount: u64) {
    #[cfg(feature = "tracing")]
    tracing::info!(policy_name, amount, "budget reservation committed");

    #[cfg(not(feature = "tracing"))]
    let _ = (policy_name, amount);
}

/// Emits a reservation-refunded event when tracing is enabled.
pub fn reservation_refunded(policy_name: &str, amount: u64) {
    #[cfg(feature = "tracing")]
    tracing::info!(policy_name, amount, "budget reservation refunded");

    #[cfg(not(feature = "tracing"))]
    let _ = (policy_name, amount);
}

/// Emits a store-unavailable event when tracing is enabled.
pub fn store_unavailable(policy: &BudgetPolicy, request: &BudgetRequest, error: &StoreError) {
    #[cfg(feature = "tracing")]
    tracing::warn!(
        policy_name = policy.name(),
        fail_mode = ?policy.fail_mode(),
        provider = %request.key().provider(),
        domain = %request.key().domain(),
        resource = %request.key().resource(),
        subject = %request.key().subject(),
        error = %error,
        "budget store unavailable"
    );

    #[cfg(not(feature = "tracing"))]
    let _ = (policy, request, error);
}
