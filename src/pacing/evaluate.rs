use chrono::{DateTime, Utc};

use crate::decision::DenialReason;
use crate::policy::{BudgetPolicy, BudgetStrategy};
use crate::priority::Priority;
use crate::window::WindowBounds;

/// Result of policy pacing evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacingEvaluation {
    effective_limit: u64,
    safe_to_spend_now: u64,
    reserved_remaining: u64,
    ahead_of_pace: bool,
    denial_reason: Option<DenialReason>,
}

impl PacingEvaluation {
    /// Creates an evaluation.
    #[must_use]
    pub const fn new(
        effective_limit: u64,
        safe_to_spend_now: u64,
        reserved_remaining: u64,
        ahead_of_pace: bool,
        denial_reason: Option<DenialReason>,
    ) -> Self {
        Self {
            effective_limit,
            safe_to_spend_now,
            reserved_remaining,
            ahead_of_pace,
            denial_reason,
        }
    }

    /// Returns the limit to pass to the backing store.
    #[must_use]
    pub const fn effective_limit(self) -> u64 {
        self.effective_limit
    }

    /// Returns units safe to spend now.
    #[must_use]
    pub const fn safe_to_spend_now(self) -> u64 {
        self.safe_to_spend_now
    }

    /// Returns reserved remaining units.
    #[must_use]
    pub const fn reserved_remaining(self) -> u64 {
        self.reserved_remaining
    }

    /// Returns whether usage is ahead of pace.
    #[must_use]
    pub const fn ahead_of_pace(self) -> bool {
        self.ahead_of_pace
    }

    /// Returns denial reason, if the policy pre-denies the request.
    #[must_use]
    pub const fn denial_reason(self) -> Option<DenialReason> {
        self.denial_reason
    }
}

/// Evaluates a policy for the current usage.
#[must_use]
pub fn evaluate_policy(
    policy: &BudgetPolicy,
    used: u64,
    amount: u64,
    priority: Priority,
    window: WindowBounds,
    now: DateTime<Utc>,
) -> PacingEvaluation {
    if used.saturating_add(amount) > policy.hard_limit() {
        return PacingEvaluation::new(
            policy.hard_limit(),
            0,
            0,
            false,
            Some(DenialReason::HardLimitReached),
        );
    }

    match policy.strategy() {
        BudgetStrategy::HardLimitOnly => hard_limit_only(policy.hard_limit(), used),
        BudgetStrategy::PreserveForWindow(strategy) => {
            let reserve = percent_of(policy.hard_limit(), strategy.emergency_reserve_percent());
            let normal_usable_limit = policy.hard_limit().saturating_sub(reserve);
            if priority >= Priority::Critical {
                return PacingEvaluation::new(
                    policy.hard_limit(),
                    policy.hard_limit().saturating_sub(used),
                    reserve.min(policy.hard_limit().saturating_sub(used)),
                    false,
                    None,
                );
            }

            if priority < Priority::Critical && used.saturating_add(amount) > normal_usable_limit {
                return PacingEvaluation::new(
                    normal_usable_limit,
                    normal_usable_limit.saturating_sub(used),
                    policy.hard_limit().saturating_sub(used),
                    false,
                    Some(DenialReason::ReservedForHigherPriority),
                );
            }

            let paced_limit = paced_limit(policy, window, now, normal_usable_limit, strategy);
            if used.saturating_add(amount) > paced_limit {
                return PacingEvaluation::new(
                    paced_limit,
                    paced_limit.saturating_sub(used),
                    reserve.saturating_sub(policy.hard_limit().saturating_sub(used)),
                    true,
                    Some(DenialReason::AheadOfBudgetPace),
                );
            }

            if let Some(minimum) = strategy.minimum_remaining_units() {
                let remaining_after = policy.hard_limit().saturating_sub(used + amount);
                if remaining_after < minimum && priority < Priority::Critical {
                    return PacingEvaluation::new(
                        paced_limit,
                        paced_limit.saturating_sub(used),
                        reserve,
                        false,
                        Some(DenialReason::MinimumRemainingWouldBeViolated),
                    );
                }
            }

            PacingEvaluation::new(
                paced_limit,
                paced_limit.saturating_sub(used),
                reserve.min(policy.hard_limit().saturating_sub(used)),
                used > paced_limit,
                None,
            )
        }
        BudgetStrategy::ReserveCapacity(strategy) => {
            let normal_usable_limit = policy
                .hard_limit()
                .saturating_sub(strategy.reserved_units());
            if priority < strategy.reserved_for()
                && used.saturating_add(amount) > normal_usable_limit
            {
                return PacingEvaluation::new(
                    normal_usable_limit,
                    normal_usable_limit.saturating_sub(used),
                    strategy.reserved_units(),
                    false,
                    Some(DenialReason::ReservedForHigherPriority),
                );
            }

            PacingEvaluation::new(
                policy.hard_limit(),
                policy.hard_limit().saturating_sub(used),
                strategy
                    .reserved_units()
                    .min(policy.hard_limit().saturating_sub(used)),
                false,
                None,
            )
        }
    }
}

fn hard_limit_only(hard_limit: u64, used: u64) -> PacingEvaluation {
    PacingEvaluation::new(hard_limit, hard_limit.saturating_sub(used), 0, false, None)
}

