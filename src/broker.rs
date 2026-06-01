use std::sync::Arc;

use async_trait::async_trait;
use chrono::Duration;

use crate::clock::{Clock, SystemClock};
use crate::decision::{BudgetDecision, DenialReason, FallbackAction};
use crate::error::BudgetError;
use crate::key::BudgetKey;
use crate::pacing::evaluate_policy;
use crate::policy::{BudgetPolicy, FailMode};
use crate::report::BudgetReport;
use crate::request::BudgetRequest;
use crate::reservation::BudgetReservation;
use crate::store::{BudgetStore, CounterUsage, ReserveRequest, StoreKey};
use crate::usage::{UsageQuery, UsageSnapshot, UsageSnapshotInput};

const DEFAULT_RESERVATION_TTL: Duration = Duration::minutes(5);

/// Budget broker interface.
#[async_trait]
pub trait BudgetBroker: Send + Sync {
    /// Authorizes a request without reserving budget.
    async fn authorize(&self, request: BudgetRequest) -> Result<BudgetDecision, BudgetError>;

    /// Atomically reserves budget when live access is allowed.
    async fn reserve(&self, request: BudgetRequest) -> Result<BudgetDecision, BudgetError>;

    /// Returns usage for a budget key.
    async fn usage(&self, query: UsageQuery) -> Result<UsageSnapshot, BudgetError>;

    /// Returns configured policies.
    async fn policies(&self) -> Result<Vec<BudgetPolicy>, BudgetError>;

    /// Returns a report for exact-key policies.
    async fn report(&self) -> Result<BudgetReport, BudgetError>;
}

/// Policy-driven budget governor.
#[derive(Clone)]
pub struct BudgetWarden {
    store: Arc<dyn BudgetStore>,
    policies: PolicyRegistry,
    clock: Arc<dyn Clock>,
    reservation_ttl: Duration,
}

impl BudgetWarden {
    /// Starts building a [`BudgetWarden`].
    pub fn builder() -> BudgetWardenBuilder {
        BudgetWardenBuilder::default()
    }
}

#[async_trait]
impl BudgetBroker for BudgetWarden {
    async fn authorize(&self, request: BudgetRequest) -> Result<BudgetDecision, BudgetError> {
        let Some(policy) = self.policies.find(request.key()) else {
            return Ok(no_policy_decision(&request));
        };
        validate_request_against_policy(&request, policy)?;

        let now = self.clock.now();
        let window = policy.window().bounds_at(now)?;
        let store_key = store_key(policy, request.key());
        let usage = match self.store.usage(&store_key, window, now).await {
            Ok(usage) => usage,
            Err(error) if policy.fail_mode() == FailMode::Open => {
                return Ok(fail_open_decision(policy, &request, window, now, &error));
            }
            Err(error) => return Err(BudgetError::from(error)),
        };
        let evaluation = evaluate_policy(
            policy,
            usage.used(),
            request.amount(),
            request.priority(),
            window,
            now,
        );
        let snapshot = snapshot(policy, request.key(), usage.used(), evaluation, window);

        if let Some(reason) = evaluation.denial_reason() {
            crate::telemetry::denied(&request, reason, &snapshot);
            return Ok(deny(policy, reason, snapshot));
        }

        crate::telemetry::allowed(&request, &snapshot);
        Ok(BudgetDecision::AllowLive {
            reservation: BudgetReservation::preview(
                policy.name().to_owned(),
                request.key().clone(),
                request.amount(),
                request.unit().clone(),
                now + self.reservation_ttl,
            ),
            usage: snapshot,
        })
    }

