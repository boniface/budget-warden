use crate::error::BudgetError;

/// Fully-qualified budget key.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BudgetKey {
    provider: String,
    domain: String,
    resource: String,
    subject: String,
}

impl BudgetKey {
    /// Creates a validated budget key.
    ///
    /// # Errors
    ///
    /// Returns an error when any component is empty.
    pub fn new(
        provider: impl Into<String>,
        domain: impl Into<String>,
        resource: impl Into<String>,
        subject: impl Into<String>,
    ) -> Result<Self, BudgetError> {
        let key = Self {
            provider: provider.into(),
            domain: domain.into(),
            resource: resource.into(),
            subject: subject.into(),
        };
        key.validate()?;
        Ok(key)
    }

    /// Returns the provider component.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Returns the domain component.
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// Returns the resource component.
    #[must_use]
    pub fn resource(&self) -> &str {
        &self.resource
    }

    /// Returns the subject component.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    fn validate(&self) -> Result<(), BudgetError> {
        validate_component("provider", &self.provider)?;
        validate_component("domain", &self.domain)?;
        validate_component("resource", &self.resource)?;
        validate_component("subject", &self.subject)
    }
}

/// Policy key pattern. `None` fields match any value.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BudgetKeyPattern {
    provider: String,
    domain: Option<String>,
    resource: Option<String>,
    subject: Option<String>,
}

impl BudgetKeyPattern {
    /// Creates a provider-scoped key pattern.
    ///
    /// # Errors
    ///
    /// Returns an error when the provider is empty.
    pub fn new(provider: impl Into<String>) -> Result<Self, BudgetError> {
        let provider = provider.into();
        validate_component("provider", &provider)?;
        Ok(Self {
            provider,
            domain: None,
            resource: None,
            subject: None,
        })
    }

    /// Adds a domain condition.
    #[must_use]
    pub fn with_domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    /// Adds a resource condition.
    #[must_use]
    pub fn with_resource(mut self, resource: impl Into<String>) -> Self {
        self.resource = Some(resource.into());
        self
    }

    /// Adds a subject condition.
    #[must_use]
    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// Returns true when this pattern matches the key.
    #[must_use]
    pub fn matches(&self, key: &BudgetKey) -> bool {
        self.provider == key.provider
            && matches_optional(self.domain.as_deref(), key.domain())
            && matches_optional(self.resource.as_deref(), key.resource())
            && matches_optional(self.subject.as_deref(), key.subject())
    }

    /// Returns the number of exact fields in the pattern.
    #[must_use]
    pub fn specificity(&self) -> u8 {
        [
            self.domain.as_ref(),
            self.resource.as_ref(),
            self.subject.as_ref(),
        ]
        .into_iter()
        .flatten()
        .count()
        .try_into()
        .unwrap_or(3)
    }

    /// Returns the provider component.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Returns the optional domain condition.
    #[must_use]
    pub fn domain(&self) -> Option<&str> {
        self.domain.as_deref()
    }

    /// Returns the optional resource condition.
    #[must_use]
    pub fn resource(&self) -> Option<&str> {
        self.resource.as_deref()
    }

    /// Returns the optional subject condition.
    #[must_use]
    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }

    /// Returns an exact key when this pattern has no wildcards.
    #[must_use]
    pub fn exact_key(&self) -> Option<BudgetKey> {
        Some(BudgetKey {
            provider: self.provider.clone(),
            domain: self.domain.clone()?,
            resource: self.resource.clone()?,
            subject: self.subject.clone()?,
        })
    }

    /// Returns a display key for reports, using `*` for wildcard components.
    #[must_use]
    pub fn report_key(&self) -> BudgetKey {
        BudgetKey {
            provider: self.provider.clone(),
            domain: self.domain.clone().unwrap_or_else(|| "*".to_owned()),
            resource: self.resource.clone().unwrap_or_else(|| "*".to_owned()),
            subject: self.subject.clone().unwrap_or_else(|| "*".to_owned()),
        }
    }
}

fn validate_component(name: &str, value: &str) -> Result<(), BudgetError> {
    if value.trim().is_empty() {
        return Err(BudgetError::InvalidRequest(format!(
            "{name} must not be empty"
        )));
    }

    Ok(())
}

fn matches_optional(expected: Option<&str>, actual: &str) -> bool {
    expected.is_none_or(|value| value == actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_rejects_empty_provider() {
        let result = BudgetKey::new("", "search", "google", "global");

        assert!(matches!(result, Err(BudgetError::InvalidRequest(_))));
    }

    #[test]
    fn wildcard_pattern_matches_missing_fields() {
        let key =
            BudgetKey::new("serpapi", "search", "google", "global").expect("valid key fixture");
        let pattern = BudgetKeyPattern::new("serpapi").expect("valid pattern fixture");

        assert!(pattern.matches(&key));
        assert_eq!(pattern.specificity(), 0);
    }

    #[test]
    fn exact_pattern_can_reconstruct_key() {
        let pattern = BudgetKeyPattern::new("serpapi")
            .expect("valid pattern fixture")
            .with_domain("search")
            .with_resource("google")
            .with_subject("global");
        let key = pattern.exact_key().expect("exact pattern");
        let report_key = pattern.report_key();

        assert_eq!(pattern.provider(), "serpapi");
        assert_eq!(pattern.domain(), Some("search"));
        assert_eq!(pattern.resource(), Some("google"));
        assert_eq!(pattern.subject(), Some("global"));
        assert_eq!(key.provider(), "serpapi");
        assert_eq!(report_key.subject(), "global");
    }

    #[test]
    fn wildcard_pattern_report_key_uses_stars() {
        let pattern = BudgetKeyPattern::new("serpapi").expect("valid pattern fixture");
        let key = pattern.report_key();

        assert_eq!(key.provider(), "serpapi");
        assert_eq!(key.domain(), "*");
        assert_eq!(key.resource(), "*");
        assert_eq!(key.subject(), "*");
    }
}
