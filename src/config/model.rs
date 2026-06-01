use crate::decision::FallbackAction;
use crate::error::BudgetError;
use crate::policy::{BudgetPolicy, BudgetStrategy, FailMode, PreserveForWindow, ReserveCapacity};
use crate::priority::Priority;
use crate::unit::BudgetUnit;

/// Top-level policy configuration.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct PolicyConfig {
    /// Configured policies.
    pub policies: Vec<ConfigPolicy>,
}

/// Serializable policy configuration.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct ConfigPolicy {
    /// Policy name.
    pub name: String,
    /// Provider component.
    pub provider: String,
    /// Optional domain component.
    pub domain: Option<String>,
    /// Optional resource component.
    pub resource: Option<String>,
    /// Optional subject component.
    pub subject: Option<String>,
    /// Budget unit.
    pub unit: String,
    /// Hard provider budget limit.
    pub hard_limit: u64,
    /// Window kind.
    pub window: WindowConfig,
    /// Recommended fallback once exhausted.
    pub exhausted_action: String,
    /// Optional fallback before exhaustion.
    pub low_budget_action: Option<String>,
    /// Store failure behavior.
    pub fail_mode: Option<String>,
    /// Budget strategy.
    pub strategy: StrategyConfig,
}

/// Supported config window shapes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WindowConfig {
    /// Calendar day in a timezone.
    CalendarDay { timezone: String },
    /// Calendar month in a timezone.
    CalendarMonth { timezone: String },
}

/// Supported config strategy shapes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StrategyConfig {
    /// Strict hard limit.
    HardLimitOnly,
    /// Preserve budget across the full window.
    PreserveForWindow {
        /// Allowed spend-ahead percentage.
        max_spend_ahead_percent: u8,
        /// Emergency reserve percentage.
        emergency_reserve_percent: u8,
        /// Minimum remaining units.
        minimum_remaining_units: Option<u64>,
    },
    /// Fixed reserve for a priority.
    ReserveCapacity {
        /// Reserved units.
        reserved_units: u64,
        /// Minimum priority allowed to spend reserve.
        reserved_for: String,
    },
}

impl TryFrom<PolicyConfig> for Vec<BudgetPolicy> {
    type Error = BudgetError;

    fn try_from(config: PolicyConfig) -> Result<Self, Self::Error> {
        config.policies.into_iter().map(TryInto::try_into).collect()
    }
}

impl TryFrom<ConfigPolicy> for BudgetPolicy {
    type Error = BudgetError;

    fn try_from(config: ConfigPolicy) -> Result<Self, Self::Error> {
        let mut builder = BudgetPolicy::builder(config.name)
            .provider(config.provider)
            .unit(parse_unit(&config.unit)?)
            .hard_limit(config.hard_limit)
            .strategy(parse_strategy(config.strategy)?)
            .exhausted_action(parse_action(&config.exhausted_action)?)
            .fail_mode(parse_fail_mode(config.fail_mode.as_deref())?);

        if let Some(domain) = config.domain {
            builder = builder.domain(domain);
        }
        if let Some(resource) = config.resource {
            builder = builder.resource(resource);
        }
        if let Some(subject) = config.subject {
            builder = builder.subject(subject);
        }
        if let Some(action) = config.low_budget_action {
            builder = builder.low_budget_action(parse_action(&action)?);
        }

        builder = match config.window {
            WindowConfig::CalendarDay { timezone } => builder.calendar_day(timezone),
            WindowConfig::CalendarMonth { timezone } => builder.calendar_month(timezone),
        };

        builder.build()
    }
}

fn parse_strategy(config: StrategyConfig) -> Result<BudgetStrategy, BudgetError> {
    Ok(match config {
        StrategyConfig::HardLimitOnly => BudgetStrategy::HardLimitOnly,
        StrategyConfig::PreserveForWindow {
            max_spend_ahead_percent,
            emergency_reserve_percent,
            minimum_remaining_units,
        } => BudgetStrategy::PreserveForWindow(PreserveForWindow::new(
            max_spend_ahead_percent,
            emergency_reserve_percent,
            minimum_remaining_units,
        )),
        StrategyConfig::ReserveCapacity {
            reserved_units,
            reserved_for,
        } => BudgetStrategy::ReserveCapacity(ReserveCapacity::new(
            reserved_units,
            parse_priority(&reserved_for)?,
        )),
    })
}

fn parse_unit(value: &str) -> Result<BudgetUnit, BudgetError> {
    Ok(match normalized(value).as_str() {
        "requests" => BudgetUnit::Requests,
        "tokens" => BudgetUnit::Tokens,
        "credits" => BudgetUnit::Credits,
        "cents" => BudgetUnit::Cents,
        "bytes" => BudgetUnit::Bytes,
        "messages" => BudgetUnit::Messages,
        custom if custom.starts_with("custom:") => {
            BudgetUnit::Custom(custom.trim_start_matches("custom:").to_owned())
        }
        _ => {
            return Err(BudgetError::ConfigError(format!(
                "unknown budget unit `{value}`"
            )));
        }
    })
}

