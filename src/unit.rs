/// Unit consumed from a scarce external budget.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum BudgetUnit {
    /// Request-count budget.
    #[default]
    Requests,
    /// Token-count budget.
    Tokens,
    /// Provider credit budget.
    Credits,
    /// Currency budget represented as cents.
    Cents,
    /// Byte budget.
    Bytes,
    /// Message budget.
    Messages,
    /// Domain-specific budget unit.
    Custom(String),
}

impl BudgetUnit {
    /// Returns the stable lowercase representation used by stores and config.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Requests => "requests",
            Self::Tokens => "tokens",
            Self::Credits => "credits",
            Self::Cents => "cents",
            Self::Bytes => "bytes",
            Self::Messages => "messages",
            Self::Custom(value) => value.as_str(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_unit_is_requests() {
        assert_eq!(BudgetUnit::default(), BudgetUnit::Requests);
    }

    #[test]
    fn unit_returns_stable_names() {
        assert_eq!(BudgetUnit::Requests.as_str(), "requests");
        assert_eq!(BudgetUnit::Tokens.as_str(), "tokens");
        assert_eq!(BudgetUnit::Credits.as_str(), "credits");
        assert_eq!(BudgetUnit::Cents.as_str(), "cents");
        assert_eq!(BudgetUnit::Bytes.as_str(), "bytes");
        assert_eq!(BudgetUnit::Messages.as_str(), "messages");
        assert_eq!(BudgetUnit::Custom("widgets".to_owned()).as_str(), "widgets");
    }
}
