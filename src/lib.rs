//! Policy-based protection for scarce external API budgets.
//!
//! `budget-warden` helps applications decide whether a live third-party API
//! call should spend budget now or use a fallback path. It is designed for
//! scarce provider budgets such as monthly request quotas, daily free tiers,
//! token allowances, credit balances, or other externally imposed limits.
//!
//! The crate separates policy decisions from application behavior. A denial is
//! returned as [`BudgetDecision::DenyLive`], allowing the host application to
//! choose whether to serve cached data, queue work, downgrade quality, use a
//! cheaper provider, or reject the request.
//!
//! # Basic flow
//!
//! 1. Build one or more [`BudgetPolicy`] values.
//! 2. Create a store, such as [`MemoryStore`] for local development or a
//!    production backend behind an optional feature.
//! 3. Build a [`BudgetWarden`].
//! 4. Call [`BudgetBroker::reserve`] before the live provider call.
//! 5. Commit the returned reservation if the live call is sent, or refund it if
//!    the live call is not sent.
//!
//! # Example
//!
//! ```no_run
//! use budget_warden::{
//!     BudgetBroker, BudgetDecision, BudgetPolicy, BudgetRequest, BudgetStrategy, BudgetUnit,
//!     BudgetWarden, FallbackAction, MemoryStore, PreserveForWindow, Priority,
//! };
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let policy = BudgetPolicy::builder("serpapi-monthly-free-plan")
//!     .provider("serpapi")
//!     .domain("search")
//!     .resource("google-search")
//!     .subject("global")
//!     .unit(BudgetUnit::Requests)
//!     .hard_limit(250)
//!     .calendar_month("America/Toronto")
//!     .strategy(BudgetStrategy::PreserveForWindow(PreserveForWindow::new(
//!         10,
//!         20,
//!         Some(10),
//!     )))
//!     .exhausted_action(FallbackAction::UseStaleCache)
//!     .build()?;
//!
//! let warden = BudgetWarden::builder()
//!     .store(MemoryStore::new())
//!     .policy(policy)
//!     .build()?;
//!
//! let request = BudgetRequest::builder("serpapi", "search", "google-search")
//!     .subject("global")
//!     .unit(BudgetUnit::Requests)
//!     .amount(1)
//!     .priority(Priority::Normal)
//!     .build()?;
//!
//! match warden.reserve(request).await? {
//!     BudgetDecision::AllowLive { reservation, .. } => {
//!         // Send the live provider request here.
//!         reservation.commit().await?;
//!     }
//!     BudgetDecision::DenyLive {
//!         recommended_action, ..
//!     } => {
//!         // Execute the application fallback represented by recommended_action.
//!         let _ = recommended_action;
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Secrets and infrastructure configuration
//!
//! This crate does not read `.env` files, Kubernetes Secrets, Docker secrets, or
//! cloud secret managers directly. Host applications should load secrets with
//! their existing configuration system, create the database or Redis pool, then
//! pass the ready-to-use pool to a store such as `PostgresStore`, `RedisStore`,
//! or `SqliteStore`.
//!
//! Keeping secret loading in the host application avoids storing raw
//! credentials in budget policy files and lets each deployment use its own
//! secret-management standard.

pub mod broker;
pub mod clock;
#[cfg(feature = "toml")]
pub mod config;
pub mod decision;
pub mod error;
pub mod key;
pub mod pacing;
pub mod policy;
pub mod priority;
pub mod report;
pub mod request;
pub mod reservation;
pub mod store;
pub mod telemetry;
pub mod unit;
pub mod usage;
pub mod window;

pub use broker::{BudgetBroker, BudgetWarden, BudgetWardenBuilder};
pub use clock::{Clock, SystemClock};
pub use decision::{BudgetDecision, DenialReason, FallbackAction};
pub use error::{BudgetError, StoreError};
pub use key::{BudgetKey, BudgetKeyPattern};
pub use pacing::{PacingEvaluation, evaluate_policy};
pub use policy::{
    BudgetPolicy, BudgetPolicyBuilder, BudgetStrategy, FailMode, PreserveForWindow, ReserveCapacity,
};
pub use priority::Priority;
pub use report::BudgetReport;
pub use request::{BudgetRequest, BudgetRequestBuilder};
pub use reservation::{BudgetReservation, IdempotencyKey, ReservationId};
#[cfg(feature = "memory")]
pub use store::MemoryStore;
#[cfg(feature = "postgres")]
pub use store::PostgresStore;
#[cfg(feature = "redis")]
pub use store::RedisStore;
#[cfg(feature = "sqlite")]
pub use store::SqliteStore;
pub use store::{
    BudgetStore, CounterUsage, ReserveRequest, StoreKey, StoreReservation, StoreReservationResult,
};
pub use unit::BudgetUnit;
pub use usage::{UsageQuery, UsageSnapshot, UsageSnapshotInput};
pub use window::{BudgetWindow, WindowBounds};
