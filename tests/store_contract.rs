use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use budget_warden::{
    BudgetStore, BudgetUnit, CounterUsage, IdempotencyKey, MemoryStore, ReserveRequest, StoreKey,
};
use chrono::{Duration, Utc};

static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(1);

fn store_key(suffix: &str) -> StoreKey {
    let key =
        budget_warden::BudgetKey::new("serpapi", "search", "google", format!("global-{suffix}"))
            .expect("valid key");
    StoreKey::new(format!("policy-{suffix}"), key, BudgetUnit::Requests)
}

fn window() -> budget_warden::WindowBounds {
    let start = Utc::now();
    budget_warden::WindowBounds::new(start, start + Duration::hours(1)).expect("valid window")
}

async fn exercise_store_contract(store: &dyn BudgetStore) {
    let suffix = unique_suffix("contract");
    let key = store_key(&suffix);
    let window = window();
    let now = Utc::now();
    let idempotency_key =
        IdempotencyKey::new(format!("contract-request-{suffix}")).expect("valid key");
    let request = ReserveRequest {
        key: &key,
        amount: 5,
        hard_limit: 10,
        effective_limit: 10,
        reservation_ttl: Duration::minutes(5),
        window,
        idempotency_key: Some(&idempotency_key),
        now,
    };

    let first = store
        .reserve_if_allowed(request)
        .await
        .expect("first reservation succeeds");
    let second = store
        .reserve_if_allowed(request)
        .await
        .expect("idempotent reservation succeeds");

    assert_eq!(
        first.reservation().map(budget_warden::StoreReservation::id),
        second
            .reservation()
            .map(budget_warden::StoreReservation::id)
    );
    assert_eq!(second.usage(), CounterUsage::new(0, 5));

    let reservation = first.reservation().expect("allowed reservation");
    store
        .commit(reservation.id())
        .await
        .expect("commit succeeds");
    store
        .commit(reservation.id())
        .await
        .expect("commit is idempotent");

    let usage = store
        .usage(&key, window, Utc::now())
        .await
        .expect("usage succeeds");
    assert_eq!(usage, CounterUsage::new(5, 0));
}

async fn exercise_expired_commit_contract(store: &dyn BudgetStore) {
    let suffix = unique_suffix("expired");
    let key = store_key(&suffix);
    let window = window();
    let now = Utc::now() - Duration::seconds(2);
    let result = store
        .reserve_if_allowed(ReserveRequest {
            key: &key,
            amount: 1,
            hard_limit: 10,
            effective_limit: 10,
            reservation_ttl: Duration::seconds(1),
            window,
            idempotency_key: None,
            now,
        })
        .await
        .expect("reservation succeeds");
    let reservation = result.reservation().expect("allowed reservation");

    let commit = store.commit(reservation.id()).await;

    assert_eq!(commit, Err(budget_warden::StoreError::ReservationExpired));
}

#[tokio::test]
async fn memory_store_satisfies_contract() {
    let store = MemoryStore::new();

    exercise_store_contract(&store).await;
    exercise_expired_commit_contract(&store).await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_store_satisfies_contract() {
    use budget_warden::SqliteStore;
    use sqlx_sqlite::SqlitePoolOptions;

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite pool connects");
    let store = SqliteStore::new(pool);
    store.setup_schema().await.expect("schema setup succeeds");

    exercise_store_contract(&store).await;
    exercise_expired_commit_contract(&store).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_store_satisfies_contract_when_configured() {
    use budget_warden::PostgresStore;
    use sqlx_postgres::PgPoolOptions;

    let Some(url) = live_service_url("BUDGET_WARDEN_POSTGRES_URL") else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("postgres pool connects");
    let store = PostgresStore::new(pool);
    store.setup_schema().await.expect("schema setup succeeds");

    exercise_store_contract(&store).await;
    exercise_expired_commit_contract(&store).await;
}

#[cfg(feature = "redis")]
#[tokio::test]
async fn redis_store_satisfies_contract_when_configured() {
    use budget_warden::RedisStore;

    let Some(url) = live_service_url("BUDGET_WARDEN_REDIS_URL") else {
        return;
    };
    let config = deadpool_redis::Config::from_url(url);
    let pool = config
        .create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .expect("redis pool builds");
    let store = RedisStore::new(pool);

    exercise_store_contract(&store).await;
    exercise_expired_commit_contract(&store).await;
}

fn live_services_required() -> bool {
    std::env::var("BUDGET_WARDEN_REQUIRE_LIVE")
        .is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "yes"))
}

fn live_service_url(name: &'static str) -> Option<String> {
    match std::env::var(name) {
        Ok(url) => Some(url),
        Err(std::env::VarError::NotPresent | std::env::VarError::NotUnicode(_))
            if !live_services_required() =>
        {
            None
        }
        Err(error) => {
            panic!("{name} must be set for live integration tests: {error}");
        }
    }
}

fn unique_suffix(label: &str) -> String {
    let sequence = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{label}-{}-{sequence}-{timestamp}", std::process::id())
}