fn paced_limit(
    policy: &BudgetPolicy,
    window: WindowBounds,
    now: DateTime<Utc>,
    normal_usable_limit: u64,
    strategy: crate::policy::PreserveForWindow,
) -> u64 {
    let duration_ms = non_negative_millis(window.duration().num_milliseconds()).max(1);
    let elapsed_ms = non_negative_millis((now - window.start()).num_milliseconds());
    let ideal_spend = saturating_u64(u128::from(policy.hard_limit()) * elapsed_ms / duration_ms);
    let ahead_allowance = percent_of(policy.hard_limit(), strategy.max_spend_ahead_percent());

    normal_usable_limit.min(ideal_spend.saturating_add(ahead_allowance))
}

fn percent_of(value: u64, percent: u8) -> u64 {
    saturating_u64(u128::from(value) * u128::from(percent) / 100)
}

fn non_negative_millis(milliseconds: i64) -> u128 {
    u128::try_from(milliseconds.max(0)).unwrap_or(0)
}

fn saturating_u64(value: u128) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;
    use crate::decision::FallbackAction;
    use crate::policy::PreserveForWindow;
    use crate::unit::BudgetUnit;
    use crate::window::BudgetWindow;

    fn policy() -> BudgetPolicy {
        BudgetPolicy::builder("monthly")
            .provider("serpapi")
            .domain("search")
            .resource("google")
            .subject("global")
            .unit(BudgetUnit::Requests)
            .hard_limit(250)
            .calendar_month("UTC")
            .strategy(BudgetStrategy::PreserveForWindow(PreserveForWindow::new(
                10,
                20,
                Some(10),
            )))
            .exhausted_action(FallbackAction::UseStaleCache)
            .build()
            .expect("valid policy")
    }

    #[test]
    fn preserve_for_window_denies_ahead_of_pace() {
        let policy = policy();
        let now = Utc
            .with_ymd_and_hms(2026, 6, 6, 0, 0, 0)
            .single()
            .expect("valid timestamp");
        let window = BudgetWindow::calendar_month("UTC")
            .bounds_at(now)
            .expect("valid bounds");

        let evaluation = evaluate_policy(&policy, 80, 1, Priority::Normal, window, now);

        assert_eq!(
            evaluation.denial_reason(),
            Some(DenialReason::AheadOfBudgetPace)
        );
    }

    #[test]
    fn critical_priority_can_use_reserved_capacity() {
        let policy = policy();
        let now = Utc
            .with_ymd_and_hms(2026, 6, 30, 23, 0, 0)
            .single()
            .expect("valid timestamp");
        let window = WindowBounds::new(now - Duration::days(29), now + Duration::hours(1))
            .expect("valid window");

        let evaluation = evaluate_policy(&policy, 220, 1, Priority::Critical, window, now);

        assert_eq!(evaluation.denial_reason(), None);
    }

    #[test]
    fn hard_limit_denies_when_limit_would_be_exceeded() {
        let policy = BudgetPolicy::builder("hard")
            .provider("serpapi")
            .hard_limit(2)
            .calendar_day("UTC")
            .build()
            .expect("valid policy");
        let now = Utc
            .with_ymd_and_hms(2026, 6, 1, 12, 0, 0)
            .single()
            .expect("valid timestamp");
        let window = BudgetWindow::calendar_day("UTC")
            .bounds_at(now)
            .expect("valid bounds");

        let evaluation = evaluate_policy(&policy, 2, 1, Priority::Normal, window, now);

        assert_eq!(evaluation.effective_limit(), 2);
        assert_eq!(
            evaluation.denial_reason(),
            Some(DenialReason::HardLimitReached)
        );
    }

    #[test]
    fn reserve_capacity_denies_lower_priority() {
        let policy = BudgetPolicy::builder("reserve")
            .provider("serpapi")
            .hard_limit(10)
            .calendar_day("UTC")
            .strategy(BudgetStrategy::ReserveCapacity(
                crate::policy::ReserveCapacity::new(2, Priority::Critical),
            ))
            .build()
            .expect("valid policy");
        let now = Utc
            .with_ymd_and_hms(2026, 6, 1, 12, 0, 0)
            .single()
            .expect("valid timestamp");
        let window = BudgetWindow::calendar_day("UTC")
            .bounds_at(now)
            .expect("valid bounds");

        let evaluation = evaluate_policy(&policy, 8, 1, Priority::Normal, window, now);

        assert_eq!(
            evaluation.denial_reason(),
            Some(DenialReason::ReservedForHigherPriority)
        );
    }

    #[test]
    fn minimum_remaining_denies_normal_priority() {
        let policy = policy();
        let now = Utc
            .with_ymd_and_hms(2026, 6, 30, 23, 0, 0)
            .single()
            .expect("valid timestamp");
        let window = WindowBounds::new(now - Duration::days(29), now + Duration::hours(1))
            .expect("valid window");

        let evaluation = evaluate_policy(&policy, 239, 2, Priority::Normal, window, now);

        assert_eq!(
            evaluation.denial_reason(),
            Some(DenialReason::ReservedForHigherPriority)
        );
    }
}