    async fn reserve(&self, request: BudgetRequest) -> Result<BudgetDecision, BudgetError> {
        let Some(policy) = self.policies.find(request.key()) else {
            return Ok(no_policy_decision(&request));
        };
        validate_request_against_policy(&request, policy)?;

        let now = self.clock.now();
        let window = policy.window().bounds_at(now)?;
        let store_key = store_key(policy, request.key());
        let usage = match self.store.usage(&store_key, window, now).await {
            Ok(usage) => usage,
            Err(error) if policy.fail_mode() == FailMode::Open => {
                return Ok(fail_open_decision(policy, &request, window, now, &error));
            }
            Err(error) => return Err(BudgetError::from(error)),
        };
        let evaluation = evaluate_policy(
            policy,
            usage.used(),
            request.amount(),
            request.priority(),
            window,
            now,
        );
        if let Some(reason) = evaluation.denial_reason() {
            let snapshot = snapshot(policy, request.key(), usage.used(), evaluation, window);
            crate::telemetry::denied(&request, reason, &snapshot);
            return Ok(deny(policy, reason, snapshot));
        }

        let result = self
            .store
            .reserve_if_allowed(ReserveRequest {
                key: &store_key,
                amount: request.amount(),
                hard_limit: policy.hard_limit(),
                effective_limit: evaluation.effective_limit(),
                reservation_ttl: self.reservation_ttl,
                window,
                idempotency_key: request.idempotency_key(),
                now,
            })
            .await;
        let result = match result {
            Ok(result) => result,
            Err(error) if policy.fail_mode() == FailMode::Open => {
                return Ok(fail_open_decision(policy, &request, window, now, &error));
            }
            Err(error) => return Err(BudgetError::from(error)),
        };

        let post_evaluation = evaluate_policy(
            policy,
            result.usage().used(),
            0,
            request.priority(),
            window,
            now,
        );
        let snapshot = snapshot(
            policy,
            request.key(),
            result.usage().used(),
            post_evaluation,
            window,
        );

        if !result.is_allowed() {
            crate::telemetry::denied(&request, DenialReason::AheadOfBudgetPace, &snapshot);
            return Ok(deny(policy, DenialReason::AheadOfBudgetPace, snapshot));
        }

        let Some(store_reservation) = result.reservation() else {
            return Err(BudgetError::StoreError(
                "store allowed reservation without returning reservation data".to_owned(),
            ));
        };

        crate::telemetry::allowed(&request, &snapshot);
        crate::telemetry::reservation_created(&request, &snapshot);
        Ok(BudgetDecision::AllowLive {
            reservation: BudgetReservation::new(
                store_reservation.id(),
                policy.name().to_owned(),
                request.key().clone(),
                request.amount(),
                request.unit().clone(),
                store_reservation.expires_at(),
                Arc::clone(&self.store),
            ),
            usage: snapshot,
        })
    }

    async fn usage(&self, query: UsageQuery) -> Result<UsageSnapshot, BudgetError> {
        let Some(policy) = self.policies.find(query.key()) else {
            return Err(BudgetError::NoMatchingPolicy);
        };

        let now = self.clock.now();
        let window = policy.window().bounds_at(now)?;
        let key = store_key(policy, query.key());
        let usage = self
            .store
            .usage(&key, window, now)
            .await
            .map_err(BudgetError::from)?;
        let evaluation = evaluate_policy(
            policy,
            usage.used(),
            0,
            crate::priority::Priority::Normal,
            window,
            now,
        );

        Ok(snapshot(
            policy,
            query.key(),
            usage.used(),
            evaluation,
            window,
        ))
    }

    async fn policies(&self) -> Result<Vec<BudgetPolicy>, BudgetError> {
        Ok(self.policies.all())
    }

    async fn report(&self) -> Result<BudgetReport, BudgetError> {
        let generated_at = self.clock.now();
        let mut snapshots = Vec::with_capacity(self.policies.len());

        for policy in self.policies.iter() {
            let key = policy.key().report_key();
            let window = policy.window().bounds_at(generated_at)?;
            let store_key = store_key(policy, &key);
            let usage = self
                .store
                .usage(&store_key, window, generated_at)
                .await
                .map_err(BudgetError::from)?;
            let evaluation = evaluate_policy(
                policy,
                usage.used(),
                0,
                crate::priority::Priority::Normal,
                window,
                generated_at,
            );
            snapshots.push(snapshot(policy, &key, usage.used(), evaluation, window));
        }

        Ok(BudgetReport::new(generated_at, snapshots))
    }
}

/// Builder for [`BudgetWarden`].
#[derive(Default)]
#[must_use]
pub struct BudgetWardenBuilder {
    store: Option<Arc<dyn BudgetStore>>,
    policies: Vec<BudgetPolicy>,
    clock: Option<Arc<dyn Clock>>,
    reservation_ttl: Option<Duration>,
}

impl BudgetWardenBuilder {
    /// Sets the budget store.
    pub fn store<S>(mut self, store: S) -> Self
    where
        S: BudgetStore + 'static,
    {
        self.store = Some(Arc::new(store));
        self
    }

    /// Adds a policy.
    pub fn policy(mut self, policy: BudgetPolicy) -> Self {
        self.policies.push(policy);
        self
    }

    /// Sets the clock.
    pub fn clock<C>(mut self, clock: C) -> Self
    where
        C: Clock + 'static,
    {
        self.clock = Some(Arc::new(clock));
        self
    }

    /// Sets reservation time-to-live.
    pub const fn reservation_ttl(mut self, ttl: Duration) -> Self {
        self.reservation_ttl = Some(ttl);
        self
    }