fn parse_priority(value: &str) -> Result<Priority, BudgetError> {
    Ok(match normalized(value).as_str() {
        "background" => Priority::Background,
        "normal" => Priority::Normal,
        "important" => Priority::Important,
        "critical" => Priority::Critical,
        _ => {
            return Err(BudgetError::ConfigError(format!(
                "unknown priority `{value}`"
            )));
        }
    })
}

fn parse_action(value: &str) -> Result<FallbackAction, BudgetError> {
    Ok(match normalized(value).as_str() {
        "use_fresh_cache" => FallbackAction::UseFreshCache,
        "use_stale_cache" => FallbackAction::UseStaleCache,
        "queue_for_later" => FallbackAction::QueueForLater,
        "use_cheaper_provider" => FallbackAction::UseCheaperProvider,
        "downgrade_quality" => FallbackAction::DowngradeQuality,
        "reject" => FallbackAction::Reject,
        "return_unavailable" => FallbackAction::ReturnUnavailable,
        custom if custom.starts_with("custom:") => {
            FallbackAction::Custom(custom.trim_start_matches("custom:").to_owned())
        }
        _ => {
            return Err(BudgetError::ConfigError(format!(
                "unknown fallback action `{value}`"
            )));
        }
    })
}

fn parse_fail_mode(value: Option<&str>) -> Result<FailMode, BudgetError> {
    Ok(match value.map(normalized).as_deref() {
        None | Some("closed") => FailMode::Closed,
        Some("open") => FailMode::Open,
        Some(value) => {
            return Err(BudgetError::ConfigError(format!(
                "unknown fail mode `{value}`"
            )));
        }
    })
}

fn normalized(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_policy_converts_to_budget_policy() {
        let policy = ConfigPolicy {
            name: "serpapi".to_owned(),
            provider: "serpapi".to_owned(),
            domain: Some("search".to_owned()),
            resource: Some("google".to_owned()),
            subject: Some("global".to_owned()),
            unit: "requests".to_owned(),
            hard_limit: 250,
            window: WindowConfig::CalendarMonth {
                timezone: "UTC".to_owned(),
            },
            exhausted_action: "use_stale_cache".to_owned(),
            low_budget_action: Some("queue_for_later".to_owned()),
            fail_mode: Some("closed".to_owned()),
            strategy: StrategyConfig::PreserveForWindow {
                max_spend_ahead_percent: 10,
                emergency_reserve_percent: 20,
                minimum_remaining_units: Some(10),
            },
        };

        let policy = BudgetPolicy::try_from(policy).expect("config converts");

        assert_eq!(policy.name(), "serpapi");
        assert_eq!(policy.hard_limit(), 250);
    }

    #[test]
    fn invalid_config_values_are_rejected() {
        assert!(matches!(
            parse_unit("unknown"),
            Err(BudgetError::ConfigError(_))
        ));
        assert!(matches!(
            parse_action("unknown"),
            Err(BudgetError::ConfigError(_))
        ));
        assert!(matches!(
            parse_priority("unknown"),
            Err(BudgetError::ConfigError(_))
        ));
        assert!(matches!(
            parse_fail_mode(Some("bad")),
            Err(BudgetError::ConfigError(_))
        ));
    }

    #[test]
    fn config_policy_rejects_invalid_timezone_and_strategy_values() {
        let mut invalid_timezone = ConfigPolicy {
            name: "serpapi".to_owned(),
            provider: "serpapi".to_owned(),
            domain: None,
            resource: None,
            subject: None,
            unit: "requests".to_owned(),
            hard_limit: 250,
            window: WindowConfig::CalendarMonth {
                timezone: "bad-zone".to_owned(),
            },
            exhausted_action: "reject".to_owned(),
            low_budget_action: None,
            fail_mode: None,
            strategy: StrategyConfig::HardLimitOnly,
        };

        assert!(matches!(
            BudgetPolicy::try_from(invalid_timezone.clone()),
            Err(BudgetError::WindowError(_))
        ));

        invalid_timezone.window = WindowConfig::CalendarMonth {
            timezone: "UTC".to_owned(),
        };
        invalid_timezone.strategy = StrategyConfig::ReserveCapacity {
            reserved_units: 251,
            reserved_for: "critical".to_owned(),
        };

        assert!(matches!(
            BudgetPolicy::try_from(invalid_timezone),
            Err(BudgetError::InvalidRequest(_))
        ));
    }
}
