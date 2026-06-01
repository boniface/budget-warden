use chrono::{DateTime, Utc};

use crate::usage::UsageSnapshot;

/// Usage report across configured budgets.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetReport {
    generated_at: DateTime<Utc>,
    budgets: Vec<UsageSnapshot>,
}

impl BudgetReport {
    /// Creates a budget report.
    #[must_use]
    pub const fn new(generated_at: DateTime<Utc>, budgets: Vec<UsageSnapshot>) -> Self {
        Self {
            generated_at,
            budgets,
        }
    }

    /// Returns report generation timestamp.
    #[must_use]
    pub const fn generated_at(&self) -> DateTime<Utc> {
        self.generated_at
    }

    /// Returns budget snapshots.
    #[must_use]
    pub fn budgets(&self) -> &[UsageSnapshot] {
        &self.budgets
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn report_exposes_budget_slice() {
        let now = Utc::now();
        let report = BudgetReport::new(now, Vec::new());

        assert_eq!(report.generated_at(), now);
        assert!(report.budgets().is_empty());
    }
}
