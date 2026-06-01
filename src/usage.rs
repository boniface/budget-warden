use chrono::{DateTime, Utc};
use num_traits::ToPrimitive;

use crate::key::BudgetKey;
use crate::unit::BudgetUnit;

/// Query for a usage snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageQuery {
    key: BudgetKey,
}

impl UsageQuery {
    /// Creates a usage query.
    #[must_use]
    pub const fn new(key: BudgetKey) -> Self {
        Self { key }
    }

    /// Returns the queried budget key.
    #[must_use]
    pub const fn key(&self) -> &BudgetKey {
        &self.key
    }
}

/// Dashboard-friendly budget usage snapshot.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct UsageSnapshot {
    policy_name: String,
    key: BudgetKey,
    unit: BudgetUnit,
    used: u64,
    hard_limit: u64,
    remaining_hard: u64,
    safe_to_spend_now: u64,
    reserved_remaining: u64,
    percent_used: f64,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
    resets_at: DateTime<Utc>,
    ahead_of_pace: bool,
}

/// Input data for creating a [`UsageSnapshot`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSnapshotInput {
    /// Policy name.
    pub policy_name: String,
    /// Budget key represented by the snapshot.
    pub key: BudgetKey,
    /// Budget unit.
    pub unit: BudgetUnit,
    /// Used budget units.
    pub used: u64,
    /// Hard provider limit.
    pub hard_limit: u64,
    /// Units safe to spend at this instant.
    pub safe_to_spend_now: u64,
    /// Remaining protected reserve.
    pub reserved_remaining: u64,
    /// Window start.
    pub window_start: DateTime<Utc>,
    /// Window end.
    pub window_end: DateTime<Utc>,
    /// Whether usage is ahead of the pacing policy.
    pub ahead_of_pace: bool,
}

impl UsageSnapshot {
    /// Creates a usage snapshot.
    #[must_use]
    pub fn new(input: UsageSnapshotInput) -> Self {
        let remaining_hard = input.hard_limit.saturating_sub(input.used);
        let percent_used = if input.hard_limit == 0 {
            0.0
        } else {
            let used = input.used.to_f64().unwrap_or(f64::INFINITY);
            let hard_limit = input.hard_limit.to_f64().unwrap_or(f64::INFINITY);
            (used / hard_limit) * 100.0
        };

        Self {
            policy_name: input.policy_name,
            key: input.key,
            unit: input.unit,
            used: input.used,
            hard_limit: input.hard_limit,
            remaining_hard,
            safe_to_spend_now: input.safe_to_spend_now,
            reserved_remaining: input.reserved_remaining,
            percent_used,
            window_start: input.window_start,
            window_end: input.window_end,
            resets_at: input.window_end,
            ahead_of_pace: input.ahead_of_pace,
        }
    }

    /// Returns used budget units.
    #[must_use]
    pub const fn used(&self) -> u64 {
        self.used
    }

    /// Returns hard remaining budget.
    #[must_use]
    pub const fn remaining_hard(&self) -> u64 {
        self.remaining_hard
    }

    /// Returns budget safe to spend now.
    #[must_use]
    pub const fn safe_to_spend_now(&self) -> u64 {
        self.safe_to_spend_now
    }

    /// Returns whether usage is ahead of pace.
    #[must_use]
    pub const fn ahead_of_pace(&self) -> bool {
        self.ahead_of_pace
    }

    /// Returns the policy name.
    #[must_use]
    pub fn policy_name(&self) -> &str {
        &self.policy_name
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn snapshot_calculates_remaining_and_percent() {
        let now = Utc
            .with_ymd_and_hms(2026, 6, 1, 0, 0, 0)
            .single()
            .expect("valid timestamp");
        let key =
            BudgetKey::new("serpapi", "search", "google", "global").expect("valid key fixture");

        let snapshot = UsageSnapshot::new(UsageSnapshotInput {
            policy_name: "policy".to_owned(),
            key,
            unit: BudgetUnit::Requests,
            used: 25,
            hard_limit: 100,
            safe_to_spend_now: 10,
            reserved_remaining: 20,
            window_start: now,
            window_end: now + chrono::Duration::days(1),
            ahead_of_pace: false,
        });

        assert_eq!(snapshot.remaining_hard(), 75);
        assert!((snapshot.percent_used - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn snapshot_handles_zero_hard_limit() {
        let now = Utc
            .with_ymd_and_hms(2026, 6, 1, 0, 0, 0)
            .single()
            .expect("valid timestamp");
        let key =
            BudgetKey::new("serpapi", "search", "google", "global").expect("valid key fixture");
        let query = UsageQuery::new(key.clone());
        let snapshot = UsageSnapshot::new(UsageSnapshotInput {
            policy_name: "policy".to_owned(),
            key,
            unit: BudgetUnit::Requests,
            used: 0,
            hard_limit: 0,
            safe_to_spend_now: 0,
            reserved_remaining: 0,
            window_start: now,
            window_end: now + chrono::Duration::days(1),
            ahead_of_pace: true,
        });

        assert_eq!(query.key().provider(), "serpapi");
        assert_eq!(snapshot.used(), 0);
        assert_eq!(snapshot.safe_to_spend_now(), 0);
        assert!(snapshot.ahead_of_pace());
        assert_eq!(snapshot.policy_name(), "policy");
    }
}