    /// Builds the warden.
    ///
    /// # Errors
    ///
    /// Returns an error when required configuration is missing or invalid.
    pub fn build(self) -> Result<BudgetWarden, BudgetError> {
        let Some(store) = self.store else {
            return Err(BudgetError::ConfigError(
                "budget store must be configured".to_owned(),
            ));
        };
        let reservation_ttl = self.reservation_ttl.unwrap_or(DEFAULT_RESERVATION_TTL);
        if reservation_ttl <= Duration::zero() {
            return Err(BudgetError::ConfigError(
                "reservation ttl must be positive".to_owned(),
            ));
        }

        Ok(BudgetWarden {
            store,
            policies: PolicyRegistry::new(self.policies)?,
            clock: self.clock.unwrap_or_else(|| Arc::new(SystemClock)),
            reservation_ttl,
        })
    }
}

#[derive(Debug, Clone)]
struct PolicyRegistry {
    policies: Vec<BudgetPolicy>,
}

impl PolicyRegistry {
    fn new(policies: Vec<BudgetPolicy>) -> Result<Self, BudgetError> {
        let has_duplicate = policies.iter().enumerate().any(|(index, policy)| {
            policies
                .iter()
                .skip(index + 1)
                .any(|candidate| candidate.name() == policy.name())
        });
        if has_duplicate {
            return Err(BudgetError::ConfigError(
                "policy names must be unique".to_owned(),
            ));
        }

        Ok(Self { policies })
    }

    fn find(&self, key: &BudgetKey) -> Option<&BudgetPolicy> {
        self.policies
            .iter()
            .filter(|policy| policy.key().matches(key))
            .max_by_key(|policy| policy.key().specificity())
    }

    fn all(&self) -> Vec<BudgetPolicy> {
        self.policies.clone()
    }

    fn iter(&self) -> impl Iterator<Item = &BudgetPolicy> {
        self.policies.iter()
    }

    fn len(&self) -> usize {
        self.policies.len()
    }
}

fn validate_request_against_policy(
    request: &BudgetRequest,
    policy: &BudgetPolicy,
) -> Result<(), BudgetError> {
    if request.unit() != policy.unit() {
        return Err(BudgetError::InvalidRequest(
            "request unit does not match policy unit".to_owned(),
        ));
    }

    Ok(())
}

fn store_key(policy: &BudgetPolicy, key: &BudgetKey) -> StoreKey {
    StoreKey::new(policy.name().to_owned(), key.clone(), policy.unit().clone())
}

fn snapshot(
    policy: &BudgetPolicy,
    key: &BudgetKey,
    used: u64,
    evaluation: crate::pacing::PacingEvaluation,
    window: crate::window::WindowBounds,
) -> UsageSnapshot {
    UsageSnapshot::new(UsageSnapshotInput {
        policy_name: policy.name().to_owned(),
        key: key.clone(),
        unit: policy.unit().clone(),
        used,
        hard_limit: policy.hard_limit(),
        safe_to_spend_now: evaluation.safe_to_spend_now(),
        reserved_remaining: evaluation.reserved_remaining(),
        window_start: window.start(),
        window_end: window.end(),
        ahead_of_pace: evaluation.ahead_of_pace(),
    })
}

fn deny(policy: &BudgetPolicy, reason: DenialReason, usage: UsageSnapshot) -> BudgetDecision {
    BudgetDecision::DenyLive {
        reason,
        usage,
        recommended_action: policy
            .low_budget_action()
            .unwrap_or_else(|| policy.exhausted_action())
            .clone(),
    }
}

fn no_policy_decision(request: &BudgetRequest) -> BudgetDecision {
    let now = chrono::Utc::now();
    BudgetDecision::DenyLive {
        reason: DenialReason::NoMatchingPolicy,
        usage: UsageSnapshot::new(UsageSnapshotInput {
            policy_name: String::new(),
            key: request.key().clone(),
            unit: request.unit().clone(),
            used: 0,
            hard_limit: 0,
            safe_to_spend_now: 0,
            reserved_remaining: 0,
            window_start: now,
            window_end: now + Duration::seconds(1),
            ahead_of_pace: false,
        }),
        recommended_action: FallbackAction::Reject,
    }
}

