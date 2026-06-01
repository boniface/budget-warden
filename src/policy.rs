use crate::decision::FallbackAction;
use crate::error::BudgetError;
use crate::key::BudgetKeyPattern;
use crate::priority::Priority;
use crate::unit::BudgetUnit;
use crate::window::BudgetWindow;

/// Behavior when the budget store fails.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FailMode {
    /// Allow live access when the store is unavailable.
    Open,
    /// Deny live access when the store is unavailable.
    #[default]
    Closed,
}

/// Preserve budget across the whole window.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreserveForWindow {
    max_spend_ahead_percent: u8,
    emergency_reserve_percent: u8,
    minimum_remaining_units: Option<u64>,
}

impl PreserveForWindow {
    /// Creates a preserve-for-window strategy.
    #[must_use]
    pub const fn new(
        max_spend_ahead_percent: u8,
        emergency_reserve_percent: u8,
        minimum_remaining_units: Option<u64>,
    ) -> Self {
        Self {
            max_spend_ahead_percent,
            emergency_reserve_percent,
            minimum_remaining_units,
        }
    }

    /// Returns maximum spend-ahead percentage.
    #[must_use]
    pub const fn max_spend_ahead_percent(self) -> u8 {
        self.max_spend_ahead_percent
    }

    /// Returns emergency reserve percentage.
    #[must_use]
    pub const fn emergency_reserve_percent(self) -> u8 {
        self.emergency_reserve_percent
    }

    /// Returns minimum remaining units.
    #[must_use]
    pub const fn minimum_remaining_units(self) -> Option<u64> {
        self.minimum_remaining_units
    }
}

/// Fixed reserved capacity for a minimum priority.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReserveCapacity {
    reserved_units: u64,
    reserved_for: Priority,
}

impl ReserveCapacity {
    /// Creates reserved-capacity strategy data.
    #[must_use]
    pub const fn new(reserved_units: u64, reserved_for: Priority) -> Self {
        Self {
            reserved_units,
            reserved_for,
        }
    }

    /// Returns the reserved unit count.
    #[must_use]
    pub const fn reserved_units(self) -> u64 {
        self.reserved_units
    }

    /// Returns the minimum priority allowed to spend reserved units.
    #[must_use]
    pub const fn reserved_for(self) -> Priority {
        self.reserved_for
    }
}

/// Budget strategy.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetStrategy {
    /// Strict quota only.
    HardLimitOnly,
    /// Preserve the budget across the full provider window.
    PreserveForWindow(PreserveForWindow),
    /// Keep fixed capacity for a priority.
    ReserveCapacity(ReserveCapacity),
}

/// A budget policy.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetPolicy {
    name: String,
    key: BudgetKeyPattern,
    unit: BudgetUnit,
    hard_limit: u64,
    window: BudgetWindow,
    strategy: BudgetStrategy,
    exhausted_action: FallbackAction,
    low_budget_action: Option<FallbackAction>,
    fail_mode: FailMode,
}

impl BudgetPolicy {
    /// Starts building a policy.
    pub fn builder(name: impl Into<String>) -> BudgetPolicyBuilder {
        BudgetPolicyBuilder::new(name)
    }

    /// Returns the policy name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the key pattern.
    #[must_use]
    pub const fn key(&self) -> &BudgetKeyPattern {
        &self.key
    }

    /// Returns the budget unit.
    #[must_use]
    pub const fn unit(&self) -> &BudgetUnit {
        &self.unit
    }

    /// Returns the hard limit.
    #[must_use]
    pub const fn hard_limit(&self) -> u64 {
        self.hard_limit
    }

    /// Returns the budget window.
    #[must_use]
    pub const fn window(&self) -> &BudgetWindow {
        &self.window
    }

    /// Returns the strategy.
    #[must_use]
    pub const fn strategy(&self) -> BudgetStrategy {
        self.strategy
    }

    /// Returns the exhausted fallback action.
    #[must_use]
    pub const fn exhausted_action(&self) -> &FallbackAction {
        &self.exhausted_action
    }

    /// Returns the low-budget fallback action.
    #[must_use]
    pub const fn low_budget_action(&self) -> Option<&FallbackAction> {
        self.low_budget_action.as_ref()
    }

