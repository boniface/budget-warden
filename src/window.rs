use chrono::{DateTime, Datelike, Duration, LocalResult, TimeZone, Utc};
use chrono_tz::Tz;

use crate::error::BudgetError;

/// Budget reset window.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetWindow {
    /// Calendar day in a named time zone.
    CalendarDay { timezone: String },
    /// Calendar month in a named time zone.
    CalendarMonth { timezone: String },
    /// Fixed-duration window.
    Fixed { duration: Duration },
    /// Rolling-duration window.
    Rolling { duration: Duration },
}

impl BudgetWindow {
    /// Creates a calendar-day window.
    #[must_use]
    pub fn calendar_day(timezone: impl Into<String>) -> Self {
        Self::CalendarDay {
            timezone: timezone.into(),
        }
    }

    /// Creates a calendar-month window.
    #[must_use]
    pub fn calendar_month(timezone: impl Into<String>) -> Self {
        Self::CalendarMonth {
            timezone: timezone.into(),
        }
    }

    /// Calculates active window bounds for a timestamp.
    ///
    /// # Errors
    ///
    /// Returns an error when the time zone or local boundary is invalid.
    pub fn bounds_at(&self, now: DateTime<Utc>) -> Result<WindowBounds, BudgetError> {
        match self {
            Self::CalendarDay { timezone } => calendar_day_bounds(timezone, now),
            Self::CalendarMonth { timezone } => calendar_month_bounds(timezone, now),
            Self::Fixed { duration } | Self::Rolling { duration } => fixed_bounds(*duration, now),
        }
    }

