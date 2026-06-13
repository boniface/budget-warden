use budget_warden::{
    BudgetBroker, BudgetDecision, BudgetPolicy, BudgetRequest, BudgetStrategy, BudgetUnit,
    BudgetWarden, FallbackAction, MemoryStore, PreserveForWindow, Priority,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let policy = BudgetPolicy::builder("serpapi-monthly-free-plan")
        .provider("serpapi")
        .domain("search")
        .resource("google-search")
        .subject("global")
        .unit(BudgetUnit::Requests)
        .hard_limit(250)
        .calendar_month("Africa/Lusaka")
        .strategy(BudgetStrategy::PreserveForWindow(PreserveForWindow::new(
            10,
            20,
            Some(10),
        )))
        .exhausted_action(FallbackAction::UseStaleCache)
        .build()?;
    let warden = BudgetWarden::builder()
        .store(MemoryStore::new())
        .policy(policy)
        .build()?;
    let request = BudgetRequest::builder("serpapi", "search", "google-search")
        .subject("global")
        .unit(BudgetUnit::Requests)
        .amount(1)
        .priority(Priority::Normal)
        .build()?;

    if let BudgetDecision::AllowLive { reservation, .. } = warden.reserve(request).await? {
        reservation.commit().await?;
    }

    Ok(())
}
