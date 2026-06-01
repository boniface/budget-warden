use chrono::{DateTime, Utc};

/// Source of current time.
pub trait Clock: Send + Sync {
    /// Returns the current UTC timestamp.
    fn now(&self) -> DateTime<Utc>;
}

/// System clock backed by UTC wall-clock time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use chrono::{DateTime, Utc};

    use super::Clock;

    #[derive(Debug, Clone, Copy)]
    pub(crate) struct FixedClock {
        now: DateTime<Utc>,
    }

    impl FixedClock {
        pub(crate) const fn new(now: DateTime<Utc>) -> Self {
            Self { now }
        }
    }

    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            self.now
        }
    }
}
