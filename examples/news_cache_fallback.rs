use budget_warden::{
    BudgetBroker, BudgetDecision, BudgetPolicy, BudgetRequest, BudgetUnit, BudgetWarden,
    FallbackAction, MemoryStore,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let policy = BudgetPolicy::builder("newsapi-daily-free-plan")
        .provider("newsapi")
        .domain("news")
        .resource("top-headlines")
        .subject("global")
        .unit(BudgetUnit::Requests)
        .hard_limit(1)
        .calendar_day("UTC")
        .exhausted_action(FallbackAction::UseStaleCache)
        .build()?;
    let warden = BudgetWarden::builder()
        .store(MemoryStore::new())
        .policy(policy)
        .build()?;
    let request = BudgetRequest::builder("newsapi", "news", "top-headlines").build()?;

    match warden.reserve(request).await? {
        BudgetDecision::AllowLive { reservation, .. } => reservation.refund().await?,
        BudgetDecision::DenyLive {
            recommended_action, ..
        } => {
            assert_eq!(recommended_action, FallbackAction::UseStaleCache);
        }
    }

    Ok(())
}