    /// Returns the store fail mode.
    #[must_use]
    pub const fn fail_mode(&self) -> FailMode {
        self.fail_mode
    }
}

/// Builder for [`BudgetPolicy`].
#[derive(Debug, Clone)]
#[must_use]
pub struct BudgetPolicyBuilder {
    name: String,
    provider: Option<String>,
    domain: Option<String>,
    resource: Option<String>,
    subject: Option<String>,
    unit: BudgetUnit,
    hard_limit: Option<u64>,
    window: Option<BudgetWindow>,
    strategy: BudgetStrategy,
    exhausted_action: FallbackAction,
    low_budget_action: Option<FallbackAction>,
    fail_mode: FailMode,
}

impl BudgetPolicyBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            provider: None,
            domain: None,
            resource: None,
            subject: None,
            unit: BudgetUnit::default(),
            hard_limit: None,
            window: None,
            strategy: BudgetStrategy::HardLimitOnly,
            exhausted_action: FallbackAction::Reject,
            low_budget_action: None,
            fail_mode: FailMode::default(),
        }
    }

    /// Sets provider.
    pub fn provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Sets domain.
    pub fn domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    /// Sets resource.
    pub fn resource(mut self, resource: impl Into<String>) -> Self {
        self.resource = Some(resource.into());
        self
    }

    /// Sets subject.
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// Sets unit.
    pub fn unit(mut self, unit: BudgetUnit) -> Self {
        self.unit = unit;
        self
    }

    /// Sets hard limit.
    pub const fn hard_limit(mut self, hard_limit: u64) -> Self {
        self.hard_limit = Some(hard_limit);
        self
    }

    /// Sets calendar-day window.
    pub fn calendar_day(mut self, timezone: impl Into<String>) -> Self {
        self.window = Some(BudgetWindow::calendar_day(timezone));
        self
    }

    /// Sets calendar-month window.
    pub fn calendar_month(mut self, timezone: impl Into<String>) -> Self {
        self.window = Some(BudgetWindow::calendar_month(timezone));
        self
    }

    /// Sets strategy.
    pub const fn strategy(mut self, strategy: BudgetStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Sets exhausted fallback action.
    pub fn exhausted_action(mut self, action: FallbackAction) -> Self {
        self.exhausted_action = action;
        self
    }

    /// Sets low budget fallback action.
    pub fn low_budget_action(mut self, action: FallbackAction) -> Self {
        self.low_budget_action = Some(action);
        self
    }

    /// Sets fail mode.
    pub const fn fail_mode(mut self, fail_mode: FailMode) -> Self {
        self.fail_mode = fail_mode;
        self
    }

    /// Builds a validated policy.
    ///
    /// # Errors
    ///
    /// Returns an error when required fields are missing or invalid.
    pub fn build(self) -> Result<BudgetPolicy, BudgetError> {
        if self.name.trim().is_empty() {
            return Err(BudgetError::InvalidRequest(
                "policy name must not be empty".to_owned(),
            ));
        }

        let hard_limit = self.hard_limit.ok_or_else(|| {
            BudgetError::InvalidRequest("hard limit must be configured".to_owned())
        })?;
        if hard_limit == 0 {
            return Err(BudgetError::InvalidRequest(
                "hard limit must be greater than zero".to_owned(),
            ));
        }

        let provider = self
            .provider
            .ok_or_else(|| BudgetError::InvalidRequest("provider must be configured".to_owned()))?;
        let mut key = BudgetKeyPattern::new(provider)?;
        if let Some(domain) = self.domain {
            key = key.with_domain(domain);
        }
        if let Some(resource) = self.resource {
            key = key.with_resource(resource);
        }
        if let Some(subject) = self.subject {
            key = key.with_subject(subject);
        }

        let window = self
            .window
            .ok_or_else(|| BudgetError::InvalidRequest("window must be configured".to_owned()))?;
        window.validate()?;
        validate_strategy(self.strategy, hard_limit)?;

        Ok(BudgetPolicy {
            name: self.name,
            key,
            unit: self.unit,
            hard_limit,
            window,
            strategy: self.strategy,
            exhausted_action: self.exhausted_action,
            low_budget_action: self.low_budget_action,
            fail_mode: self.fail_mode,
        })
    }
}