fn fail_open_decision(
    policy: &BudgetPolicy,
    request: &BudgetRequest,
    window: crate::window::WindowBounds,
    now: chrono::DateTime<chrono::Utc>,
    error: &crate::error::StoreError,
) -> BudgetDecision {
    crate::telemetry::store_unavailable(policy, request, error);
    let evaluation = evaluate_policy(
        policy,
        CounterUsage::default().used(),
        request.amount(),
        request.priority(),
        window,
        now,
    );
    let usage = snapshot(policy, request.key(), 0, evaluation, window);
    crate::telemetry::allowed(request, &usage);
    BudgetDecision::AllowLive {
        reservation: BudgetReservation::preview(
            policy.name().to_owned(),
            request.key().clone(),
            request.amount(),
            request.unit().clone(),
            now + Duration::seconds(1),
        ),
        usage,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};

    use super::*;
    use crate::StoreError;
    use crate::clock::tests_support::FixedClock;
    use crate::decision::FallbackAction;
    use crate::policy::{BudgetStrategy, PreserveForWindow};
    use crate::priority::Priority;
    use crate::store::{BudgetStore, CounterUsage, MemoryStore};
    use crate::unit::BudgetUnit;
    use crate::window::WindowBounds;

    fn policy() -> BudgetPolicy {
        BudgetPolicy::builder("serpapi-monthly")
            .provider("serpapi")
            .domain("search")
            .resource("google")
            .subject("global")
            .unit(BudgetUnit::Requests)
            .hard_limit(10)
            .calendar_month("UTC")
            .strategy(BudgetStrategy::HardLimitOnly)
            .exhausted_action(FallbackAction::UseStaleCache)
            .build()
            .expect("valid policy")
    }

    fn request() -> BudgetRequest {
        BudgetRequest::builder("serpapi", "search", "google")
            .subject("global")
            .unit(BudgetUnit::Requests)
            .amount(1)
            .priority(Priority::Normal)
            .build()
            .expect("valid request")
    }

    fn test_clock() -> FixedClock {
        FixedClock::new(Utc::now())
    }

    #[derive(Debug, Clone, Copy)]
    struct FailingStore;

    #[async_trait]
    impl BudgetStore for FailingStore {
        async fn reserve_if_allowed(
            &self,
            _request: ReserveRequest<'_>,
        ) -> Result<crate::store::StoreReservationResult, StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn commit(
            &self,
            _reservation_id: crate::reservation::ReservationId,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn refund(
            &self,
            _reservation_id: crate::reservation::ReservationId,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn usage(
            &self,
            _key: &StoreKey,
            _window: WindowBounds,
            _now: chrono::DateTime<Utc>,
        ) -> Result<CounterUsage, StoreError> {
            Err(StoreError::Unavailable)
        }
    }

    #[tokio::test]
    async fn reserve_allows_and_commits_usage() {
        let warden = BudgetWarden::builder()
            .store(MemoryStore::new())
            .policy(policy())
            .clock(test_clock())
            .build()
            .expect("warden builds");

        let decision = warden.reserve(request()).await.expect("reserve succeeds");

        let BudgetDecision::AllowLive { reservation, .. } = decision else {
            panic!("expected allow");
        };
        reservation.commit().await.expect("commit succeeds");

        let usage = warden
            .usage(UsageQuery::new(request().key().clone()))
            .await
            .expect("usage succeeds");
        assert_eq!(usage.used(), 1);
    }

    #[tokio::test]
    async fn authorize_does_not_reserve_usage() {
        let warden = BudgetWarden::builder()
            .store(MemoryStore::new())
            .policy(policy())
            .clock(test_clock())
            .build()
            .expect("warden builds");

        let decision = warden
            .authorize(request())
            .await
            .expect("authorize succeeds");

        assert!(matches!(decision, BudgetDecision::AllowLive { .. }));
        let usage = warden
            .usage(UsageQuery::new(request().key().clone()))
            .await
            .expect("usage succeeds");
        assert_eq!(usage.used(), 0);
    }

    #[tokio::test]
    async fn no_matching_policy_returns_denial() {
        let warden = BudgetWarden::builder()
            .store(MemoryStore::new())
            .policy(policy())
            .clock(test_clock())
            .build()
            .expect("warden builds");
        let request = BudgetRequest::builder("other", "search", "google")
            .build()
            .expect("valid request");

        let decision = warden.reserve(request).await.expect("reserve succeeds");

        assert!(matches!(
            decision,
            BudgetDecision::DenyLive {
                reason: DenialReason::NoMatchingPolicy,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn preserve_for_window_denies_when_ahead() {
        let policy = BudgetPolicy::builder("paced")
            .provider("serpapi")
            .domain("search")
            .resource("google")
            .subject("global")
            .unit(BudgetUnit::Requests)
            .hard_limit(10)
            .calendar_month("UTC")
            .strategy(BudgetStrategy::PreserveForWindow(PreserveForWindow::new(
                0, 20, None,
            )))
            .exhausted_action(FallbackAction::UseStaleCache)
            .build()
            .expect("valid policy");
        let warden = BudgetWarden::builder()
            .store(MemoryStore::new())
            .policy(policy)
            .clock(FixedClock::new(
                Utc.with_ymd_and_hms(2026, 6, 1, 0, 1, 0)
                    .single()
                    .expect("valid timestamp"),
            ))
            .reservation_ttl(Duration::minutes(5))
            .build()
            .expect("warden builds");

        let decision = warden.reserve(request()).await.expect("reserve succeeds");

        assert!(matches!(
            decision,
            BudgetDecision::DenyLive {
                reason: DenialReason::AheadOfBudgetPace,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn builder_validates_required_configuration() {
        let missing_store = BudgetWarden::builder().policy(policy()).build();
        let duplicate_policies = BudgetWarden::builder()
            .store(MemoryStore::new())
            .policy(policy())
            .policy(policy())
            .build();
        let invalid_ttl = BudgetWarden::builder()
            .store(MemoryStore::new())
            .policy(policy())
            .reservation_ttl(Duration::zero())
            .build();

        assert!(matches!(missing_store, Err(BudgetError::ConfigError(_))));
        assert!(matches!(
            duplicate_policies,
            Err(BudgetError::ConfigError(_))
        ));
        assert!(matches!(invalid_ttl, Err(BudgetError::ConfigError(_))));
    }

    #[tokio::test]
    async fn broker_exposes_policies_and_rejects_unit_mismatch() {
        let warden = BudgetWarden::builder()
            .store(MemoryStore::new())
            .policy(policy())
            .clock(test_clock())
            .build()
            .expect("warden builds");
        let request = BudgetRequest::builder("serpapi", "search", "google")
            .subject("global")
            .unit(BudgetUnit::Tokens)
            .build()
            .expect("valid request");

        let policies = warden.policies().await.expect("policies succeed");
        let decision = warden.reserve(request).await;

        assert_eq!(policies.len(), 1);
        assert!(matches!(decision, Err(BudgetError::InvalidRequest(_))));
    }

    #[tokio::test]
    async fn report_returns_exact_policy_snapshots() {
        let warden = BudgetWarden::builder()
            .store(MemoryStore::new())
            .policy(policy())
            .clock(test_clock())
            .build()
            .expect("warden builds");

        let report = warden.report().await.expect("report succeeds");

        assert_eq!(report.budgets().len(), 1);
        assert_eq!(report.budgets()[0].policy_name(), "serpapi-monthly");
    }

    #[tokio::test]
    async fn fail_open_allows_live_access_when_store_is_unavailable() {
        let policy = BudgetPolicy::builder("serpapi-monthly")
            .provider("serpapi")
            .domain("search")
            .resource("google")
            .subject("global")
            .unit(BudgetUnit::Requests)
            .hard_limit(10)
            .calendar_month("UTC")
            .strategy(BudgetStrategy::HardLimitOnly)
            .exhausted_action(FallbackAction::UseStaleCache)
            .fail_mode(FailMode::Open)
            .build()
            .expect("valid policy");
        let warden = BudgetWarden::builder()
            .store(FailingStore)
            .policy(policy)
            .clock(test_clock())
            .build()
            .expect("warden builds");

        let decision = warden.reserve(request()).await.expect("fail open succeeds");

        assert!(matches!(decision, BudgetDecision::AllowLive { .. }));
    }

    #[tokio::test]
    async fn fail_closed_returns_store_error_when_store_is_unavailable() {
        let warden = BudgetWarden::builder()
            .store(FailingStore)
            .policy(policy())
            .clock(test_clock())
            .build()
            .expect("warden builds");

        let decision = warden.reserve(request()).await;

        assert!(matches!(decision, Err(BudgetError::StoreUnavailable)));
    }

    #[tokio::test]
    async fn report_includes_wildcard_policy_snapshot() {
        let policy = BudgetPolicy::builder("serpapi-wildcard")
            .provider("serpapi")
            .unit(BudgetUnit::Requests)
            .hard_limit(10)
            .calendar_month("UTC")
            .strategy(BudgetStrategy::HardLimitOnly)
            .exhausted_action(FallbackAction::UseStaleCache)
            .build()
            .expect("valid policy");
        let warden = BudgetWarden::builder()
            .store(MemoryStore::new())
            .policy(policy)
            .clock(test_clock())
            .build()
            .expect("warden builds");

        let report = warden.report().await.expect("report succeeds");

        assert_eq!(report.budgets().len(), 1);
        assert_eq!(report.budgets()[0].policy_name(), "serpapi-wildcard");
    }
}