    /// Validates static window configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the time zone or duration is invalid.
    pub fn validate(&self) -> Result<(), BudgetError> {
        match self {
            Self::CalendarDay { timezone } | Self::CalendarMonth { timezone } => {
                parse_timezone(timezone).map(|_| ())
            }
            Self::Fixed { duration } | Self::Rolling { duration } => {
                if *duration <= Duration::zero() {
                    return Err(BudgetError::WindowError(
                        "window duration must be positive".to_owned(),
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Inclusive start and exclusive end of a budget window.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowBounds {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}

impl WindowBounds {
    /// Creates window bounds.
    ///
    /// # Errors
    ///
    /// Returns an error when `end <= start`.
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Self, BudgetError> {
        if end <= start {
            return Err(BudgetError::WindowError(
                "window end must be after window start".to_owned(),
            ));
        }

        Ok(Self { start, end })
    }

    /// Returns the inclusive start timestamp.
    #[must_use]
    pub const fn start(&self) -> DateTime<Utc> {
        self.start
    }

    /// Returns the exclusive end timestamp.
    #[must_use]
    pub const fn end(&self) -> DateTime<Utc> {
        self.end
    }

    /// Returns the reset timestamp.
    #[must_use]
    pub const fn resets_at(&self) -> DateTime<Utc> {
        self.end
    }

    /// Returns the window duration.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.end - self.start
    }
}

fn calendar_day_bounds(timezone: &str, now: DateTime<Utc>) -> Result<WindowBounds, BudgetError> {
    let tz = parse_timezone(timezone)?;
    let local = now.with_timezone(&tz);
    let start_local = local_boundary(tz, local.year(), local.month(), local.day())?;
    let end_local = start_local + Duration::days(1);
    WindowBounds::new(
        start_local.with_timezone(&Utc),
        end_local.with_timezone(&Utc),
    )
}

fn calendar_month_bounds(timezone: &str, now: DateTime<Utc>) -> Result<WindowBounds, BudgetError> {
    let tz = parse_timezone(timezone)?;
    let local = now.with_timezone(&tz);
    let start_local = local_boundary(tz, local.year(), local.month(), 1)?;
    let (next_year, next_month) = if local.month() == 12 {
        (local.year() + 1, 1)
    } else {
        (local.year(), local.month() + 1)
    };
    let end_local = local_boundary(tz, next_year, next_month, 1)?;
    WindowBounds::new(
        start_local.with_timezone(&Utc),
        end_local.with_timezone(&Utc),
    )
}

fn fixed_bounds(duration: Duration, now: DateTime<Utc>) -> Result<WindowBounds, BudgetError> {
    if duration <= Duration::zero() {
        return Err(BudgetError::WindowError(
            "fixed window duration must be positive".to_owned(),
        ));
    }

    let duration_seconds = duration.num_seconds();
    let timestamp = now.timestamp();
    let start_timestamp = timestamp - timestamp.rem_euclid(duration_seconds);
    let Some(start) = DateTime::<Utc>::from_timestamp(start_timestamp, 0) else {
        return Err(BudgetError::WindowError(
            "fixed window start is out of range".to_owned(),
        ));
    };
    WindowBounds::new(start, start + duration)
}

fn parse_timezone(timezone: &str) -> Result<Tz, BudgetError> {
    timezone.parse::<Tz>().map_err(|error| {
        BudgetError::WindowError(format!("invalid timezone `{timezone}`: {error}"))
    })
}

fn local_boundary(
    timezone: Tz,
    year: i32,
    month: u32,
    day: u32,
) -> Result<DateTime<Tz>, BudgetError> {
    match timezone.with_ymd_and_hms(year, month, day, 0, 0, 0) {
        LocalResult::Single(value) => Ok(value),
        LocalResult::Ambiguous(earliest, _) => Ok(earliest),
        LocalResult::None => Err(BudgetError::WindowError(format!(
            "local boundary does not exist for {year:04}-{month:02}-{day:02}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn calendar_month_bounds_cross_month_boundary() {
        let now = Utc
            .with_ymd_and_hms(2026, 6, 15, 12, 0, 0)
            .single()
            .expect("valid test timestamp");
        let bounds = BudgetWindow::calendar_month("America/Toronto")
            .bounds_at(now)
            .expect("valid bounds");

        assert_eq!(
            bounds.start(),
            Utc.with_ymd_and_hms(2026, 6, 1, 4, 0, 0)
                .single()
                .expect("valid start")
        );
        assert_eq!(
            bounds.end(),
            Utc.with_ymd_and_hms(2026, 7, 1, 4, 0, 0)
                .single()
                .expect("valid end")
        );
    }

    #[test]
    fn leap_year_february_has_correct_month_end() {
        let now = Utc
            .with_ymd_and_hms(2024, 2, 10, 12, 0, 0)
            .single()
            .expect("valid test timestamp");
        let bounds = BudgetWindow::calendar_month("UTC")
            .bounds_at(now)
            .expect("valid bounds");

        assert_eq!(
            bounds.end(),
            Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0)
                .single()
                .expect("valid end")
        );
    }

    #[test]
    fn invalid_timezone_returns_error() {
        let now = Utc::now();
        let result = BudgetWindow::calendar_day("bad-zone").bounds_at(now);

        assert!(matches!(result, Err(BudgetError::WindowError(_))));
    }

    #[test]
    fn calendar_day_and_fixed_windows_return_bounds() {
        let now = Utc
            .with_ymd_and_hms(2026, 6, 1, 12, 30, 0)
            .single()
            .expect("valid timestamp");
        let day = BudgetWindow::calendar_day("UTC")
            .bounds_at(now)
            .expect("valid day bounds");
        let fixed = BudgetWindow::Fixed {
            duration: Duration::hours(1),
        }
        .bounds_at(now)
        .expect("valid fixed bounds");

        assert_eq!(day.resets_at(), day.end());
        assert_eq!(day.duration(), Duration::days(1));
        assert_eq!(
            fixed.start(),
            Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0)
                .single()
                .expect("valid fixed start")
        );
    }

    #[test]
    fn invalid_window_bounds_and_duration_return_errors() {
        let now = Utc::now();
        let bad_bounds = WindowBounds::new(now, now);
        let bad_fixed = BudgetWindow::Fixed {
            duration: Duration::zero(),
        }
        .bounds_at(now);

        assert!(matches!(bad_bounds, Err(BudgetError::WindowError(_))));
        assert!(matches!(bad_fixed, Err(BudgetError::WindowError(_))));
    }
}