fn validate_strategy(strategy: BudgetStrategy, hard_limit: u64) -> Result<(), BudgetError> {
    match strategy {
        BudgetStrategy::HardLimitOnly => Ok(()),
        BudgetStrategy::PreserveForWindow(settings) => {
            if settings.max_spend_ahead_percent() > 100 {
                return Err(BudgetError::InvalidRequest(
                    "max spend ahead percent must be at most 100".to_owned(),
                ));
            }
            if settings.emergency_reserve_percent() > 100 {
                return Err(BudgetError::InvalidRequest(
                    "emergency reserve percent must be at most 100".to_owned(),
                ));
            }
            if settings
                .minimum_remaining_units()
                .is_some_and(|minimum| minimum > hard_limit)
            {
                return Err(BudgetError::InvalidRequest(
                    "minimum remaining units must not exceed hard limit".to_owned(),
                ));
            }
            Ok(())
        }
        BudgetStrategy::ReserveCapacity(settings) => {
            if settings.reserved_units() == 0 {
                return Err(BudgetError::InvalidRequest(
                    "reserved units must be greater than zero".to_owned(),
                ));
            }
            if settings.reserved_units() > hard_limit {
                return Err(BudgetError::InvalidRequest(
                    "reserved units must not exceed hard limit".to_owned(),
                ));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_builder_constructs_policy() {
        let policy = BudgetPolicy::builder("serpapi")
            .provider("serpapi")
            .domain("search")
            .resource("google")
            .subject("global")
            .hard_limit(250)
            .calendar_month("UTC")
            .build()
            .expect("valid policy");

        assert_eq!(policy.name(), "serpapi");
        assert_eq!(policy.hard_limit(), 250);
    }

    #[test]
    fn policy_builder_rejects_missing_provider() {
        let policy = BudgetPolicy::builder("serpapi")
            .hard_limit(250)
            .calendar_month("UTC")
            .build();

        assert!(matches!(policy, Err(BudgetError::InvalidRequest(_))));
    }

    #[test]
    fn policy_builder_rejects_invalid_required_values() {
        let empty_name = BudgetPolicy::builder(" ")
            .provider("serpapi")
            .hard_limit(1)
            .calendar_month("UTC")
            .build();
        let zero_limit = BudgetPolicy::builder("zero")
            .provider("serpapi")
            .hard_limit(0)
            .calendar_month("UTC")
            .build();
        let missing_limit = BudgetPolicy::builder("missing")
            .provider("serpapi")
            .calendar_month("UTC")
            .build();
        let missing_window = BudgetPolicy::builder("missing")
            .provider("serpapi")
            .hard_limit(1)
            .build();

        assert!(matches!(empty_name, Err(BudgetError::InvalidRequest(_))));
        assert!(matches!(zero_limit, Err(BudgetError::InvalidRequest(_))));
        assert!(matches!(missing_limit, Err(BudgetError::InvalidRequest(_))));
        assert!(matches!(
            missing_window,
            Err(BudgetError::InvalidRequest(_))
        ));
    }

    #[test]
    fn policy_getters_return_configured_values() {
        let preserve = PreserveForWindow::new(10, 20, Some(5));
        let reserve = ReserveCapacity::new(3, Priority::Important);
        let policy = BudgetPolicy::builder("serpapi")
            .provider("serpapi")
            .hard_limit(100)
            .calendar_day("UTC")
            .strategy(BudgetStrategy::ReserveCapacity(reserve))
            .low_budget_action(FallbackAction::UseFreshCache)
            .fail_mode(FailMode::Open)
            .build()
            .expect("valid policy");

        assert_eq!(preserve.max_spend_ahead_percent(), 10);
        assert_eq!(preserve.emergency_reserve_percent(), 20);
        assert_eq!(preserve.minimum_remaining_units(), Some(5));
        assert_eq!(reserve.reserved_units(), 3);
        assert_eq!(reserve.reserved_for(), Priority::Important);
        assert_eq!(
            policy.low_budget_action(),
            Some(&FallbackAction::UseFreshCache)
        );
        assert_eq!(policy.fail_mode(), FailMode::Open);
    }
}
