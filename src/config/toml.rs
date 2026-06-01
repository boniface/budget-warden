use crate::broker::{BudgetWarden, BudgetWardenBuilder};
use crate::config::model::PolicyConfig;
use crate::error::BudgetError;
use crate::policy::BudgetPolicy;

/// Parses policies from TOML.
///
/// # Errors
///
/// Returns an error when TOML is invalid or policy conversion fails.
pub fn policies_from_toml_str(input: &str) -> Result<Vec<BudgetPolicy>, BudgetError> {
    let config: PolicyConfig =
        toml::from_str(input).map_err(|error| BudgetError::ConfigError(error.to_string()))?;
    config.try_into()
}

/// Builds a warden from TOML using the supplied builder.
///
/// # Errors
///
/// Returns an error when TOML parsing or builder validation fails.
pub fn warden_from_toml_str(
    input: &str,
    builder: BudgetWardenBuilder,
) -> Result<BudgetWarden, BudgetError> {
    policies_from_toml_str(input)?
        .into_iter()
        .fold(builder, BudgetWardenBuilder::policy)
        .build()
}

#[cfg(all(feature = "std", feature = "memory"))]
impl BudgetWarden {
    /// Builds a memory-backed warden from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read, TOML is invalid, or policy validation fails.
    pub fn from_toml_file(path: impl AsRef<std::path::Path>) -> Result<Self, BudgetError> {
        let input = std::fs::read_to_string(path)
            .map_err(|error| BudgetError::ConfigError(error.to_string()))?;
        warden_from_toml_str(
            &input,
            Self::builder().store(crate::store::MemoryStore::new()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
[[policies]]
name = "serpapi-monthly-free-plan"
provider = "serpapi"
domain = "search"
resource = "google-search"
subject = "global"
unit = "requests"
hard_limit = 250
exhausted_action = "use_stale_cache"
fail_mode = "closed"

[policies.window]
type = "calendar_month"
timezone = "America/Toronto"

[policies.strategy]
type = "preserve_for_window"
max_spend_ahead_percent = 10
emergency_reserve_percent = 20
minimum_remaining_units = 10
"#;

    #[test]
    fn parses_toml_config() {
        let policies = policies_from_toml_str(CONFIG).expect("config parses");

        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].name(), "serpapi-monthly-free-plan");
    }

    #[test]
    fn builds_memory_warden_from_toml() {
        let warden = warden_from_toml_str(
            CONFIG,
            BudgetWarden::builder().store(crate::store::MemoryStore::new()),
        );

        assert!(warden.is_ok());
    }

    #[test]
    fn rejects_invalid_toml() {
        let result = policies_from_toml_str("not toml");

        assert!(matches!(result, Err(BudgetError::ConfigError(_))));
    }

    #[test]
    fn rejects_missing_required_toml_fields() {
        let result = policies_from_toml_str(
            r#"
[[policies]]
name = "missing-required-fields"
"#,
        );

        assert!(matches!(result, Err(BudgetError::ConfigError(_))));
    }
}
