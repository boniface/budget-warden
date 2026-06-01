/// Priority used when budget is scarce.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Priority {
    /// Background work that should yield first.
    Background,
    /// Ordinary application traffic.
    #[default]
    Normal,
    /// Important user-visible traffic.
    Important,
    /// Critical traffic allowed to consume reserved capacity.
    Critical,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_priority_orders_above_normal() {
        assert!(Priority::Critical > Priority::Normal);
    }
}
